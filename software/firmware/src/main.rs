//! Smart alarm clock — ESP32-S3 (esp-idf std).
//!
//! Worker-thread architecture (each subsystem is a `std::thread` / FreeRTOS
//! task). `main` takes the peripherals, builds the shared state + command bus,
//! spawns the workers, and supervises.
//!
//!   alarm   — source of truth: state machine + presets (drains the command bus)
//!   button  — BOOT button -> commands
//!   led     — renders the current phase on the onboard WS2812
//!   display — draws the time as HH:MM on the MAX7219 32x8 matrix (SPI)
//!   buzzer  — passive buzzer (LEDC/PWM on GPIO4): plays the melody while ringing
//!   rtc     — DS3231 (I2C): seeds the clock at boot, persists SNTP time back
//!   net     — SoftAP + HTTP REST API -> commands + state
//!
//! Every input transport (button + REST now; MQTT/HA later) submits the same
//! `Command`s onto the bus. See docs/handoff.md for the design.

mod alarm;
mod button;
mod buzzer;
mod display;
mod dns;
mod led;
mod mqtt;
mod net;
mod rtc;
mod state;

use std::thread::Builder;

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;

fn main() {
    // Required once at startup on the esp-idf std path.
    esp_idf_sys::link_patches();
    // Route the `log` crate to the ESP-IDF logger (levels, tags, timestamps).
    EspLogger::initialize_default();

    // Local timezone (Europe/Amsterdam) so localtime() gives wall-clock time
    // with DST once SNTP has set the system clock. TODO: make configurable.
    std::env::set_var("TZ", "CET-1CEST,M3.5.0,M10.5.0/3");
    unsafe { esp_idf_sys::tzset() };

    log::info!(target: "main", "smart-alarm-clock booting");

    let peripherals = Peripherals::take().expect("take peripherals");
    let shared = state::new_shared();
    let bus = state::new_bus();

    // RTC worker (DS3231 over I2C: SDA=GPIO8, SCL=GPIO9). Spawned first so it can
    // seed the system clock from the RTC before the other workers read the time.
    {
        let i2c0 = peripherals.i2c0;
        let sda = peripherals.pins.gpio8;
        let scl = peripherals.pins.gpio9;
        Builder::new()
            .name("rtc".into())
            .stack_size(4 * 1024)
            .spawn(move || rtc::run(i2c0, sda, scl))
            .expect("spawn rtc worker");
    }

    // Alarm core (source of truth) — drains the command bus.
    {
        let shared = shared.clone();
        let bus = bus.clone();
        Builder::new()
            .name("alarm".into())
            .stack_size(8 * 1024)
            .spawn(move || alarm::run(bus, shared))
            .expect("spawn alarm worker");
    }

    // BOOT-button transport.
    {
        let bus = bus.clone();
        let pin = peripherals.pins.gpio0;
        Builder::new()
            .name("button".into())
            .stack_size(4 * 1024)
            .spawn(move || button::run(pin, bus))
            .expect("spawn button worker");
    }

    // LED (display) worker.
    {
        let shared = shared.clone();
        let channel = peripherals.rmt.channel0;
        let pin = peripherals.pins.gpio48;
        Builder::new()
            .name("led".into())
            .stack_size(8 * 1024)
            .spawn(move || led::run(channel, pin, shared))
            .expect("spawn led worker");
    }

    // Matrix display worker — draws the time as HH:MM on the MAX7219 32x8 panel
    // (SPI2: SCLK=GPIO12, DIN=GPIO11, CS=GPIO10, via the level shifter).
    {
        let shared = shared.clone();
        let spi = peripherals.spi2;
        let sclk = peripherals.pins.gpio12;
        let din = peripherals.pins.gpio11;
        let cs = peripherals.pins.gpio10;
        Builder::new()
            .name("display".into())
            .stack_size(8 * 1024)
            .spawn(move || display::run(spi, sclk, din, cs, shared))
            .expect("spawn display worker");
    }

    // Buzzer worker — passive buzzer via LEDC/PWM on GPIO4 (plays melody while ringing).
    {
        let shared = shared.clone();
        let timer = peripherals.ledc.timer0;
        let channel = peripherals.ledc.channel0;
        let pin = peripherals.pins.gpio4;
        Builder::new()
            .name("buzzer".into())
            .stack_size(4 * 1024)
            .spawn(move || buzzer::run(timer, channel, pin, shared))
            .expect("spawn buzzer worker");
    }

    // Network worker — SoftAP + HTTP REST API.
    {
        let shared = shared.clone();
        let bus = bus.clone();
        let modem = peripherals.modem;
        Builder::new()
            .name("net".into())
            .stack_size(16 * 1024)
            .spawn(move || net::run(modem, shared, bus))
            .expect("spawn net worker");
    }

    log::info!(target: "main", "workers spawned");

    // Supervisor: stay alive so the workers keep running.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
