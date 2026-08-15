//! DS3231 RTC bench test — confirms the chip, wiring, BCD, read, write, and the
//! oscillator, before the RTC is ported into a proper firmware module.
//!
//! Wiring (DS3231 is native 3.3V — NO level shifter):
//!   VCC=3V3  GND=GND  SDA=GPIO8  SCL=GPIO9   (module has onboard pull-ups)
//!
//! Run:
//!   RTC_SET='2026-08-15 14:40:00' cargo build --example rtc   # write, then tick
//!   cargo build --example rtc                                 # read-only
//! then flash the built example and watch it with `espflash monitor`.

use std::thread::sleep;
use std::time::Duration;

use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::log::EspLogger;

const T: &str = "rtc";
const ADDR: u8 = 0x68; // DS3231 I2C address
const REG_SECONDS: u8 = 0x00; // 0x00..=0x06: sec,min,hour,dow,date,month,year (BCD)
const REG_STATUS: u8 = 0x0F; // bit7 = OSF (oscillator stopped since last power-up)

/// Optional compile-time "YYYY-MM-DD HH:MM:SS" to program into the RTC.
const RTC_SET: Option<&str> = option_env!("RTC_SET");

fn bcd_to_dec(b: u8) -> u8 {
    (b >> 4) * 10 + (b & 0x0F)
}
fn dec_to_bcd(d: u8) -> u8 {
    ((d / 10) << 4) | (d % 10)
}

/// (year 0-99, month, date, hour, minute, second) read from the RTC.
fn read_time(i2c: &mut I2cDriver) -> anyhow::Result<(u8, u8, u8, u8, u8, u8)> {
    let mut r = [0u8; 7];
    i2c.write_read(ADDR, &[REG_SECONDS], &mut r, BLOCK)?;
    Ok((
        bcd_to_dec(r[6]),        // year (00-99)
        bcd_to_dec(r[5] & 0x1F), // month (ignore century bit)
        bcd_to_dec(r[4] & 0x3F), // date
        bcd_to_dec(r[2] & 0x3F), // hour (24h: bits 0-5)
        bcd_to_dec(r[1] & 0x7F), // minute
        bcd_to_dec(r[0] & 0x7F), // second
    ))
}

/// Program the time registers (24-hour mode) and clear the OSF status bit.
fn write_time(
    i2c: &mut I2cDriver,
    year: u8,
    month: u8,
    date: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> anyhow::Result<()> {
    // bit6 of the hour register = 0 selects 24-hour mode. Day-of-week is unused
    // by the clock, so 1 is fine.
    i2c.write(
        ADDR,
        &[
            REG_SECONDS,
            dec_to_bcd(second),
            dec_to_bcd(minute),
            dec_to_bcd(hour),
            1,
            dec_to_bcd(date),
            dec_to_bcd(month),
            dec_to_bcd(year),
        ],
        BLOCK,
    )?;
    // Clear OSF (bit7 of status) so a later read knows the time is now valid.
    let mut st = [0u8; 1];
    i2c.write_read(ADDR, &[REG_STATUS], &mut st, BLOCK)?;
    i2c.write(ADDR, &[REG_STATUS, st[0] & 0x7F], BLOCK)?;
    Ok(())
}

/// Parse "YYYY-MM-DD HH:MM:SS" → (year 0-99, month, date, hour, minute, second).
fn parse_set(s: &str) -> Option<(u8, u8, u8, u8, u8, u8)> {
    let mut it = s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
    let year: u32 = it.next()?.parse().ok()?;
    let month = it.next()?.parse().ok()?;
    let date = it.next()?.parse().ok()?;
    let hour = it.next()?.parse().ok()?;
    let minute = it.next()?.parse().ok()?;
    let second = it.next()?.parse().ok()?;
    Some(((year % 100) as u8, month, date, hour, minute, second))
}

fn main() {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();
    let p = Peripherals::take().expect("take peripherals");

    log::info!(target: T, "==== DS3231 RTC BENCH TEST ====");
    let mut i2c = I2cDriver::new(
        p.i2c0,
        p.pins.gpio8, // SDA
        p.pins.gpio9, // SCL
        &I2cConfig::new().baudrate(100u32.kHz().into()),
    )
    .expect("i2c init");

    // 1) Probe the chip.
    let mut probe = [0u8; 1];
    if i2c.write_read(ADDR, &[REG_STATUS], &mut probe, BLOCK).is_err() {
        log::error!(target: T, "no ACK from 0x68 — check SDA=GPIO8, SCL=GPIO9, VCC=3V3, GND");
        loop {
            sleep(Duration::from_secs(3600));
        }
    }
    log::info!(target: T, "DS3231 found at 0x68");

    // 2) Report oscillator-stop flag (set on a fresh chip / after power loss with
    //    no backup cell — means the held time is not trustworthy).
    let osf = probe[0] & 0x80 != 0;
    if osf {
        log::warn!(target: T, "OSF set — RTC lost power / never set; time is invalid until written");
    } else {
        log::info!(target: T, "OSF clear — RTC has been keeping time");
    }

    // 3) Optionally program the time.
    match RTC_SET.and_then(parse_set) {
        Some((y, mo, d, h, mi, s)) => {
            write_time(&mut i2c, y, mo, d, h, mi, s).expect("write RTC");
            log::info!(target: T, "RTC set to 20{y:02}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
        }
        None => {
            if RTC_SET.is_some() {
                log::warn!(target: T, "RTC_SET malformed — expected 'YYYY-MM-DD HH:MM:SS'; skipping write");
            } else {
                log::info!(target: T, "no RTC_SET given — read-only");
            }
        }
    }

    // 4) Read + print once a second so you can watch it tick.
    loop {
        match read_time(&mut i2c) {
            Ok((y, mo, d, h, mi, s)) => {
                log::info!(target: T, "20{y:02}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            }
            Err(e) => log::error!(target: T, "read failed: {e}"),
        }
        sleep(Duration::from_secs(1));
    }
}
