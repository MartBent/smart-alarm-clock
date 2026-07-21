//! LED (display) worker — renders the current [`Phase`] on the onboard WS2812
//! RGB LED (GPIO48, via RMT). Reads shared state each frame; the alarm core is
//! the sole writer. This is the visible stand-in for the eventual dot-matrix.

use core::time::Duration;

use esp_idf_hal::gpio::Gpio48;
use esp_idf_hal::rmt::CHANNEL0;
use smart_leds::{
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite, RGB8,
};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

use crate::state::{Phase, SharedState};

const FRAME: Duration = Duration::from_millis(50);
const OFF: RGB8 = RGB8 { r: 0, g: 0, b: 0 };

pub fn run(channel: CHANNEL0, pin: Gpio48, shared: SharedState) {
    let mut led = Ws2812Esp32Rmt::new(channel, pin).expect("init WS2812 RMT driver");
    log::info!(target: "led", "worker started");

    let mut hue: u8 = 0; // animates the idle rainbow
    let mut frame: u32 = 0;
    loop {
        let phase = shared.lock().unwrap().phase;
        let color = match phase {
            // Idle: slow rainbow — the device is alive but not armed.
            Phase::Idle => hsv2rgb(Hsv {
                hue,
                sat: 255,
                val: 8,
            }),
            // Armed: steady dim green — waiting for the alarm.
            Phase::Armed => RGB8 { r: 0, g: 10, b: 0 },
            // Ringing: attention-grabbing red flash (~2 Hz).
            Phase::Ringing => {
                if (frame / 5) % 2 == 0 {
                    RGB8 { r: 80, g: 0, b: 0 }
                } else {
                    OFF
                }
            }
            // Snoozed: dim amber, steady.
            Phase::Snoozed => RGB8 { r: 24, g: 8, b: 0 },
        };

        led.write([color]).expect("write LED");
        hue = hue.wrapping_add(1);
        frame = frame.wrapping_add(1);
        std::thread::sleep(FRAME);
    }
}
