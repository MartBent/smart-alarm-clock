//! Buzzer worker — drives the passive buzzer on GPIO4 via LEDC (PWM). A passive
//! buzzer makes no sound on DC; it needs a square wave, which LEDC provides.
//! Reads the shared [`Phase`] each frame (the alarm core is the sole writer) and
//! plays the alarm melody while `Ringing`, silent otherwise. v1 audio per
//! docs/handoff.md; a real I2S DAC + speaker is a documented future option.
//!
//! Hardware: GPIO4 -> gate of a BS170 (N-ch MOSFET, low-side switch); drain ->
//! buzzer -> 5V. The FET switches the buzzer at the tone frequency off 5V while
//! the GPIO only drives the tiny gate. Pitch is set per note by retuning the LEDC
//! timer (the passive buzzer's ~2 kHz resonance is loudest, but any tone sounds).

use core::time::Duration;

use esp_idf_hal::gpio::Gpio4;
use esp_idf_hal::ledc::{
    config::{Resolution, TimerConfig},
    LedcDriver, LedcTimerDriver, CHANNEL0, TIMER0,
};
use esp_idf_hal::units::FromValueType;

use crate::state::{Phase, SharedState};

/// Poll interval when idle (how fast the worker reacts to entering `Ringing`).
const FRAME: Duration = Duration::from_millis(50);

/// Alarm melody: `(frequency Hz, duration ms)`, `0` = rest. Retuning the timer
/// per note plays a tune; this repeats while the alarm rings. Kept short and
/// rising so it reads as an alarm, not background music.
const MELODY: &[(u32, u64)] = &[
    (880, 180),
    (0, 60),
    (1109, 180),
    (0, 60),
    (1319, 180),
    (0, 60),
    (1319, 300),
    (0, 240),
];

pub fn run(timer: TIMER0, channel: CHANNEL0, pin: Gpio4, shared: SharedState) {
    // 10-bit resolution keeps duty ~50% across the melody's pitch range.
    let timer = LedcTimerDriver::new(
        timer,
        &TimerConfig::new()
            .resolution(Resolution::Bits10)
            .frequency(2u32.kHz().into()),
    )
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

    loop {
        let ringing = matches!(shared.lock().unwrap().phase, Phase::Ringing);
        if ringing {
            play_melody(&mut buzzer, half, &shared);
        } else {
            buzzer.set_duty(0).ok();
            std::thread::sleep(FRAME);
        }
    }
}

/// Play the melody once, aborting between notes if the alarm stops ringing so a
/// snooze/dismiss silences the buzzer promptly. Leaves the buzzer silent on exit.
fn play_melody(buzzer: &mut LedcDriver<'static>, half: u32, shared: &SharedState) {
    for &(freq, ms) in MELODY {
        if !matches!(shared.lock().unwrap().phase, Phase::Ringing) {
            break;
        }
        if freq == 0 {
            buzzer.set_duty(0).ok();
        } else {
            set_tone(freq);
            buzzer.set_duty(half).ok();
        }
        std::thread::sleep(Duration::from_millis(ms));
    }
    buzzer.set_duty(0).ok();
}

/// Retune the LEDC low-speed timer 0 to `freq` Hz. The [`LedcDriver`] borrows the
/// timer immutably, so pitch changes go through the IDF API directly.
/// SAFETY: valid mode/timer constants; only changes timer 0's output frequency.
fn set_tone(freq: u32) {
    unsafe {
        esp_idf_sys::ledc_set_freq(
            esp_idf_sys::ledc_mode_t_LEDC_LOW_SPEED_MODE,
            esp_idf_sys::ledc_timer_t_LEDC_TIMER_0,
            freq,
        );
    }
}
