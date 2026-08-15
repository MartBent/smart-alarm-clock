# Smart Alarm Clock

A custom-hardware, embedded-Rust, Home-Assistant-aware bedside **smart alarm clock**.
Aesthetic: **"dark & silent until summoned"** — a minimal wood-veneer bar that shows
nothing until a touch reveals the time through the veneer. Fully integrated with Home
Assistant, but fires alarms **on-device** so it works even if WiFi/HA is down.

> Firmware + a Home Assistant integration already work; the current phase is **breadboard
> bench validation** before committing to a PCB.

## Repository layout

| Path | What | Details |
|---|---|---|
| `software/firmware/` | Rust (esp-idf) firmware — the device itself | [software/README.md](software/README.md) |
| `software/homeassistant/` | Home Assistant integration — HACS repo pointer | [README](software/homeassistant/README.md) |
| `software/tools/` | `sim_device.py` — device simulator (REST + SSE) | — |
| `hardware/` | PCB + enclosure + bench BOM & wiring | [hardware/README.md](hardware/README.md) |
| `docs/` | Locked design + reasoning | [handoff.md](docs/handoff.md) |

## Hardware

ESP32-S3 driving a monochrome dot-matrix (time only) behind wood veneer, with **native capacitive
touch through the veneer**, a **DS3231 RTC + supercap**, and a **passive buzzer** — USB-C mains
powered, no battery. Currently in **breadboard bench validation**; a custom PCB + wood-veneer
enclosure come next.

→ Components, bench BOM, wiring, and PCB plan: **[hardware/README.md](hardware/README.md)**

## Software

Rust (`esp-idf`) firmware built from worker threads over a shared state machine — the **alarm core
is the on-device source of truth** (8 alarm slots, fires on the real clock), with an on-device
**web UI + WiFi captive portal** and **two Home Assistant paths** (MQTT discovery + a broker-free
REST/SSE custom integration). A Python **simulator** lets the HA side be developed with no hardware.

→ Architecture, build/flash, and HA integration: **[software/README.md](software/README.md)**

## Design

Locked decisions and the reasoning behind them: **[docs/handoff.md](docs/handoff.md)**.
