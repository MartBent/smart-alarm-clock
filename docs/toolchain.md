# Toolchain & build

The firmware targets the Xtensa **ESP32-S3** on the `std` / ESP-IDF path, so it
needs the Espressif Rust fork (not stock `rustup`). The Cargo project lives in
`software/` — run all build/flash commands from there.

## One-time setup

```sh
# 1. Xtensa Rust toolchain + esp-idf prerequisites
cargo install espup
espup install                 # installs the esp/xtensa toolchain
. $HOME/export-esp.sh         # source each shell (or add to your profile)

# 2. Flashing + serial monitor
cargo install cargo-espflash espflash

# 3. (Optional) generate a fresh template to diff against this scaffold
cargo install cargo-generate
cargo generate esp-rs/esp-idf-template cargo   # pick esp32s3, std
```

## Build / flash / monitor

```sh
cd software
cargo build
cargo espflash flash --monitor   # native USB-C (USB-JTAG)
```

## Build config left to generate

This skeleton deliberately omits the esp-idf build config that
`esp-idf-template` produces:

- `.cargo/config.toml`
- `sdkconfig.defaults`
- `rust-toolchain.toml`
- `build.rs`

Generate them with the template above (or copy them in) so you own that setup.

**Reference:** Espressif's *"Embedded Rust on ESP"* book — covers WiFi + MQTT,
which is the bulk of the network thread.
