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
    /// Waiting for the clock to sync (SNTP) before any alarm can be trusted.
    Syncing,
    /// No alarm enabled; quiet.
    Idle,
    /// At least one alarm enabled; watching for its time.
    Armed,
    /// Alarm firing.
    Ringing,
    /// Snoozed; re-rings after the snooze interval.
    Snoozed,
}

/// Number of alarm slots in the fixed pool. "Customizable" = enable the ones
/// you want and set their times; a disabled slot is effectively "no alarm".
pub const NUM_PRESETS: usize = 8;

/// One alarm slot. Repeat-days / sound / sunrise come later. The slot's index in
/// [`Settings::presets`] is its stable id (front-ends address it by index).
#[derive(Debug, Clone)]
pub struct Preset {
    /// Fire time, seconds since midnight.
    pub secs: u32,
    pub enabled: bool,
}

impl Preset {
    fn new(secs: u32, enabled: bool) -> Self {
        Self { secs, enabled }
    }
}

/// User-configurable settings. Later persisted in NVS + editable from web/HA.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Fixed pool of alarm slots (mutated by all front-ends; never diverge).
    pub presets: Vec<Preset>,
    /// Snooze length, seconds.
    pub snooze_secs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        // A fixed pool of NUM_PRESETS slots: slot 0 on at 07:00, the rest off.
        let mut presets = vec![Preset::new(0, false); NUM_PRESETS];
        presets[0] = Preset::new(7 * 3600, true);
        Self {
            presets,
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
    /// Master switch for all light-emitting components (matrix + status LED). When
    /// `false` the clock is dark ("dark & silent until summoned"); a firing alarm
    /// still lights up regardless. Toggled from the API and capacitive touch.
    pub display_on: bool,
    /// Time of day (seconds since midnight) as the core currently sees it.
    pub now_secs: u32,
    /// Bumped by the alarm core on every *material* change (phase or settings),
    /// but NOT on the per-second `now_secs` tick. Push transports (SSE, MQTT)
    /// watch this to avoid re-serializing state twice a second while idle.
    pub version: u64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            settings: Settings::default(),
            display_on: true,
            now_secs: 0,
            version: 0,
        }
    }
}

pub type SharedState = Arc<Mutex<Shared>>;

pub fn new_shared() -> SharedState {
    Arc::new(Mutex::new(Shared::default()))
}

/// Returns `true` (and records the new value in `last`) when the shared
/// [`Shared::version`] has advanced since `last` was last recorded. Push
/// transports (SSE, MQTT) call this to gate work so they only re-serialize on a
/// material change, not on every poll. The cheap version read is one brief lock;
/// callers snapshot the fuller state under their own lock only when this is true.
pub fn version_advanced(shared: &SharedState, last: &mut Option<u64>) -> bool {
    let version = shared.lock().unwrap().version;
    if *last != Some(version) {
        *last = Some(version);
        true
    } else {
        false
    }
}

/// Commands submitted into the alarm core by any input transport.
///
/// `Button*` are raw physical input (the core interprets them by phase); the
/// rest are semantic intents the REST API / MQTT send directly.
#[derive(Debug, Clone, Copy)]
pub enum Command {
    /// Quick BOOT-button press (snooze while ringing).
    ButtonShort,
    /// Sustained BOOT-button hold (dismiss while ringing/snoozed).
    ButtonLong,
    Snooze,
    Dismiss,
    SetPresetEnabled { idx: usize, enabled: bool },
    SetPresetTime { idx: usize, secs: u32 },
    /// Turn all light-emitting components on/off (API sets an explicit state).
    SetDisplay { on: bool },
    /// Flip the display on/off (capacitive touch).
    ToggleDisplay,
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
        Phase::Syncing => "syncing",
        Phase::Idle => "idle",
        Phase::Armed => "armed",
        Phase::Ringing => "ringing",
        Phase::Snoozed => "snoozed",
    }
}
