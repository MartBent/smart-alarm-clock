//! Display thread — renders the warm dot-matrix behind the wood veneer.
//!
//! Panel: diffused warm monochrome LED matrix, APA102/SK9822 (SPI) preferred for
//! true per-pixel brightness (fades + dismiss-progress fill). Driven fully OFF
//! when idle — true dark, "dark & silent until summoned".
//!
//! Renders at a fixed refresh: current time, alarm time, preset name, "ARMED",
//! dismiss-progress fill, "AP MODE"/"SETUP", "syncing". Handles fade in/out.

pub fn run(/* shared state, led matrix handle */) {
    // TODO (you): drive the matrix from shared state; off when idle, fade on reveal.
    loop {
        // TODO: read state, render the active field set, tick fades.
    }
}
