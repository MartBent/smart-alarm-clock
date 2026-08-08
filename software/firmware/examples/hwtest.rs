//! Hardware self-test — a quick bench diagnostic for the bench-validation parts.
//!
//! Flash with:  `cargo run --example hwtest`
//!
//! It exercises each peripheral in turn and logs results/hints over serial.
//! Watch it with `espflash monitor --port <PORT>`. This is a throwaway diagnostic
//! at the raw-probe level (I2C scan, an LEDC beep, an LED cycle, a MAX7219 lamp
//! test) — NOT the real drivers, which live in the main firmware. Wire per
//! `hardware/README.md`. Re-flash the main firmware (`cargo run`) when done.

use std::thread::sleep;
use std::time::Duration;

use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::gpio::AnyIOPin;
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::ledc::{config::{Resolution, TimerConfig}, LedcDriver, LedcTimerDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::spi::config::{Config as SpiConfig, DriverConfig};
use esp_idf_hal::spi::SpiDeviceDriver;
use esp_idf_hal::units::FromValueType;
use esp_idf_svc::log::EspLogger;

use smart_leds::{SmartLedsWrite, RGB8};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

const T: &str = "hwtest";

fn banner(name: &str) {
    log::info!(target: T, "──────── {name} ────────");
}

/// One MAX7219 command ({register, value}) broadcast to all 4 cascaded chips.
fn max7219_frame(reg: u8, val: u8) -> [u8; 8] {
    let mut b = [0u8; 8];
    for i in 0..4 {
        b[i * 2] = reg;
        b[i * 2 + 1] = val;
    }
    b
}

fn main() {
    esp_idf_sys::link_patches();
    EspLogger::initialize_default();
    let p = Peripherals::take().expect("take peripherals");

    log::info!(target: T, "==== SMART ALARM CLOCK — HARDWARE SELF-TEST ====");
    sleep(Duration::from_millis(500));

    // 1) Onboard WS2812 status LED (GPIO48) — visual: red → green → blue → off.
    banner("1. onboard RGB LED (GPIO48)");
    {
        let mut led = Ws2812Esp32Rmt::new(p.rmt.channel0, p.pins.gpio48).expect("ws2812 init");
        for (name, c) in [
            ("red", RGB8 { r: 40, g: 0, b: 0 }),
            ("green", RGB8 { r: 0, g: 40, b: 0 }),
            ("blue", RGB8 { r: 0, g: 0, b: 40 }),
        ] {
            log::info!(target: T, "  LED → {name}");
            led.write([c]).ok();
            sleep(Duration::from_millis(500));
        }
        led.write([RGB8 { r: 0, g: 0, b: 0 }]).ok();
        log::info!(target: T, "  PASS if you saw red → green → blue");
    }

    // 2) Buzzer melody (LEDC → BS170 → buzzer). Plays a tune by retuning the LEDC
    //    timer frequency per note. Fixed 10-bit resolution keeps duty ~50% across
    //    the pitch range.
    banner("2. buzzer melody (GPIO4)");
    {
        let timer = LedcTimerDriver::new(
            p.ledc.timer0,
            &TimerConfig::new()
                .resolution(Resolution::Bits10)
                .frequency(1u32.kHz().into()),
        )
        .expect("ledc timer");
        let mut buz = LedcDriver::new(p.ledc.channel0, &timer, p.pins.gpio4).expect("ledc channel");
        let half = buz.get_max_duty() / 2;

        // "Twinkle Twinkle Little Star" — (frequency Hz, duration ms); 0 = rest.
        const MELODY: &[(u32, u64)] = &[
            (523, 350), (523, 350), (784, 350), (784, 350), (880, 350), (880, 350), (784, 700),
            (698, 350), (698, 350), (659, 350), (659, 350), (587, 350), (587, 350), (523, 700),
        ];
        for &(freq, ms) in MELODY {
            if freq == 0 {
                buz.set_duty(0).ok();
            } else {
                // SAFETY: retune timer0 (low-speed) to this note's pitch. The
                // LedcDriver holds the timer immutably, so set the frequency here.
                unsafe {
                    esp_idf_sys::ledc_set_freq(
                        esp_idf_sys::ledc_mode_t_LEDC_LOW_SPEED_MODE,
                        esp_idf_sys::ledc_timer_t_LEDC_TIMER_0,
                        freq,
                    );
                }
                buz.set_duty(half).ok();
            }
            sleep(Duration::from_millis(ms - 40));
            buz.set_duty(0).ok(); // brief gap so repeated notes separate
            sleep(Duration::from_millis(40));
        }
        buz.set_duty(0).ok();
        log::info!(target: T, "  PASS if you heard the melody");
    }

    // 3) I2C bus scan (SDA=GPIO8, SCL=GPIO9) — expect the DS3231 @ 0x68.
    banner("3. I2C scan (SDA=GPIO8, SCL=GPIO9)");
    {
        let mut i2c = I2cDriver::new(
            p.i2c0,
            p.pins.gpio8,
            p.pins.gpio9,
            &I2cConfig::new().baudrate(100u32.kHz().into()),
        )
        .expect("i2c init");

        let mut found = 0u32;
        let mut buf = [0u8; 1];
        for addr in 0x08u8..=0x77 {
            // A 1-byte read ACKs the address on a present device, NACKs (errors) otherwise.
            if i2c.read(addr, &mut buf, BLOCK).is_ok() {
                let what = match addr {
                    0x68 => " ← DS3231 RTC",
                    0x57 => " ← AT24C32 EEPROM (on some DS3231 boards)",
                    _ => "",
                };
                log::info!(target: T, "  device @ 0x{addr:02X}{what}");
                found += 1;
            }
        }
        if found == 0 {
            log::warn!(target: T, "  no I2C devices found — check SDA/SCL, 3V3 power, and pull-ups");
        } else {
            log::info!(target: T, "  PASS if 0x68 (DS3231) is listed above");
        }
    }

    // 4) MAX7219 matrix (SPI: SCLK=GPIO12, DIN=GPIO11, CS=GPIO10, via level shifter).
    //    Lamp test = light every LED, then clear. Raw wiring check, not the renderer.
    banner("4. MAX7219 lamp test (SCLK=12, DIN=11, CS=10)");
    {
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

        spi.write(&max7219_frame(0x0F, 0x00)).ok(); // display-test off
        spi.write(&max7219_frame(0x09, 0x00)).ok(); // no BCD decode (raw segments)
        spi.write(&max7219_frame(0x0A, 0x04)).ok(); // medium brightness
        spi.write(&max7219_frame(0x0B, 0x07)).ok(); // scan all 8 digits/rows
        spi.write(&max7219_frame(0x0C, 0x01)).ok(); // shutdown reg → normal operation
        spi.write(&max7219_frame(0x0F, 0x01)).ok(); // display-test ON → all LEDs on
        log::info!(target: T, "  all LEDs should be ON now (needs the level shifter wired)");
        sleep(Duration::from_millis(1500));
        spi.write(&max7219_frame(0x0F, 0x00)).ok(); // display-test off → clear
        log::info!(target: T, "  PASS if the matrix lit fully, then cleared");
    }

    // 5) Capacitive touch (GPIO14) — left as a stub: raw touch read + threshold
    //    tuning is firmware-author territory. See hardware/README.md. Wire a foil
    //    electrode to GPIO14 and read via esp_idf_sys touch_pad_* (init / config /
    //    fsm_start / read_raw_data) to print a baseline vs finger-present value.
    banner("5. capacitive touch (GPIO14)");
    log::info!(target: T, "  (stub — implement the raw touch read to tune it)");

    log::info!(target: T, "==== SELF-TEST COMPLETE — re-flash the main firmware with `cargo run` ====");
    loop {
        sleep(Duration::from_secs(3600));
    }
}
