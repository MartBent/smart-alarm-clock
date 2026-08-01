//! Buzzer worker — drives the passive buzzer on GPIO4 via LEDC (PWM). A passive
//! buzzer makes no sound on DC; it needs a square wave, which LEDC provides.
//! Reads the shared [`Phase`] each frame (the alarm core is the sole writer) and
//! beeps while `Ringing`, silent otherwise. v1 audio per docs/handoff.md; a real
//! I2S DAC + speaker is a documented future option.
//!
//! Hardware: GPIO4 -> gate of a BS170 (N-ch MOSFET, low-side switch); drain ->
//! buzzer -> 5V. The FET switches the buzzer at the tone frequency off 5V while
//! the GPIO only drives the tiny gate. Tone sits at ~2 kHz (the buzzer's
//! resonance = loudest). Fixed frequency for now; per-note RTTTL comes later
//! (it needs a mutable timer, so restructure the timer/channel borrow then).

use core::time::Duration;

use esp_idf_hal::gpio::Gpio4;
use esp_idf_hal::ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver, CHANNEL0, TIMER0};
use esp_idf_hal::units::FromValueType;

use crate::state::{Phase, SharedState};

const FRAME: Duration = Duration::from_millis(50);
/// Near the passive buzzer's ~2 kHz resonance — loudest for the least current.
const TONE_HZ: u32 = 2000;

pub fn run(timer: TIMER0, channel: CHANNEL0, pin: Gpio4, shared: SharedState) {
    let timer = LedcTimerDriver::new(timer, &TimerConfig::new().frequency(TONE_HZ.Hz()))
        .expect("init LEDC timer");
    let mut buzzer = LedcDriver::new(channel, &timer, pin).expect("init LEDC channel");
    // 50% duty = the square wave that makes the passive buzzer sound; 0 = silent.
    let half = buzzer.get_max_duty() / 2;
    buzzer.set_duty(0).ok(); // start silent

    // Hold the BS170 gate low if the pad is ever undriven. In operation LEDC
    // duty=0 already drives GPIO4 low, so this internal pulldown is belt-and-
    // suspenders (a pre-firmware boot click would need an external pulldown).
    // SAFETY: valid pin + mode constants; only touches the GPIO4 pad pull.
    unsafe {
        esp_idf_sys::gpio_set_pull_mode(
            esp_idf_sys::gpio_num_t_GPIO_NUM_4,
            esp_idf_sys::gpio_pull_mode_t_GPIO_PULLDOWN_ONLY,
        );
    }

    log::info!(target: "buzzer", "worker started");

    let mut frame: u32 = 0;
    loop {
        let phase = shared.lock().unwrap().phase;
        // Ringing: beep ~3 Hz (150 ms on / 150 ms off). Any other phase: silent.
        let on = matches!(phase, Phase::Ringing) && (frame / 3) % 2 == 0;
        buzzer.set_duty(if on { half } else { 0 }).ok();
        frame = frame.wrapping_add(1);
        std::thread::sleep(FRAME);
    }
}
