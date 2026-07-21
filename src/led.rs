//! LED worker thread — drives the onboard WS2812 RGB LED (GPIO48).
//!
//! First worker in the FreeRTOS-thread architecture (see docs/handoff.md): each
//! subsystem runs on its own `std::thread` (a FreeRTOS task). For now this just
//! cycles a rainbow to prove the thread model works end to end; it will grow
//! into the display thread later.

use core::time::Duration;

use esp_idf_hal::gpio::Gpio48;
use esp_idf_hal::rmt::CHANNEL0;
use smart_leds::{
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite,
};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

/// Take ownership of the RMT channel + LED pin and rainbow forever.
/// Runs on its own thread (never returns).
pub fn run(channel: CHANNEL0, pin: Gpio48) {
    let mut led = Ws2812Esp32Rmt::new(channel, pin).expect("init WS2812 RMT driver");
    println!("[led] worker started");

    let mut hue: u8 = 0;
    loop {
        let color = hsv2rgb(Hsv {
            hue,
            sat: 255,
            val: 16,
        });
        led.write([color]).expect("write LED");
        hue = hue.wrapping_add(2);
        std::thread::sleep(Duration::from_millis(20));
    }
}
