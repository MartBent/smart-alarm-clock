# Smart Alarm Clock

A bedside alarm clock built from scratch: custom hardware, firmware in Rust on an
ESP32-S3, and a Home Assistant integration. The idea is a plain wood-veneer bar that
stays dark until you touch it, then shows the time glowing through the veneer. It sits
on the network and works with Home Assistant, but it keeps its own time and fires
alarms itself, so it still rings if WiFi or HA goes down.

Right now it runs on a breadboard. The firmware and the HA integration work; the
custom PCB and the veneer enclosure are still ahead.

## Layout

| Path | What it is |
|---|---|
| `software/firmware/` | Rust (esp-idf) firmware for the device. See [software/README.md](software/README.md). |
| `software/homeassistant/` | Home Assistant integration. It now lives in its own HACS repo; this folder is a pointer. See [README](software/homeassistant/README.md). |
| `software/tools/` | `sim_device.py`, a stand-in for the device over REST + SSE. |
| `hardware/` | PCB, enclosure, and the bench parts list + wiring. See [hardware/README.md](hardware/README.md). |
| `docs/` | Design decisions and the reasoning behind them. See [handoff.md](docs/handoff.md). |

## Hardware

An ESP32-S3 drives a monochrome dot-matrix that shows the time behind wood veneer.
Touch is the ESP32-S3's built-in capacitive sensing through a foil pad, which reads
fine through wood. A DS3231 keeps time while the network is down; on the PCB a
supercap backs it up, so there's no coin cell. Sound is a passive buzzer for now. The
whole thing runs off USB-C, no battery.

It's on a breadboard at the moment. The parts list, wiring, and PCB plan are in
[hardware/README.md](hardware/README.md).

## Software

The firmware is a handful of worker threads around one shared state machine. The
alarm core owns the clock and the eight alarm slots and decides when to ring; nothing
about firing depends on the network. There's an on-device web UI with a WiFi captive
portal for first-time setup, and two ways into Home Assistant: MQTT discovery, and a
broker-free integration that uses the REST API with SSE for live updates. A small
Python simulator stands in for the hardware so the HA side can be worked on without a
device.

Build and flash steps, the thread layout, and the HA details are in
[software/README.md](software/README.md).

## Design

The locked decisions and why they were made are in [docs/handoff.md](docs/handoff.md).
