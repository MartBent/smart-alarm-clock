//! RTC worker — DS3231 battery-/supercap-backed real-time clock over I2C.
//!
//! SNTP is the primary time source; the DS3231 covers the gap before WiFi+SNTP
//! is up (and offline operation). Two jobs:
//!   1. At boot: if the RTC holds a valid time, seed the system clock from it so
//!      the display shows the time immediately instead of waiting for SNTP.
//!   2. After SNTP has corrected the system clock: write it back to the RTC
//!      periodically, keeping the RTC disciplined to network time.
//!
//! Times are handled in UTC (the system clock is UTC; TZ turns it into local
//! wall-clock for display). Hardware: native 3.3V, no level shifter —
//! SDA=GPIO8, SCL=GPIO9, VCC=3V3.

use core::time::Duration;
use std::time::Instant;

use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::gpio::{Gpio8, Gpio9};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver, I2C0};
use esp_idf_hal::units::FromValueType;

const ADDR: u8 = 0x68; // DS3231 I2C address
const REG_SECONDS: u8 = 0x00; // 0x00..=0x06: sec,min,hour,dow,date,month,year (BCD)
const REG_STATUS: u8 = 0x0F; // bit7 = OSF (oscillator stopped since last power-up)

/// Unix time below this (~2023-11) is treated as "clock not set yet".
const VALID_AFTER: i64 = 1_700_000_000;
/// How often to write the system clock back to the RTC once time is valid.
const PERSIST_EVERY: Duration = Duration::from_secs(3600);
/// Poll cadence; the first tick also gives SNTP a head start before we persist.
const TICK: Duration = Duration::from_secs(30);

fn bcd_to_dec(b: u8) -> u8 {
    (b >> 4) * 10 + (b & 0x0F)
}
fn dec_to_bcd(d: u8) -> u8 {
    ((d / 10) << 4) | (d % 10)
}

/// Days since the Unix epoch for a proleptic-Gregorian date (Howard Hinnant's
/// algorithm). `m` is 1..=12.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Read the RTC as Unix seconds (UTC), or `None` if the oscillator-stop flag is
/// set (lost power / never programmed) or the value is implausible.
fn read_unix(i2c: &mut I2cDriver) -> Option<i64> {
    let mut st = [0u8; 1];
    i2c.write_read(ADDR, &[REG_STATUS], &mut st, BLOCK).ok()?;
    if st[0] & 0x80 != 0 {
        return None; // OSF set → time not trustworthy
    }
    let mut r = [0u8; 7];
    i2c.write_read(ADDR, &[REG_SECONDS], &mut r, BLOCK).ok()?;
    let sec = bcd_to_dec(r[0] & 0x7F) as i64;
    let min = bcd_to_dec(r[1] & 0x7F) as i64;
    let hour = bcd_to_dec(r[2] & 0x3F) as i64; // 24-hour mode
    let date = bcd_to_dec(r[4] & 0x3F) as i64;
    let month = bcd_to_dec(r[5] & 0x1F) as i64; // ignore century bit
    let year = 2000 + bcd_to_dec(r[6]) as i64;
    let unix = days_from_civil(year, month, date) * 86_400 + hour * 3600 + min * 60 + sec;
    (unix >= VALID_AFTER).then_some(unix)
}

/// Write Unix seconds (UTC) into the RTC (24-hour mode) and clear the OSF flag.
fn write_unix(i2c: &mut I2cDriver, unix: i64) -> anyhow::Result<()> {
    // Break down UTC via the C library (matches the system clock's convention).
    let t = unix as esp_idf_sys::time_t;
    let mut tm: esp_idf_sys::tm = unsafe { core::mem::zeroed() };
    unsafe { esp_idf_sys::gmtime_r(&t, &mut tm) };
    // Day-of-week is unused by the clock, so 1 is fine; bit6=0 hour => 24-hour.
    i2c.write(
        ADDR,
        &[
            REG_SECONDS,
            dec_to_bcd(tm.tm_sec as u8),
            dec_to_bcd(tm.tm_min as u8),
            dec_to_bcd(tm.tm_hour as u8),
            1,
            dec_to_bcd(tm.tm_mday as u8),
            dec_to_bcd((tm.tm_mon + 1) as u8),
            dec_to_bcd((tm.tm_year - 100) as u8), // tm_year is years since 1900
        ],
        BLOCK,
    )?;
    let mut st = [0u8; 1];
    i2c.write_read(ADDR, &[REG_STATUS], &mut st, BLOCK)?;
    i2c.write(ADDR, &[REG_STATUS, st[0] & 0x7F], BLOCK)?;
    Ok(())
}

/// Current system time as Unix seconds.
fn sys_unix() -> i64 {
    unsafe { esp_idf_sys::time(core::ptr::null_mut()) as i64 }
}

/// Set the system clock (UTC) to `unix` seconds.
fn set_sys_unix(unix: i64) {
    let tv = esp_idf_sys::timeval {
        tv_sec: unix as esp_idf_sys::time_t,
        tv_usec: 0,
    };
    // SAFETY: valid timeval pointer; null timezone is accepted.
    unsafe { esp_idf_sys::settimeofday(&tv, core::ptr::null()) };
}

pub fn run(i2c0: I2C0, sda: Gpio8, scl: Gpio9) {
    let mut i2c = match I2cDriver::new(i2c0, sda, scl, &I2cConfig::new().baudrate(100u32.kHz().into())) {
        Ok(i2c) => i2c,
        Err(e) => {
            log::error!(target: "rtc", "I2C init failed: {e}; RTC disabled");
            return;
        }
    };

    // Boot: seed the system clock from the RTC if it holds a valid time.
    match read_unix(&mut i2c) {
        Some(unix) => {
            set_sys_unix(unix);
            log::info!(target: "rtc", "seeded system clock from RTC (unix {unix})");
        }
        None => log::warn!(target: "rtc", "RTC time invalid (OSF/plausibility); waiting for SNTP"),
    }

    // Discipline loop: once the system clock is valid (SNTP or the RTC seed),
    // persist it back to the RTC on boot and roughly hourly.
    let mut last_persist: Option<Instant> = None;
    loop {
        std::thread::sleep(TICK); // first sleep gives SNTP time to correct the clock
        let now = sys_unix();
        if now < VALID_AFTER {
            continue; // still no valid time from any source
        }
        let due = last_persist.map_or(true, |t| t.elapsed() >= PERSIST_EVERY);
        if due {
            match write_unix(&mut i2c, now) {
                Ok(()) => {
                    log::info!(target: "rtc", "RTC updated from system clock (unix {now})");
                    last_persist = Some(Instant::now());
                }
                Err(e) => log::warn!(target: "rtc", "RTC write failed: {e}"),
            }
        }
    }
}
