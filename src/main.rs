//! Smart alarm clock — ESP32-S3 (esp-idf std).
//!
//! Worker-thread architecture (each subsystem is a `std::thread` / FreeRTOS
//! task). `main` takes the peripherals, builds the shared state + command bus,
//! spawns the workers, and supervises.
//!
//!   alarm  — source of truth: state machine + presets (drains the command bus)
//!   button — BOOT button -> commands
//!   led    — renders the current phase on the onboard WS2812
//!   net    — SoftAP + HTTP REST API -> commands + state
//!
//! Every input transport (button + REST now; MQTT/HA later) submits the same
//! `Command`s onto the bus. See docs/handoff.md for the design.

mod alarm;
mod button;
mod dns;
mod led;
mod mqtt;
mod net;
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
