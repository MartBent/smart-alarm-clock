//! Alarm core — the single source of truth (a worker thread).
//!
//! Runs the Idle → Armed → Ringing → Snoozed state machine, applies [`Command`]s
//! from any transport, and publishes the current [`Phase`] + time into shared
//! state for the LED worker (and later the web UI) to read.
//!
//! The clock is a STAND-IN until slice B wires real SNTP time: arming seeds the
//! time of day to `ARM_LEAD` before the alarm and advances it with
//! `Instant::elapsed()`, so the alarm always fires ~5 s after arming. That makes
//! bench testing quick; slice B will compare real wall-clock time instead.

use std::time::{Duration, Instant};

use crate::state::{fmt_hms, Command, CommandReceiver, Phase, SharedState};

/// How long after arming the alarm fires (stand-in clock lead time).
const ARM_LEAD: Duration = Duration::from_secs(5);
/// Ignore button input for this long after ringing starts (fire-time guard).
const GRACE: Duration = Duration::from_millis(1500);
/// State-machine tick period.
const TICK: Duration = Duration::from_millis(50);

pub fn run(rx: CommandReceiver, shared: SharedState) {
    log::info!(target: "alarm", "worker started");

    let (mut alarm_secs, snooze_secs) = {
        let s = shared.lock().unwrap();
        (s.settings.alarm_secs, s.settings.snooze_secs)
    };

    let mut phase = Phase::Idle;
    let mut fire_at = Instant::now(); // Armed  -> Ringing deadline
    let mut snooze_end = Instant::now(); // Snoozed -> Ringing deadline
    let mut ring_start = Instant::now(); // for the grace window
    let mut clock_base = 0u32; // secs-of-day shown = clock_base + since_arm
    let mut arm_epoch = Instant::now();

    // Arm helper: seed the stand-in clock so the alarm is ARM_LEAD out.
    macro_rules! arm {
        () => {{
            phase = Phase::Armed;
            fire_at = Instant::now() + ARM_LEAD;
            clock_base = alarm_secs.saturating_sub(ARM_LEAD.as_secs() as u32);
            arm_epoch = Instant::now();
            log::info!(target: "alarm", "-> armed (alarm {}, fires in {}s)", fmt_hms(alarm_secs), ARM_LEAD.as_secs());
        }};
    }

    // Auto-arm at boot so there's something to watch.
    arm!();

    loop {
        let now_secs = (clock_base + arm_epoch.elapsed().as_secs() as u32) % 86_400;

        while let Ok(cmd) = rx.try_recv() {
            let in_grace = phase == Phase::Ringing && ring_start.elapsed() < GRACE;
            log::debug!(target: "alarm", "command {cmd:?} in phase {phase:?}");
            match cmd {
                // --- stop everything: dismiss (while ringing/snoozed) or disarm ---
                Command::ButtonLong | Command::Dismiss | Command::Disarm => {
                    let is_button = matches!(cmd, Command::ButtonLong);
                    if !(in_grace && is_button) {
                        phase = Phase::Idle;
                        log::info!(target: "alarm", "-> idle");
                    }
                }
                // --- snooze (only meaningful while ringing) ---
                Command::Snooze | Command::ButtonShort if phase == Phase::Ringing => {
                    if !in_grace {
                        phase = Phase::Snoozed;
                        snooze_end = Instant::now() + Duration::from_secs(snooze_secs as u64);
                        log::info!(target: "alarm", "-> snoozed {}s", snooze_secs);
                    }
                }
                // --- arm: explicit Arm intent, or a short press while idle ---
                Command::Arm => arm!(),
                Command::ButtonShort if phase == Phase::Idle => arm!(),
                // --- reconfigure the alarm time (web/HA later) ---
                Command::SetAlarm { secs } => {
                    alarm_secs = secs % 86_400;
                    shared.lock().unwrap().settings.alarm_secs = alarm_secs;
                    log::info!(target: "alarm", "alarm time set to {}", fmt_hms(alarm_secs));
                    if phase == Phase::Armed {
                        arm!(); // re-seed so the change takes effect now
                    }
                }
                // Snooze when not ringing, short press when armed/snoozed, etc.
                _ => {}
            }
        }

        // Automatic (time-driven) transitions.
        match phase {
            Phase::Armed if Instant::now() >= fire_at => {
                phase = Phase::Ringing;
                ring_start = Instant::now();
                log::warn!(target: "alarm", "*** RINGING ***");
            }
            Phase::Snoozed if Instant::now() >= snooze_end => {
                phase = Phase::Ringing;
                ring_start = Instant::now();
                log::warn!(target: "alarm", "*** RINGING (after snooze) ***");
            }
            _ => {}
        }

        // Publish for readers (LED worker, later the web UI).
        {
            let mut s = shared.lock().unwrap();
            s.phase = phase;
            s.now_secs = now_secs;
        }

        std::thread::sleep(TICK);
    }
}
