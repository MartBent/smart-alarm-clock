//! BOOT-button input worker (GPIO0) — the first command transport.
//!
//! Quick press → [`Command::ButtonShort`]; hold ≥ `LONG_MS` → [`Command::ButtonLong`].
//! Active-low with an internal pull-up, simple debounce. The web REST API and
//! MQTT will submit commands over the same channel later.

use std::time::{Duration, Instant};

use esp_idf_hal::gpio::{Gpio0, Pull};
use esp_idf_hal::gpio::PinDriver;

use crate::state::{submit, Command, CommandBus};

/// Hold at least this long to count as a long press (dismiss).
const LONG_MS: u128 = 1500;
/// Ignore contact bounces shorter than this.
const DEBOUNCE_MS: u128 = 25;
/// Input poll period.
const POLL: Duration = Duration::from_millis(10);

pub fn run(pin: Gpio0, bus: CommandBus) {
    let mut button = PinDriver::input(pin).expect("gpio0 input");
    button.set_pull(Pull::Up).expect("gpio0 pull-up");
    log::info!(target: "button", "worker started (BOOT = GPIO0)");

    // Active-low: pressed pulls the line low.
    let mut pressed_at: Option<Instant> = None;
    loop {
        let down = button.is_low();
        match (down, pressed_at) {
            (true, None) => pressed_at = Some(Instant::now()),
            (false, Some(t)) => {
                pressed_at = None;
                let held = t.elapsed().as_millis();
                if held >= DEBOUNCE_MS {
                    let long = held >= LONG_MS;
                    let cmd = if long {
                        Command::ButtonLong
                    } else {
                        Command::ButtonShort
                    };
                    log::info!(target: "button", "{} press ({held}ms)", if long { "long" } else { "short" });
                    submit(&bus, cmd);
                }
            }
            _ => {}
        }
        std::thread::sleep(POLL);
    }
}
