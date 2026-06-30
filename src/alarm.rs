//! Alarm / time thread — the SOURCE OF TRUTH.
//!
//! Independent of the network so the device fires alarms even if WiFi/HA is down.
//! Never blocks on the network thread.
//!
//! Responsibilities:
//!   - Boot validity check: if DS3231 time is valid, use it immediately and let
//!     SNTP correct drift; if invalid (week+ outage), enter "Syncing", wait for
//!     NTP, then set the clock + RTC.
//!   - Read DS3231, evaluate the armed preset, fire the alarm at the right time.
//!   - Run the snooze / dismiss state machine (with the fire-time grace window).
//!   - Drive the piezo (RTTTL over one LEDC/PWM GPIO) for the wake sound.
//!   - Publish every state change (so HA stays in sync) without owning HA state.

pub fn run(/* shared state, rtc handle, piezo handle */) {
    // TODO (you): write the core firing + snooze/dismiss logic yourself.
    loop {
        // TODO: read RTC, compare against armed preset, transition the state machine.
    }
}
