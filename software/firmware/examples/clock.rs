//! Matrix clock demo — draws the real local time as HH:MM on the MAX7219 panel.
//!
//! Bench test for WiFi time sync: joins WiFi (STA), starts SNTP, and renders the
//! network-synced wall-clock time with a 1 Hz blinking colon. A blinking "--:--"
//! is shown until SNTP has set the system clock (usually a few seconds).
//!
//! Set your WiFi creds at compile time, then flash:
//!   WIFI_SSID='YourSSID' WIFI_PASS='YourPass' cargo build --example clock
//!   espflash flash --partition-table partitions.csv target/.../examples/clock
//!
//! The panel is a chain of 4x 8x8 MAX7219 modules; FC-16 modules are physically
//! rotated, so if the result is mirrored / upside-down / module-swapped, flip the
//! orientation constants below instead of re-deriving the mapping.
//!
//! Wiring (via the level shifter, per hardware/README.md):
//!   SCLK=GPIO12  DIN=GPIO11  CS=GPIO10 ; matrix VCC=5V, common GND.

use std::thread::sleep;
use std::time::Duration;

use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::gpio::AnyIOPin;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::config::{Config as SpiConfig, DriverConfig};
use esp_idf_hal::spi::SpiDeviceDriver;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::EspSntp;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

// WiFi credentials, baked in at compile time (not committed). Set them via env:
//   WIFI_SSID='...' WIFI_PASS='...' cargo build --example clock
const WIFI_SSID: Option<&str> = option_env!("WIFI_SSID");
const WIFI_PASS: Option<&str> = option_env!("WIFI_PASS");

// Local timezone (Europe/Amsterdam) with DST, matching the main firmware.
const TZ: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

// Unix time below this (~2023-11) means the clock hasn't been SNTP-set yet.
const VALID_AFTER: esp_idf_sys::time_t = 1_700_000_000;

const CHIPS: usize = 4;
const W: usize = CHIPS * 8; // 32 columns
const H: usize = 8;

// --- orientation knobs (flip these if the display looks wrong) ---
const REVERSE_MODULES: bool = true; // which chip is the leftmost 8 columns
const FLIP_X: bool = false; // column bit-order within a chip (mirror left/right)
const FLIP_ROWS: bool = false; // top vs bottom row = digit register 1 (mirror up/down)
const BRIGHTNESS: u8 = 0x02; // 0x00..0x0F

type Fb = [[bool; W]; H];

// 3x5 digit font — 5 rows, low 3 bits each (bit2 = leftmost column).
const FONT: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b001, 0b001, 0b001], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];

fn clear(fb: &mut Fb) {
    for row in fb.iter_mut() {
        for c in row.iter_mut() {
            *c = false;
        }
    }
}

fn draw_digit(fb: &mut Fb, digit: usize, x0: usize) {
    const Y0: usize = 1; // 5-tall glyph in rows 1..=5
    for (r, bits) in FONT[digit].iter().enumerate() {
        for col in 0..3 {
            if bits & (1 << (2 - col)) != 0 {
                fb[Y0 + r][x0 + col] = true;
            }
        }
    }
}

/// Local wall-clock as (hh, mm, ss), or `None` until SNTP has set the clock.
fn now_local() -> Option<(u32, u32, u32)> {
    unsafe {
        let t = esp_idf_sys::time(core::ptr::null_mut());
        if t < VALID_AFTER {
            return None;
        }
        let mut tm: esp_idf_sys::tm = core::mem::zeroed();
        esp_idf_sys::localtime_r(&t, &mut tm);
        Some((tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32))
    }
}

fn main() {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();

    // Timezone so localtime_r() gives wall-clock time (with DST) once SNTP syncs.
    std::env::set_var("TZ", TZ);
    unsafe { esp_idf_sys::tzset() };

    let p = Peripherals::take().expect("take peripherals");

    // --- WiFi + SNTP: join the network and let SNTP set the system clock. ---
    let sys_loop = EspSystemEventLoop::take().expect("event loop");
    let nvs = EspDefaultNvsPartition::take().expect("nvs partition");
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(p.modem, sys_loop.clone(), Some(nvs)).expect("EspWifi::new"),
        sys_loop,
    )
    .expect("BlockingWifi::wrap");

    let (ssid, pass) = match (WIFI_SSID, WIFI_PASS) {
        (Some(s), Some(p)) if !s.is_empty() => (s, p),
        _ => {
            log::error!(target: "clock", "no WiFi creds — rebuild with WIFI_SSID='..' WIFI_PASS='..'");
            return
        }
    };
    log::info!(target: "clock", "joining WiFi '{ssid}'");
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.try_into().expect("ssid too long"),
        password: pass.try_into().expect("password too long"),
        auth_method: if pass.is_empty() { AuthMethod::None } else { AuthMethod::WPA2Personal },
        ..Default::default()
    }))
    .expect("wifi config");
    wifi.start().expect("wifi start");
    wifi.connect().expect("wifi connect");
    wifi.wait_netif_up().expect("wifi netif up");
    let ip = wifi.wifi().sta_netif().get_ip_info().expect("ip info").ip;
    log::info!(target: "clock", "connected: {ip} — starting SNTP");
    let _sntp = EspSntp::new_default().expect("sntp start");

    let mut spi = SpiDeviceDriver::new_single(
        p.spi2,
        p.pins.gpio12,            // SCLK
        p.pins.gpio11,            // SDO / DIN
        Option::<AnyIOPin>::None, // SDI / MISO — unused
        Some(p.pins.gpio10),      // CS
        &DriverConfig::new(),
        &SpiConfig::new().baudrate(1u32.MHz().into()),
    )
    .expect("spi init");

    // A register command broadcast to all 4 chips (init only).
    let bcast = |reg: u8, val: u8| {
        let mut f = [0u8; CHIPS * 2];
        for i in 0..CHIPS {
            f[i * 2] = reg;
            f[i * 2 + 1] = val;
        }
        f
    };
    spi.write(&bcast(0x0F, 0x00)).ok(); // display-test off
    spi.write(&bcast(0x09, 0x00)).ok(); // no BCD decode (raw segments)
    spi.write(&bcast(0x0B, 0x07)).ok(); // scan all 8
    spi.write(&bcast(0x0A, BRIGHTNESS)).ok();
    spi.write(&bcast(0x0C, 0x01)).ok(); // normal operation

    log::info!(target: "clock", "clock demo running (waiting for SNTP)");
    let mut fb: Fb = [[false; W]; H];
    let mut synced = false;
    loop {
        clear(&mut fb);
        match now_local() {
            Some((hh, mm, ss)) => {
                if !synced {
                    log::info!(target: "clock", "time synced: {hh:02}:{mm:02}:{ss:02}");
                    synced = true;
                }
                draw_digit(&mut fb, (hh / 10) as usize, 5);
                draw_digit(&mut fb, (hh % 10) as usize, 10);
                if ss % 2 == 0 {
                    fb[2][15] = true; // blinking colon
                    fb[4][15] = true;
                }
                draw_digit(&mut fb, (mm / 10) as usize, 19);
                draw_digit(&mut fb, (mm % 10) as usize, 24);
            }
            None => {
                // Not synced yet: blink "--:--" so it's clear time isn't set.
                if unsafe { esp_idf_sys::esp_timer_get_time() } / 500_000 % 2 == 0 {
                    for x0 in [5, 10, 19, 24] {
                        for col in 0..3 {
                            fb[3][x0 + col] = true; // dash at mid-row
                        }
                    }
                    fb[2][15] = true; // colon dots
                    fb[4][15] = true;
                }
            }
        }

        // Flush framebuffer → 4 MAX7219 chips. Mapping: each MAX7219 "digit"
        // register is a ROW; its 8 bits are the columns of that chip's 8x8 block.
        for y in 0..H {
            let dreg = (if FLIP_ROWS { H - y } else { y + 1 }) as u8;
            let mut f = [0u8; CHIPS * 2];
            // First pair clocked in lands in the farthest chip → send pos high→low.
            for (i, pos) in (0..CHIPS).rev().enumerate() {
                let dc = if REVERSE_MODULES { CHIPS - 1 - pos } else { pos };
                let mut b = 0u8;
                for cx in 0..8usize {
                    let x = dc * 8 + cx;
                    if fb[y][x] {
                        let bit = if FLIP_X { cx } else { 7 - cx };
                        b |= 1 << bit;
                    }
                }
                f[i * 2] = dreg;
                f[i * 2 + 1] = b;
            }
            spi.write(&f).ok();
        }

        sleep(Duration::from_millis(500));
    }
}
