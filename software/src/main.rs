//! Smart alarm clock — ESP32-S3, embedded Rust (std / esp-idf), FreeRTOS threads.
//!
//! "Dark & silent until summoned." HA-aware but offline-capable; the device is
//! the source of truth and fires alarms on-device even with WiFi/HA down.
//!
//! Thread layout (see each module for responsibilities). Shared state lives in
//! `state` behind an Arc<Mutex<…>> / channels. The alarm thread must never block
//! on the network thread.
//!
//! This file is a LIGHT SCAFFOLD: it lays out the four-thread structure and
//! leaves the real logic to you. Fill in peripheral init + the bodies in each
//! module. See README "Toolchain" for espup / esp-idf-template setup.

mod alarm;
mod display;
mod interaction;
mod network;
mod state;

fn main() {
    // Required once at startup on the esp-idf std path:
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("smart-alarm-clock booting");

    // TODO (you):
    //   1. Take peripherals (esp_idf_hal::peripherals::Peripherals::take()).
    //   2. Init shared resources: I2C bus (VCNL4040 + DS3231), SPI (LED matrix),
    //      LEDC/PWM (piezo), GPIO (3 rear buttons), NVS (presets + settings).
    //   3. Build the shared state (state.rs) and any command channels.
    //   4. Spawn the four threads below, handing each its peripherals + state.
    //
    // Sketch (uncomment + wire up once the module signatures exist):
    //
    // let _alarm       = std::thread::Builder::new().name("alarm".into())
    //     .spawn(move || alarm::run(/* … */)).unwrap();
    // let _network     = std::thread::Builder::new().name("network".into())
    //     .spawn(move || network::run(/* … */)).unwrap();
    // let _interaction = std::thread::Builder::new().name("interaction".into())
    //     .spawn(move || interaction::run(/* … */)).unwrap();
    // let _display     = std::thread::Builder::new().name("display".into())
    //     .spawn(move || display::run(/* … */)).unwrap();

    // Keep main alive so the spawned threads run.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
