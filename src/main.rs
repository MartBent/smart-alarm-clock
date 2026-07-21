//! Smart alarm clock — ESP32-S3 (esp-idf std).
//!
//! Slice C (warm-up): cycle a rainbow on the onboard WS2812 RGB LED (GPIO48)
//! via the RMT peripheral. Proves esp-idf-hal + the LED driver work end to end.
//! Build the real firmware out from here (see docs/handoff.md).

use esp_idf_hal::peripherals::Peripherals;
use smart_leds::{
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite,
};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

fn main() {
    // Required once at startup on the esp-idf std path.
    esp_idf_sys::link_patches();

    println!("smart-alarm-clock booting");

    let peripherals = Peripherals::take().expect("take peripherals");
    // Onboard WS2812 RGB LED (GPIO48 on the YD-ESP32-S3), clocked out via RMT ch0.
    let mut led = Ws2812Esp32Rmt::new(peripherals.rmt.channel0, peripherals.pins.gpio48)
        .expect("init WS2812 RMT driver");

    println!("rainbow start");

    // Sweep the hue continuously; ~2.5 s per full rainbow. Low brightness — the
    // onboard LED is bright.
    let mut hue: u8 = 0;
    loop {
        let color = hsv2rgb(Hsv {
            hue,
            sat: 255,
            val: 16,
        });
        led.write([color]).expect("write LED");
        hue = hue.wrapping_add(2);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
