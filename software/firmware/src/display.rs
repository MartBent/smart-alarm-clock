//! Matrix display worker — draws the current time as HH:MM on the MAX7219 32x8
//! panel (4x cascaded 8x8 modules, SPI). Reads shared state each frame; the
//! alarm core is the sole writer of the clock. Renders a blinking `--:--` while
//! [`Phase::Syncing`] (before SNTP has set the clock), then the real wall-clock
//! time with a ~0.5 Hz blinking colon.
//!
//! Hardware (via the level shifter, per hardware/README.md):
//!   SCLK=GPIO12  DIN=GPIO11  CS=GPIO10 ; matrix VCC=5V, common GND.
//!
//! FC-16 modules are physically rotated, so the flush maps each MAX7219 "digit"
//! register to a ROW (its 8 bits are that chip's 8 columns). If a rebuild looks
//! mirrored / upside-down / module-swapped, flip the orientation knobs below
//! rather than re-deriving the mapping.

use core::time::Duration;

use esp_idf_hal::gpio::{AnyIOPin, Gpio10, Gpio11, Gpio12};
use esp_idf_hal::spi::config::{Config as SpiConfig, DriverConfig};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver, SPI2};
use esp_idf_hal::units::FromValueType;

use crate::state::{Phase, SharedState};

const CHIPS: usize = 4;
const W: usize = CHIPS * 8; // 32 columns
const H: usize = 8;

// --- orientation knobs (flip these if the display looks wrong) ---
const REVERSE_MODULES: bool = true; // which chip is the leftmost 8 columns
const FLIP_X: bool = false; // column bit-order within a chip (mirror left/right)
const FLIP_ROWS: bool = false; // top vs bottom row = digit register 1 (mirror up/down)
const BRIGHTNESS: u8 = 0x02; // 0x00..=0x0F

/// Left edge of each of the four HH:MM digits (columns), and the colon column.
const DIGIT_X: [usize; 4] = [5, 10, 19, 24];
const COLON_X: usize = 15;

const FRAME: Duration = Duration::from_millis(200);

type Fb = [[bool; W]; H];

// 3x5 digit font — 5 rows, low 3 bits each (bit2 = leftmost column).
const FONT: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b001, 0b001, 0b001], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];

/// A 4-module MAX7219 matrix: owns the SPI link and an off-screen framebuffer.
/// Draw into the framebuffer, then [`flush`](Matrix::flush) it to the panel.
pub struct Matrix {
    spi: SpiDeviceDriver<'static, SpiDriver<'static>>,
    fb: Fb,
}

impl Matrix {
    /// Take SPI2 + the three matrix pins and bring the panel up (out of test/
    /// shutdown, raw segments, all rows scanned, at [`BRIGHTNESS`]).
    pub fn new(spi: SPI2, sclk: Gpio12, din: Gpio11, cs: Gpio10) -> Self {
        let mut spi = SpiDeviceDriver::new_single(
            spi,
            sclk,
            din,
            Option::<AnyIOPin>::None, // SDI / MISO — unused
            Some(cs),
            &DriverConfig::new(),
            &SpiConfig::new().baudrate(1u32.MHz().into()),
        )
        .expect("matrix SPI init");

        // Init sequence, broadcast to all 4 chips.
        for (reg, val) in [
            (0x0F, 0x00),      // display-test off
            (0x09, 0x00),      // no BCD decode (raw segments)
            (0x0B, 0x07),      // scan all 8 rows
            (0x0A, BRIGHTNESS), // intensity
            (0x0C, 0x01),      // normal operation (out of shutdown)
        ] {
            spi.write(&broadcast(reg, val)).ok();
        }

        Self { spi, fb: [[false; W]; H] }
    }

    fn clear(&mut self) {
        self.fb = [[false; W]; H];
    }

    /// Draw one 3x5 digit with its left column at `x0` (glyph sits in rows 1..=5).
    fn draw_digit(&mut self, digit: usize, x0: usize) {
        const Y0: usize = 1;
        for (r, bits) in FONT[digit].iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) != 0 {
                    self.fb[Y0 + r][x0 + col] = true;
                }
            }
        }
    }

    /// Compose HH:MM into the framebuffer; `colon` toggles the two colon dots.
    pub fn render_time(&mut self, hh: u32, mm: u32, colon: bool) {
        self.clear();
        self.draw_digit((hh / 10) as usize, DIGIT_X[0]);
        self.draw_digit((hh % 10) as usize, DIGIT_X[1]);
        self.draw_digit((mm / 10) as usize, DIGIT_X[2]);
        self.draw_digit((mm % 10) as usize, DIGIT_X[3]);
        if colon {
            self.fb[2][COLON_X] = true;
            self.fb[4][COLON_X] = true;
        }
    }

    /// Compose a blinking `--:--` placeholder shown until the clock is synced.
    pub fn render_syncing(&mut self, on: bool) {
        self.clear();
        if on {
            for x0 in DIGIT_X {
                for col in 0..3 {
                    self.fb[3][x0 + col] = true; // dash at mid-row
                }
            }
            self.fb[2][COLON_X] = true;
            self.fb[4][COLON_X] = true;
        }
    }

    /// Push the framebuffer to the four chips. Each digit register is a ROW; its
    /// 8 bits are that chip's 8 columns (FC-16 modules are rotated 90°).
    pub fn flush(&mut self) {
        for y in 0..H {
            let dreg = (if FLIP_ROWS { H - y } else { y + 1 }) as u8;
            let mut f = [0u8; CHIPS * 2];
            // First pair clocked in lands in the farthest chip → send pos high→low.
            for (i, pos) in (0..CHIPS).rev().enumerate() {
                let dc = if REVERSE_MODULES { CHIPS - 1 - pos } else { pos };
                let mut b = 0u8;
                for cx in 0..8usize {
                    let x = dc * 8 + cx;
                    if self.fb[y][x] {
                        let bit = if FLIP_X { cx } else { 7 - cx };
                        b |= 1 << bit;
                    }
                }
                f[i * 2] = dreg;
                f[i * 2 + 1] = b;
            }
            self.spi.write(&f).ok();
        }
    }
}

/// A register command `{reg, val}` broadcast to all 4 cascaded chips.
fn broadcast(reg: u8, val: u8) -> [u8; CHIPS * 2] {
    let mut f = [0u8; CHIPS * 2];
    for i in 0..CHIPS {
        f[i * 2] = reg;
        f[i * 2 + 1] = val;
    }
    f
}

/// Matrix display worker: render the shared clock time (or a syncing placeholder)
/// forever. The alarm core owns `now_secs`/`phase`; this only reads them.
pub fn run(spi: SPI2, sclk: Gpio12, din: Gpio11, cs: Gpio10, shared: SharedState) {
    let mut matrix = Matrix::new(spi, sclk, din, cs);
    log::info!(target: "display", "worker started");

    let mut frame: u32 = 0;
    loop {
        let (phase, now) = {
            let s = shared.lock().unwrap();
            (s.phase, s.now_secs)
        };
        if phase == Phase::Syncing {
            // Blink the placeholder ~1.25 Hz (on 400 ms / off 400 ms).
            matrix.render_syncing((frame / 2) % 2 == 0);
        } else {
            let colon = now % 2 == 0; // ~0.5 Hz colon off the wall clock
            matrix.render_time(now / 3600, (now % 3600) / 60, colon);
        }
        matrix.flush();
        frame = frame.wrapping_add(1);
        std::thread::sleep(FRAME);
    }
}
