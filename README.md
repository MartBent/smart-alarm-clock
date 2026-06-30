# Smart Alarm Clock

A custom-hardware, embedded-Rust, Home-Assistant-aware bedside **smart alarm clock**.
Aesthetic: **"dark & silent until summoned"** — a minimal wood-veneer bar that shows
nothing until a proximity gesture reveals the time through the veneer. Fully integrated
with Home Assistant, but fires alarms **on-device** so it works even if WiFi/HA is down.

> This is a **light scaffold** to start the firmware. The core logic is intentionally
> left as `TODO`s — the project owner is learning embedded Rust and wants to write it.
> See `docs/` for the locked design and the options/trade-offs reference.

## Hardware (v1)

- **MCU:** ESP32-S3-WROOM-1 (native USB-JTAG for program + debug)
- **Time:** NTP (SNTP) primary + DS3231 RTC + supercap backup (no battery)
- **Power:** USB-C mains only, 5V → 3.3V buck (supercap backs the RTC only)
- **Display:** diffused warm dot-matrix LED panel behind thin wood veneer (APA102/SK9822 SPI preferred), driven OFF when idle
- **Interaction:** single VCNL4040 (proximity gesture + ambient lux) over I²C; 3 rear buttons
- **Audio:** passive piezo + RTTTL (one LEDC/PWM GPIO); I²S DAC pads reserved for v2

## Repository layout

```
software/   Rust firmware (esp-idf, ESP32-S3) — the Cargo project lives here
hardware/   KiCad schematic + PCB + fab outputs (populated after bench validation)
docs/       design handoff + options/trade-offs reference
```

## Firmware structure

`std` path: `esp-idf-hal` + `esp-idf-svc` (FreeRTOS underneath, exposed as `std::thread`).
Four threads, with shared state behind `Arc<Mutex<…>>` / channels:

| Thread | File | Role |
| --- | --- | --- |
| Alarm / time | `software/src/alarm.rs` | **Source of truth.** RTC, preset eval, firing, snooze/dismiss. Never blocks on network. |
| Network | `software/src/network.rs` | MQTT (HA discovery + LWT), HTTP web UI, mDNS, AP/captive portal, SNTP. |
| Interaction | `software/src/interaction.rs` | VCNL4040 gesture + lux, rear buttons, brightness. |
| Display | `software/src/display.rs` | Renders time / alarm / preset / armed / dismiss-progress; fades. |
| Shared state | `software/src/state.rs` | Preset model + state machine + settings (NVS-backed). |

## Toolchain setup

This is the Xtensa ESP32-S3 target on the `std`/esp-idf path, so it needs the
Espressif Rust fork (not stock `rustup`). The Cargo project lives in `software/`,
so run the build/flash commands from there (`cd software`):

```sh
# 1. Install the Xtensa Rust toolchain + esp-idf prerequisites
cargo install espup
espup install                 # installs the esp/xtensa toolchain
. $HOME/export-esp.sh         # exports env each shell (source it, or add to your profile)

# 2. Flashing + monitor
cargo install cargo-espflash espflash

# 3. (Optional) regenerate a fresh template to compare against this scaffold
cargo install cargo-generate
cargo generate esp-rs/esp-idf-template cargo   # pick esp32s3, std

# 4. Build / flash / monitor (native USB-C)
cargo build
cargo espflash flash --monitor
```

You will still need the esp-idf build config that `esp-idf-template` generates
(`.cargo/config.toml`, `sdkconfig.defaults`, `rust-toolchain.toml`, `build.rs`).
Those were intentionally **left out of this skeleton** — generate them with the
template above (or copy from it) so you own that setup. Reference: Espressif's
"Embedded Rust on ESP" book (covers WiFi + MQTT).

## Build sequence (from the design)

1. **Bench validation first (~€60):** dev-kit S3 + VCNL4040 + DS3231(supercap) +
   warm matrix + piezo + 3 buttons + wood-veneer samples. Prove IR-through-veneer,
   the proximity gesture, and the warm glow before committing to a PCB.
2. Validate firmware on the breadboard (gestures + guards, presets in NVS, web UI +
   captive portal + mDNS, on-device firing, MQTT discovery/parity/LWT, RTC + NTP).
3. KiCad schematic → ERC → 2-layer layout → DRC → Gerbers → order.
4. Bring up the bare board incrementally; iterate the veneer enclosure.

## Open questions

See `docs/` — display panel sourcing, veneer IR passthrough (bench test), preset
model specifics, enclosure dimensions, budget/fab (JLCPCB vs Aisler).
