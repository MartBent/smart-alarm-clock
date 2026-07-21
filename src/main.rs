//! Smart alarm clock — ESP32-S3 (esp-idf std).
//!
//! Worker-thread architecture: each subsystem runs on its own `std::thread`
//! (a FreeRTOS task underneath). `main` takes the peripherals, spawns the
//! workers, and then supervises. Right now there's just the `led` worker
//! (rainbow) to validate the threading model; the alarm core and WiFi will
//! join as their own workers next. See docs/handoff.md for the target design.

mod led;

use esp_idf_hal::peripherals::Peripherals;

fn main() {
    // Required once at startup on the esp-idf std path.
    esp_idf_sys::link_patches();

    println!("smart-alarm-clock booting");

    let peripherals = Peripherals::take().expect("take peripherals");

    // LED worker — owns RMT channel 0 + the onboard WS2812 on GPIO48.
    let led_channel = peripherals.rmt.channel0;
    let led_pin = peripherals.pins.gpio48;
    let _led = std::thread::Builder::new()
        .name("led".into())
        .stack_size(8 * 1024)
        .spawn(move || led::run(led_channel, led_pin))
        .expect("spawn led worker");

    println!("workers spawned");

    // main becomes the supervisor; for now just stay alive so the workers run.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
