# Firmware architecture

`std` path: `esp-idf-hal` + `esp-idf-svc`, with FreeRTOS underneath exposed to
Rust as `std::thread` / mutexes / channels. The device is **HA-aware but
offline-capable** and is the **source of truth** — local actions (gesture,
buttons, web UI) and MQTT commands are both just inputs into the same on-device
state machine.

## Four threads

Shared state lives behind `Arc<Mutex<…>>` / channels. **The alarm thread never
blocks on the network thread.**

| Thread | File | Role |
| --- | --- | --- |
| Alarm / time | `software/src/alarm.rs` | **Source of truth.** Reads DS3231, evaluates the armed preset, fires the alarm, runs the snooze/dismiss state machine. Independent of the network. |
| Network | `software/src/network.rs` | MQTT (HA auto-discovery, retained state, LWT), self-hosted HTTP web UI, mDNS `.local`, WiFi AP + captive portal in setup mode, SNTP. Reconnects without blocking anything else. |
| Interaction | `software/src/interaction.rs` | VCNL4040 proximity-gesture classification + ambient lux, 3 rear buttons, brightness. |
| Display | `software/src/display.rs` | Renders time / alarm / preset name / armed / dismiss-progress / setup at a fixed refresh; handles fades. Driven OFF when idle. |
| Shared state | `software/src/state.rs` | Preset data model + runtime state machine + settings (NVS-backed). |

## Key behaviours

- **Boot time validity:** if the DS3231 time is valid, use it immediately and let
  SNTP correct drift; if invalid (week+ outage), enter a brief "syncing" state,
  wait for NTP, then set the clock + RTC.
- **Source-of-truth rule:** publish state on every change (retained); HA never
  holds authoritative state; offline → local control unaffected, resync on
  reconnect.
- **Full parity:** everything doable locally is doable from HA, and vice-versa.
- **Presets:** one data model mutated by all three front-ends (buttons, web UI,
  MQTT) so they never diverge; stored in NVS, fired on-device.

For the full locked design, rationale, open questions, and v2 hooks, see
[`handoff.md`](./handoff.md).
