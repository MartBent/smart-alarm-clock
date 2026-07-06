//! Smart alarm clock — ESP32-S3, embedded Rust (std / esp-idf).
//!
//! "Dark & silent until summoned." HA-aware but offline-capable; the device is
//! the source of truth and fires alarms on-device even with WiFi/HA down.
//!
//! Minimal entry-point scaffold — build the firmware out from here. See
//! README "Toolchain" for espup / esp-idf setup, and docs/handoff.md for the
//! locked design (four threads: alarm, network, interaction, display).

fn main() {
    // Required once at startup on the esp-idf std path.
    esp_idf_sys::link_patches();

    println!("smart-alarm-clock booting");

    // TODO (you): take peripherals, init buses (I2C/SPI/LEDC/GPIO/NVS), build
    // shared state, and spawn the worker threads.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
