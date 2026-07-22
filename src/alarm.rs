//! Alarm core — the single source of truth (a worker thread).
//!
//! Runs the state machine, drains [`Command`]s from the bus, evaluates presets
//! against the **real local wall-clock time**, and publishes the current
//! [`Phase`] + time for the LED worker + web UI to read.
//!
//! Time comes from the system clock, which SNTP sets a few seconds after WiFi
//! connects (see `net.rs`; timezone is set in `main.rs`). Until it's valid the
//! core sits in [`Phase::Syncing`]. Firing is by edge crossing: while Armed,
//! when local time crosses an enabled preset's time, it rings.

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
    // Whether the user wants the alarm armed. Applied when the clock syncs (so an
    // arm/disarm issued during `Syncing` isn't lost); defaults to armed so a
    // fresh boot arms itself once time is known.
    let mut desired_armed = true;
    // Last phase we published; a change bumps the shared `version`.
    let mut published_phase: Option<Phase> = None;

    loop {
        let now = now_local_secs();
        // Set when a command mutates settings, so we bump `version` this tick.
        let mut settings_changed = false;

        // Apply queued commands.
        let cmds: Vec<Command> = bus.lock().unwrap().drain(..).collect();
        for cmd in cmds {
            let in_grace = phase == Phase::Ringing && ring_start.elapsed() < GRACE;
            log::debug!(target: "alarm", "command {cmd:?} in phase {phase:?}");
            match cmd {
                Command::Snooze | Command::ButtonShort if phase == Phase::Ringing => {
                    if !in_grace {
                        phase = Phase::Snoozed;
                        snooze_end = Instant::now() + Duration::from_secs(snooze_secs as u64);
                        log::info!(target: "alarm", "-> snoozed {}s", snooze_secs);
                    }
                }
                Command::Dismiss => {
                    if matches!(phase, Phase::Ringing | Phase::Snoozed) {
                        desired_armed = true;
                        phase = Phase::Armed;
                        log::info!(target: "alarm", "-> armed (dismissed)");
                    }
                }
                Command::ButtonLong => {
                    if matches!(phase, Phase::Ringing | Phase::Snoozed) {
                        if !in_grace {
                            desired_armed = true;
                            phase = Phase::Armed;
                            log::info!(target: "alarm", "-> armed (dismissed)");
                        }
                    } else if phase == Phase::Idle {
                        desired_armed = true;
                        phase = Phase::Armed;
                        log::info!(target: "alarm", "-> armed");
                    } else if phase == Phase::Armed {
                        desired_armed = false;
                        phase = Phase::Idle;
                        log::info!(target: "alarm", "-> idle");
                    }
                }
                // Arm/Disarm record the user's intent even while `Syncing` (applied
                // on sync) so the request is never silently dropped.
                Command::Arm => {
                    desired_armed = true;
                    if phase != Phase::Syncing {
                        phase = Phase::Armed;
                        log::info!(target: "alarm", "-> armed");
                    } else {
                        log::info!(target: "alarm", "arm requested; will arm on time sync");
                    }
                }
                Command::Disarm => {
                    desired_armed = false;
                    if phase != Phase::Syncing {
                        phase = Phase::Idle;
                        log::info!(target: "alarm", "-> idle");
                    } else {
                        log::info!(target: "alarm", "disarm requested; will stay idle on time sync");
                    }
                }
                Command::SetPresetEnabled { idx, enabled } => {
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.enabled = enabled;
                        settings_changed = true;
                        log::info!(target: "alarm", "preset {idx} ({}) enabled={enabled}", p.label);
                    }
                }
                Command::SetPresetTime { idx, secs } => {
                    let secs = secs % 86_400;
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.secs = secs;
                        settings_changed = true;
                        log::info!(target: "alarm", "preset {idx} ({}) time={}", p.label, fmt_hms(secs));
                    }
                }
                _ => {}
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
                if phase == Phase::Syncing {
                    phase = if desired_armed { Phase::Armed } else { Phase::Idle };
                    log::info!(target: "alarm", "time synced ({}) -> {}",
                        fmt_hms(secs), if desired_armed { "armed" } else { "idle" });
                }
                if phase == Phase::Armed {
                    let hit = {
                        let s = shared.lock().unwrap();
                        s.settings
                            .presets
                            .iter()
                            .any(|p| p.enabled && crossed(prev_secs.unwrap_or(secs), secs, p.secs))
                    };
                    if hit {
                        phase = Phase::Ringing;
                        ring_start = Instant::now();
                        log::warn!(target: "alarm", "*** RINGING *** ({})", fmt_hms(secs));
                    }
                } else if phase == Phase::Snoozed && Instant::now() >= snooze_end {
                    phase = Phase::Ringing;
                    ring_start = Instant::now();
                    log::warn!(target: "alarm", "*** RINGING (after snooze) ***");
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
