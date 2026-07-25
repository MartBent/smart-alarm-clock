//! Alarm core — the single source of truth (a worker thread).
//!
//! Runs the state machine, drains [`Command`]s from the bus, evaluates presets
//! against the **real local wall-clock time**, and publishes the current
//! [`Phase`] + time for the LED worker + web UI to read.
//!
//! Time comes from the system clock, which SNTP sets a few seconds after WiFi
//! connects (see `net.rs`; timezone is set in `main.rs`). Until it's valid the
//! core sits in [`Phase::Syncing`].
//!
//! There is no explicit arm/disarm: the clock is [`Phase::Armed`] whenever any
//! slot is enabled, else [`Phase::Idle`]. Firing is by edge crossing — when
//! local time crosses an enabled slot's time it rings. Dismiss disables the
//! slot that fired, so alarms are one-shot until re-enabled.

use std::time::{Duration, Instant};

use crate::state::{fmt_hms, Command, CommandBus, Phase, SharedState};

/// Ignore button input for this long after ringing starts (fire-time guard).
const GRACE: Duration = Duration::from_millis(1500);
/// State-machine tick period.
const TICK: Duration = Duration::from_millis(200);
/// Unix time below this (≈ 2023-11) means the clock hasn't been SNTP-set yet.
const VALID_AFTER: i64 = 1_700_000_000;

pub fn run(bus: CommandBus, shared: SharedState) {
    log::info!(target: "alarm", "worker started; waiting for time sync");

    let snooze_secs = shared.lock().unwrap().settings.snooze_secs;

    let mut phase = Phase::Syncing;
    let mut prev_secs: Option<u32> = None;
    let mut snooze_end = Instant::now();
    let mut ring_start = Instant::now();
    // Which slot is currently ringing/snoozed, so Dismiss can disable that one.
    let mut fired_idx: Option<usize> = None;
    // Last phase we published; a change bumps the shared `version`.
    let mut published_phase: Option<Phase> = None;

    loop {
        let now = now_local_secs();
        // Set when a command mutates settings, so we bump `version` this tick.
        let mut settings_changed = false;

        // Apply queued commands. There's no explicit arm/disarm: the clock is
        // "armed" whenever any slot is enabled (derived below). Dismiss turns the
        // fired slot off, so an alarm is naturally one-shot.
        let cmds: Vec<Command> = bus.lock().unwrap().drain(..).collect();
        for cmd in cmds {
            let in_grace = phase == Phase::Ringing && ring_start.elapsed() < GRACE;
            log::debug!(target: "alarm", "command {cmd:?} in phase {phase:?}");
            match cmd {
                Command::Snooze | Command::ButtonShort => {
                    if phase == Phase::Ringing && !in_grace {
                        phase = Phase::Snoozed;
                        snooze_end = Instant::now() + Duration::from_secs(snooze_secs as u64);
                        log::info!(target: "alarm", "-> snoozed {}s", snooze_secs);
                    }
                }
                Command::Dismiss | Command::ButtonLong => {
                    if matches!(phase, Phase::Ringing | Phase::Snoozed) && !in_grace {
                        if let Some(idx) = fired_idx.take() {
                            let mut s = shared.lock().unwrap();
                            if let Some(p) = s.settings.presets.get_mut(idx) {
                                p.enabled = false;
                                settings_changed = true;
                            }
                            log::info!(target: "alarm", "dismissed; disabled alarm {idx}");
                        }
                        // Base phase (armed/idle) is recomputed below from what's
                        // still enabled.
                        phase = Phase::Idle;
                    }
                }
                Command::SetPresetEnabled { idx, enabled } => {
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.enabled = enabled;
                        settings_changed = true;
                        log::info!(target: "alarm", "alarm {idx} enabled={enabled}");
                    }
                }
                Command::SetPresetTime { idx, secs } => {
                    let secs = secs % 86_400;
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.secs = secs;
                        settings_changed = true;
                        log::info!(target: "alarm", "alarm {idx} time={}", fmt_hms(secs));
                    }
                }
            }
        }

        // Time-driven transitions.
        match now {
            None => {
                if phase != Phase::Syncing {
                    phase = Phase::Syncing;
                    log::warn!(target: "alarm", "clock invalid; waiting for sync");
                }
            }
            Some(secs) => {
                let any_enabled = { shared.lock().unwrap().settings.presets.iter().any(|p| p.enabled) };
                match phase {
                    // Quiet phases: derive armed/idle from what's enabled, and ring
                    // when the clock crosses an enabled slot.
                    Phase::Syncing | Phase::Idle | Phase::Armed => {
                        let hit = {
                            let s = shared.lock().unwrap();
                            s.settings.presets.iter().enumerate().find_map(|(i, p)| {
                                (p.enabled && crossed(prev_secs.unwrap_or(secs), secs, p.secs))
                                    .then_some(i)
                            })
                        };
                        if phase == Phase::Syncing {
                            log::info!(target: "alarm", "time synced ({})", fmt_hms(secs));
                        }
                        if let Some(idx) = hit {
                            phase = Phase::Ringing;
                            ring_start = Instant::now();
                            fired_idx = Some(idx);
                            log::warn!(target: "alarm", "*** RINGING *** alarm {idx} ({})", fmt_hms(secs));
                        } else {
                            phase = if any_enabled { Phase::Armed } else { Phase::Idle };
                        }
                    }
                    Phase::Snoozed => {
                        if Instant::now() >= snooze_end {
                            phase = Phase::Ringing;
                            ring_start = Instant::now();
                            log::warn!(target: "alarm", "*** RINGING (after snooze) ***");
                        }
                    }
                    Phase::Ringing => {}
                }
                prev_secs = Some(secs);
            }
        }

        // Publish for readers. Bump `version` only on material change (phase or
        // settings) so push transports don't re-serialize on every `now` tick.
        {
            let mut s = shared.lock().unwrap();
            s.phase = phase;
            s.now_secs = now.unwrap_or(0);
            if published_phase != Some(phase) || settings_changed {
                s.version = s.version.wrapping_add(1);
            }
        }
        published_phase = Some(phase);

        std::thread::sleep(TICK);
    }
}

/// Current local time as seconds-since-midnight, or `None` until SNTP has set
/// the system clock.
fn now_local_secs() -> Option<u32> {
    unsafe {
        let t = esp_idf_sys::time(core::ptr::null_mut());
        if t < VALID_AFTER {
            return None;
        }
        let mut tm: esp_idf_sys::tm = core::mem::zeroed();
        esp_idf_sys::localtime_r(&t, &mut tm);
        Some((tm.tm_hour as u32) * 3600 + (tm.tm_min as u32) * 60 + tm.tm_sec as u32)
    }
}

/// Did `target` fall within the half-open interval `(prev, now]` (mod 24 h)?
fn crossed(prev: u32, now: u32, target: u32) -> bool {
    if now >= prev {
        prev < target && target <= now
    } else {
        target > prev || target <= now
    }
}
