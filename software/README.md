# Software

The device firmware plus everything that talks to it off-device.

```
firmware/       Rust (esp-idf) firmware for the ESP32-S3 — the device itself
homeassistant/  Home Assistant custom integration (Python) — see homeassistant/README.md
tools/          sim_device.py — a Python simulator (REST + SSE) for testing HA with no hardware
```

## Firmware

Lives in `firmware/` (entry point `src/main.rs`). `std` path (`esp-idf-hal` + `esp-idf-svc`),
FreeRTOS exposed as `std::thread` — worker threads over shared state behind `Arc<Mutex<…>>`.
Every input (button, REST, MQTT) pushes the same `Command`s onto one bus; the alarm core is the
sole writer.

| Worker | Role |
| --- | --- |
| alarm | **Source of truth** — 8-slot model, state machine, firing, snooze/dismiss. Never blocks on network. |
| net | SoftAP + captive portal, HTTP REST API + web UI, SSE push (:81), mDNS, MQTT discovery/LWT. |
| button | BOOT button → commands. |
| buzzer | Passive buzzer via LEDC — beeps while ringing. |
| led | Onboard WS2812 phase colors (status LED). |

**Alarm model:** a fixed pool of 8 slots (each a time-of-day + `enabled`). Arming is implicit
(armed whenever any slot is enabled); dismiss is one-shot (disables the slot that fired).

Working: alarm core, SNTP wall-clock firing, web UI + captive portal, MQTT + custom HA integration.
Bench TODOs: DS3231 RTC read, MAX7219 time render, capacitive-touch input, NVS persistence.

## Toolchain / build

Xtensa ESP32-S3 on the `std`/esp-idf path — needs the Espressif Rust fork (not stock `rustup`):

```sh
cargo install espup ldproxy espflash cargo-espflash --locked
espup install                 # installs the esp/xtensa toolchain + LLVM
. $HOME/export-esp.sh         # exports env each shell (source it, or add to your profile)
cd firmware
cargo run                     # builds for esp32s3 and flashes over USB-C
```

`firmware/rust-toolchain.toml` pins the `esp` channel; `firmware/.cargo/config.toml` sets the
`xtensa-esp32s3-espidf` target + `espflash flash --monitor` runner, so a plain `cargo run` builds
and flashes. In **RustRover**, the **Flash + monitor** run config (`.run/`) does the same. The
esp-idf build config (`.cargo/config.toml`, `rust-toolchain.toml`, `sdkconfig.defaults`, `build.rs`)
is already included; the first build downloads ESP-IDF `v5.2.3` (slow once, then fast).

**TTGO T-Display (classic ESP32) test board:** `cargo build-ttgo` / `cargo run-ttgo`
(= `cargo run --target xtensa-esp32-espidf`). The default `cargo run` stays on the ESP32-S3 target.

> **Flashing won't connect?** (`espflash`: "Error while connecting to device") — almost always a
> **charge-only USB cable**; use a data cable. Sanity-check with `espflash board-info --port <PORT>`
> (this board reports **ESP32-S3**).

## Home Assistant integration

A **broker-free** custom integration (REST + SSE realtime, mDNS discovery, ports 80/81) that adds
the clock to Home Assistant. Install steps + entity list: **[homeassistant/README.md](homeassistant/README.md)**.
MQTT discovery + LWT is also wired from the firmware as a second path.

## Simulator

`tools/sim_device.py` (stdlib-only Python) reproduces the device's REST API + SSE stream + state
machine, so the HA integration can be developed and tested with no hardware.
