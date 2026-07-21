//! Alarm core — the single source of truth (a worker thread).
//!
//! Runs the Idle → Armed → Ringing → Snoozed state machine, drains [`Command`]s
//! from the bus, evaluates the presets, and publishes the current [`Phase`] +
//! time into shared state for the LED worker + web UI to read.
//!
//! Firing is by **edge crossing**: while Armed, when the clock crosses an
//! enabled preset's time, it rings. This generalizes unchanged to real SNTP
//! time in the WiFi slice. For now the clock is a STAND-IN that free-runs from a
//! seed (~10 s before the earliest enabled preset) so a fire happens soon after
//! boot; you can also POST a preset time of "now + 5 s" to re-test on demand.

use std::time::{Duration, Instant};

use crate::state::{fmt_hms, Command, CommandBus, Phase, SharedState};

/// Ignore button input for this long after ringing starts (fire-time guard).
const GRACE: Duration = Duration::from_millis(1500);
/// State-machine tick period.
const TICK: Duration = Duration::from_millis(50);
/// Stand-in clock: start this many seconds before the earliest enabled preset.
const SEED_LEAD: u32 = 10;

pub fn run(bus: CommandBus, shared: SharedState) {
    log::info!(target: "alarm", "worker started");

    let snooze_secs = shared.lock().unwrap().settings.snooze_secs;

    // Seed the stand-in clock just before the earliest enabled preset.
    let seed = {
        let s = shared.lock().unwrap();
        earliest_enabled(&s.settings.presets)
            .map(|t| t.saturating_sub(SEED_LEAD))
            .unwrap_or(0)
    };
    let clock_base = seed;
    let epoch = Instant::now();

    let mut phase = Phase::Armed; // auto-arm at boot
    let mut prev_secs = clock_base;
    let mut snooze_end = Instant::now();
    let mut ring_start = Instant::now();
    log::info!(target: "alarm", "-> armed (clock seeded at {})", fmt_hms(clock_base));

    loop {
        let now_secs = (clock_base + epoch.elapsed().as_secs() as u32) % 86_400;

        // Apply queued commands.
        let cmds: Vec<Command> = bus.lock().unwrap().drain(..).collect();
        for cmd in cmds {
            let in_grace = phase == Phase::Ringing && ring_start.elapsed() < GRACE;
            log::debug!(target: "alarm", "command {cmd:?} in phase {phase:?}");
            match cmd {
                // Snooze: quick press while ringing, or explicit intent.
                Command::Snooze | Command::ButtonShort if phase == Phase::Ringing => {
                    if !in_grace {
                        phase = Phase::Snoozed;
                        snooze_end = Instant::now() + Duration::from_secs(snooze_secs as u64);
                        log::info!(target: "alarm", "-> snoozed {}s", snooze_secs);
                    }
                }
                // Dismiss: stop ringing/snooze but stay armed.
                Command::Dismiss => {
                    if matches!(phase, Phase::Ringing | Phase::Snoozed) {
                        phase = Phase::Armed;
                        log::info!(target: "alarm", "-> armed (dismissed)");
                    }
                }
                // Long hold: dismiss while ringing/snoozed (unless in grace), else toggle arm.
                Command::ButtonLong => {
                    if matches!(phase, Phase::Ringing | Phase::Snoozed) {
                        if !in_grace {
                            phase = Phase::Armed;
                            log::info!(target: "alarm", "-> armed (dismissed)");
                        }
                    } else {
                        phase = if phase == Phase::Idle { Phase::Armed } else { Phase::Idle };
                        log::info!(target: "alarm", "-> {}", if phase == Phase::Armed { "armed" } else { "idle" });
                    }
                }
                Command::Arm => {
                    phase = Phase::Armed;
                    log::info!(target: "alarm", "-> armed");
                }
                Command::Disarm => {
                    phase = Phase::Idle;
                    log::info!(target: "alarm", "-> idle");
                }
                Command::SetPresetEnabled { idx, enabled } => {
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.enabled = enabled;
                        log::info!(target: "alarm", "preset {idx} ({}) enabled={enabled}", p.label);
                    }
                }
                Command::SetPresetTime { idx, secs } => {
                    let secs = secs % 86_400;
                    let mut s = shared.lock().unwrap();
                    if let Some(p) = s.settings.presets.get_mut(idx) {
                        p.secs = secs;
                        log::info!(target: "alarm", "preset {idx} ({}) time={}", p.label, fmt_hms(secs));
                    }
                }
                // Short press when not ringing, snooze when not ringing, etc.
                _ => {}
            }
        }

        // Automatic transitions.
        match phase {
            // Fire when the clock crosses an enabled preset's time.
            Phase::Armed => {
                let hit = {
                    let s = shared.lock().unwrap();
                    s.settings.presets.iter().any(|p| p.enabled && crossed(prev_secs, now_secs, p.secs))
                };
                if hit {
                    phase = Phase::Ringing;
                    ring_start = Instant::now();
                    log::warn!(target: "alarm", "*** RINGING *** ({})", fmt_hms(now_secs));
                }
            }
            Phase::Snoozed if Instant::now() >= snooze_end => {
                phase = Phase::Ringing;
                ring_start = Instant::now();
                log::warn!(target: "alarm", "*** RINGING (after snooze) ***");
            }
            _ => {}
        }

        // Publish for readers.
        {
            let mut s = shared.lock().unwrap();
            s.phase = phase;
            s.now_secs = now_secs;
        }

        prev_secs = now_secs;
        std::thread::sleep(TICK);
    }
}

/// Earliest enabled preset time (seconds since midnight), if any.
fn earliest_enabled(presets: &[crate::state::Preset]) -> Option<u32> {
    presets.iter().filter(|p| p.enabled).map(|p| p.secs).min()
}

/// Did `target` fall within the half-open interval `(prev, now]` (mod 24 h)?
fn crossed(prev: u32, now: u32, target: u32) -> bool {
    if now >= prev {
        prev < target && target <= now
    } else {
        // Wrapped past midnight.
        target > prev || target <= now
    }
}
