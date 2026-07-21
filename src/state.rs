//! Shared device state + the transport-agnostic command bus.
//!
//! The alarm core (`alarm.rs`) is the single source of truth. Every input
//! transport — the BOOT button now, the web REST API and MQTT/HA later —
//! submits the same [`Command`]s over an `mpsc` channel; the core applies them,
//! runs the state machine, and publishes the resulting [`Phase`] into [`Shared`],
//! which the LED (display) worker renders. Adding a transport = cloning the
//! command `Sender`; the core never changes.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Runtime phase of the alarm state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Disarmed and quiet.
    Idle,
    /// Armed, waiting for the alarm time.
    Armed,
    /// Alarm firing.
    Ringing,
    /// Snoozed; re-rings after the snooze interval.
    Snoozed,
}

/// User-configurable settings. Later persisted in NVS + editable from web/HA.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Alarm time, seconds since midnight.
    pub alarm_secs: u32,
    /// Snooze length, seconds.
    pub snooze_secs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            alarm_secs: 7 * 3600 + 30 * 60, // 07:30:00
            snooze_secs: 10,                // short, for bench testing
        }
    }
}

/// The single shared state object — readers (LED worker, later the web UI) see
/// the current phase + time here; the alarm core is the sole writer.
#[derive(Debug, Clone, Copy)]
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
/// The `Button*` variants are raw physical input (the core interprets them by
/// phase). The rest are semantic intents that the web REST API / MQTT will send
/// directly.
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
    SetAlarm { secs: u32 },
}

pub type CommandSender = Sender<Command>;
pub type CommandReceiver = Receiver<Command>;

/// Format seconds-since-midnight as HH:MM:SS.
pub fn fmt_hms(secs: u32) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}
