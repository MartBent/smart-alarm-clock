//! Interaction thread — proximity gesture + rear buttons + brightness.
//!
//! Sensor: single VCNL4040 over I2C (proximity AND ambient lux — no separate LDR).
//!
//! Proximity gesture grammar:
//!   - Idle:    quick reach -> reveal time (fade up, hold ~5s, fade out).
//!   - Ringing: quick reach (sub-threshold) -> snooze;
//!              sustained hold ~2.5s (threshold) -> dismiss, with progressive
//!              fill animation and visual-only completion (no haptic in v1).
//!   - Guards:  fire-time grace window (ignore proximity 1-2s after fire),
//!              pull-away hysteresis.
//!
//! Rear buttons (3): Select (cycle presets), Arm (toggle; long-press disarm all),
//!   Adjust (+/-5 min nudge). Both outer buttons held ~3-5s -> setup AP mode.
//!
//! Night dimming: drive brightness from the VCNL4040 lux reading.

pub fn run(/* shared state, vcnl4040 handle, button gpios */) {
    // TODO (you): classify gestures + debounce buttons -> submit inputs to state.
    loop {
        // TODO: sample proximity + lux, read buttons, emit interaction events.
    }
}
