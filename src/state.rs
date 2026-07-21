//! Shared device state + the transport-agnostic command bus.
//!
//! The alarm core (`alarm.rs`) is the single source of truth and the sole writer
//! of [`Shared`]. Every input transport — the BOOT button and the HTTP REST API
//! now, MQTT/HA later — pushes the same [`Command`]s onto the [`CommandBus`]; the
//! core drains them, runs the state machine, and publishes the result for
//! readers (the LED worker, the web UI). Adding a transport = cloning the bus
//! handle; the core never changes.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Runtime phase of the alarm state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Disarmed and quiet.
    Idle,
    /// Armed, watching enabled presets.
    Armed,
    /// Alarm firing.
    Ringing,
    /// Snoozed; re-rings after the snooze interval.
    Snoozed,
}

/// One alarm preset. Repeat-days / sound / sunrise come later.
#[derive(Debug, Clone)]
pub struct Preset {
    pub label: String,
    /// Fire time, seconds since midnight.
    pub secs: u32,
    pub enabled: bool,
}

impl Preset {
    fn new(label: &str, secs: u32, enabled: bool) -> Self {
        Self {
            label: label.into(),
            secs,
            enabled,
        }
    }
}

/// User-configurable settings. Later persisted in NVS + editable from web/HA.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Alarm presets (mutated by all front-ends; never diverge).
    pub presets: Vec<Preset>,
    /// Snooze length, seconds.
    pub snooze_secs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            presets: vec![
                Preset::new("Wake", 7 * 3600, true),        // 07:00, on
                Preset::new("Weekend", 9 * 3600, false),    // 09:00, off
                Preset::new("Nap", 13 * 3600, false),       // 13:00, off
            ],
            snooze_secs: 10, // short, for bench testing
        }
    }
}

/// The single shared state object. The alarm core is the sole writer; readers
/// (LED worker, web UI) take the lock briefly to snapshot it.
#[derive(Debug, Clone)]
pub struct Shared {
    pub phase: Phase,
    pub settings: Settings,
    /// Time of day (seconds since midnight) as the core currently sees it.
    pub now_secs: u32,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            settings: Settings::default(),
            now_secs: 0,
        }
    }
}

pub type SharedState = Arc<Mutex<Shared>>;

pub fn new_shared() -> SharedState {
    Arc::new(Mutex::new(Shared::default()))
}

/// Commands submitted into the alarm core by any input transport.
///
/// `Button*` are raw physical input (the core interprets them by phase); the
/// rest are semantic intents the REST API / MQTT send directly.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    /// Quick BOOT-button press.
    ButtonShort,
    /// Sustained BOOT-button hold.
    ButtonLong,
    Arm,
    Disarm,
    Snooze,
    Dismiss,
    SetPresetEnabled { idx: usize, enabled: bool },
    SetPresetTime { idx: usize, secs: u32 },
}

/// Shared FIFO of pending commands (Send + Sync, so HTTP handlers can push).
pub type CommandBus = Arc<Mutex<VecDeque<Command>>>;

pub fn new_bus() -> CommandBus {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// Push a command onto the bus (used by every input transport).
pub fn submit(bus: &CommandBus, cmd: Command) {
    bus.lock().unwrap().push_back(cmd);
}

/// Format seconds-since-midnight as HH:MM:SS.
pub fn fmt_hms(secs: u32) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// Lowercase phase name for JSON / logs.
pub fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Idle => "idle",
        Phase::Armed => "armed",
        Phase::Ringing => "ringing",
        Phase::Snoozed => "snoozed",
    }
}
