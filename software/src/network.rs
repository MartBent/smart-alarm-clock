//! Network thread — all connectivity. Reconnects without blocking other threads.
//!
//! Responsibilities:
//!   - MQTT client: HA auto-discovery, retained state publish, command subscribe,
//!     LWT availability. Broker creds come from the captive portal / NVS.
//!   - Self-hosted HTTP server (esp-idf-svc): web UI for WiFi creds, presets,
//!     proximity sensitivity, brightness curve, reveal duration, MQTT/HA details.
//!   - mDNS `.local` advertisement for the everyday web UI.
//!   - WiFi AP + captive portal when in setup mode (both rear buttons held 3-5s).
//!   - SNTP for primary timekeeping.
//!
//! Full parity rule: anything doable locally is doable from HA, and vice-versa.
//! This thread only translates between transports and the on-device state — it
//! never holds authoritative state.

pub fn run(/* shared state, command channel */) {
    // TODO (you): bring up WiFi (STA), then HTTP server + mDNS + MQTT + SNTP.
    loop {
        // TODO: service MQTT, handle incoming commands -> submit to shared state.
    }
}
