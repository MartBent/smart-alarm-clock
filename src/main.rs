//! Smart alarm clock — ESP32-S3 (esp-idf std).
//!
//! Worker-thread architecture (each subsystem is a `std::thread` / FreeRTOS
//! task). `main` takes the peripherals, builds the shared state + command bus,
//! spawns the workers, and supervises.
//!
//!   alarm  — source of truth: state machine + timekeeping (consumes commands)
//!   button — BOOT button -> commands (first input transport)
//!   led    — renders the current phase on the onboard WS2812
//!
//! Every input transport (button now; web REST + MQTT/HA later) submits the
//! same `Command`s into the alarm core. See docs/handoff.md for the design.

mod alarm;
mod button;
mod led;
mod state;

use std::sync::mpsc;
use std::thread::Builder;

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;

fn main() {
    // Required once at startup on the esp-idf std path.
    esp_idf_sys::link_patches();
    // Route the `log` crate to the ESP-IDF logger (levels, tags, timestamps).
    EspLogger::initialize_default();

    log::info!(target: "main", "smart-alarm-clock booting");

    let peripherals = Peripherals::take().expect("take peripherals");
    let shared = state::new_shared();
    let (tx, rx) = mpsc::channel::<state::Command>();

    // Alarm core (source of truth) — owns the command Receiver.
    {
        let shared = shared.clone();
        Builder::new()
            .name("alarm".into())
            .stack_size(8 * 1024)
            .spawn(move || alarm::run(rx, shared))
            .expect("spawn alarm worker");
    }

    // BOOT-button transport — submits commands.
    {
        let tx = tx.clone();
        let pin = peripherals.pins.gpio0;
        Builder::new()
            .name("button".into())
            .stack_size(4 * 1024)
            .spawn(move || button::run(pin, tx))
            .expect("spawn button worker");
    }

    // LED (display) worker — renders the phase.
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

    // Drop our spare Sender; the button worker holds the live one.
    drop(tx);
    log::info!(target: "main", "workers spawned");

    // Supervisor: stay alive so the workers keep running.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
