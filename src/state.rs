//! Shared device state — the single source of truth.
//!
//! The alarm thread owns the authoritative state machine; the network,
//! interaction, and display threads read it and submit *inputs* to it.
//! Local actions (buttons, gesture, web UI) and MQTT commands are both just
//! inputs into the same state — they must never diverge.
//!
//! TODO (you): decide the concrete representation. Likely an `Arc<Mutex<Device>>`
//! plus a command channel (`std::sync::mpsc`) so threads submit events without
//! holding the lock. Keep the alarm thread non-blocking on the network thread.

// TODO: define the preset data model (one model mutated by buttons, web UI, MQTT).
//   - how many presets, repeat days, sound, sunrise on/off, label
//   - this drives both the NVS schema and the HA entity set (open question #3)
// pub struct Preset { ... }

// TODO: define the runtime state machine:
//   Idle -> Revealing -> Idle
//   Armed -> Ringing -> (Snoozed | Dismissed)
//   plus SetupAp (captive portal) and Syncing (RTC invalid, waiting on NTP).
// pub enum AlarmState { ... }

// TODO: define shared settings (proximity sensitivity, brightness curve,
//   reveal duration, MQTT/HA details, WiFi creds) — persisted in NVS.
// pub struct Settings { ... }
