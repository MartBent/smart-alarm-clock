# Smart Alarm Clock

A custom-hardware, embedded-Rust, Home-Assistant-aware bedside **smart alarm clock**.
Aesthetic: **"dark & silent until summoned"** — a minimal wood-veneer bar that shows
nothing until a proximity gesture reveals the time through the veneer. Fully integrated
with Home Assistant, but fires alarms **on-device** so it works even if WiFi/HA is down.

> **Status:** light scaffold. The firmware is structured but the core logic is left
> as `TODO`s — the project owner is learning embedded Rust and wants to write it.

## Hardware (v1)

ESP32-S3-WROOM-1 · DS3231 RTC + supercap · NTP primary · USB-C mains (no battery) ·
warm APA102/SK9822 dot-matrix behind veneer (off when idle) · VCNL4040 (proximity +
lux) · 3 rear buttons · passive piezo (RTTTL); I²S pads reserved for v2.

## Repository layout

```
software/   Rust firmware (esp-idf, ESP32-S3) — the Cargo project lives here
hardware/   KiCad schematic + PCB + fab outputs (populated after bench validation)
docs/       detailed design + architecture + toolchain
```

## Docs

- [`docs/handoff.md`](docs/handoff.md) — full locked design, rationale, open questions, v2 hooks
- [`docs/architecture.md`](docs/architecture.md) — four-thread firmware structure + key behaviours
- [`docs/toolchain.md`](docs/toolchain.md) — espup setup, build/flash, esp-idf config to generate

## Build sequence (short)

1. **Bench validation first (~€60)** — prove IR-through-veneer, the proximity gesture, and
   the warm glow on a dev-kit before committing to a PCB.
2. Validate firmware on the breadboard (gestures, presets in NVS, web UI/captive portal/mDNS,
   on-device firing, MQTT discovery/parity/LWT, RTC + NTP).
3. KiCad schematic → ERC → 2-layer layout → DRC → Gerbers → order.
4. Bring up the bare board incrementally; iterate the veneer enclosure.

See [`docs/handoff.md`](docs/handoff.md) for the detailed version and open questions.
