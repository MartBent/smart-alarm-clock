//! MQTT + Home Assistant integration (a worker thread, STA mode only).
//!
//! Another transport into the same command bus (like the button + REST API).
//! On connect it publishes HA MQTT-discovery configs so Home Assistant
//! auto-creates the entities, publishes retained state, and subscribes to the
//! command topics. LWT marks the device offline if it drops.
//!
//! Entities (all under one HA device):
//!   * sensor  — Phase
//!   * switch  — Armed            (arm/disarm)
//!   * button  — Snooze, Dismiss
//!   * switch  — one per preset   (enable/disable)
//!
//! Broker is configured via `POST /api/mqtt` (stored in NVS); this worker is
//! spawned by `net.rs` once WiFi is up and a broker is configured.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use esp_idf_svc::mqtt::client::{
    EspMqttClient, EventPayload, LwtConfiguration, MqttClientConfiguration, QoS,
};

use crate::state::{phase_str, submit, Command, CommandBus, Phase, SharedState};

const BASE: &str = "smart-alarm-clock";
const DISC: &str = "homeassistant"; // HA default discovery prefix
const AVAIL: &str = "smart-alarm-clock/availability";

/// Broker connection settings (from NVS / the web UI).
#[derive(Debug, Clone)]
pub struct MqttCfg {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

pub fn run(cfg: MqttCfg, shared: SharedState, bus: CommandBus) {
    let url = format!("mqtt://{}:{}", cfg.host, cfg.port);
    log::info!(target: "mqtt", "worker started; broker {url}");

    let connected = Arc::new(AtomicBool::new(false));

    let config = MqttClientConfiguration {
        client_id: Some(BASE),
        lwt: Some(LwtConfiguration {
            topic: AVAIL,
            payload: b"offline",
            qos: QoS::AtLeastOnce,
            retain: true,
        }),
        username: (!cfg.user.is_empty()).then_some(cfg.user.as_str()),
        password: (!cfg.pass.is_empty()).then_some(cfg.pass.as_str()),
        ..Default::default()
    };

    let cb_bus = bus.clone();
    let cb_conn = connected.clone();
    let mut client = match EspMqttClient::new_cb(&url, &config, move |event| {
        match event.payload() {
            EventPayload::Connected(_) => {
                cb_conn.store(true, Ordering::SeqCst);
                log::info!(target: "mqtt", "connected");
            }
            EventPayload::Disconnected => {
                cb_conn.store(false, Ordering::SeqCst);
                log::warn!(target: "mqtt", "disconnected");
            }
            EventPayload::Received {
                topic: Some(t),
                data,
                ..
            } => route(t, data, &cb_bus),
            _ => {}
        }
    }) {
        Ok(c) => c,
        Err(e) => {
            log::error!(target: "mqtt", "client init failed: {e}");
            return;
        }
    };

    let mut was_connected = false;
    let mut last_version: Option<u64> = None;
    let mut last_phase: Option<Phase> = None;
    let mut last_enabled: Vec<bool> = Vec::new();

    loop {
        let now_conn = connected.load(Ordering::SeqCst);
        if now_conn && !was_connected {
            on_connect(&mut client, &shared);
            // Force a full state republish below.
            last_version = None;
            last_phase = None;
            last_enabled.clear();
        }
        was_connected = now_conn;

        if now_conn {
            // Only snapshot state when the core reports a material change; the
            // per-second `now` tick doesn't bump `version`.
            let version = shared.lock().unwrap().version;
            if last_version != Some(version) {
                let (phase, enabled) = {
                    let s = shared.lock().unwrap();
                    (s.phase, s.settings.presets.iter().map(|p| p.enabled).collect::<Vec<_>>())
                };
                if last_phase != Some(phase) {
                    enq(&mut client, &format!("{BASE}/phase"), phase_str(phase).as_bytes());
                    enq(&mut client, &format!("{BASE}/arm"), arm_str(phase).as_bytes());
                    last_phase = Some(phase);
                }
                if last_enabled != enabled {
                    for (i, on) in enabled.iter().enumerate() {
                        enq(&mut client, &format!("{BASE}/preset/{i}/enable"), on_off(*on));
                    }
                    last_enabled = enabled;
                }
                last_version = Some(version);
            }
        }

        std::thread::sleep(Duration::from_millis(300));
    }
}

/// "ON" while the alarm is active/armed, else "OFF".
fn arm_str(p: Phase) -> &'static str {
    match p {
        Phase::Armed | Phase::Ringing | Phase::Snoozed => "ON",
        Phase::Idle | Phase::Syncing => "OFF",
    }
}

fn on_off(on: bool) -> &'static [u8] {
    if on {
        b"ON"
    } else {
        b"OFF"
    }
}

/// Publish a retained message (state topics are retained so HA has them on start).
fn enq(client: &mut EspMqttClient<'static>, topic: &str, payload: &[u8]) {
    if let Err(e) = client.enqueue(topic, QoS::AtLeastOnce, true, payload) {
        log::warn!(target: "mqtt", "publish {topic} failed: {e}");
    }
}

/// Route an incoming command topic onto the shared command bus.
fn route(topic: &str, data: &[u8], bus: &CommandBus) {
    let payload = core::str::from_utf8(data).unwrap_or("");
    let on = payload.eq_ignore_ascii_case("on") || payload == "1" || payload.eq_ignore_ascii_case("true");
    let Some(rest) = topic.strip_prefix("smart-alarm-clock/") else {
        return;
    };
    if let Some(p) = rest.strip_prefix("preset/") {
        if let Some(idx) = p.strip_suffix("/enable/set").and_then(|s| s.parse::<usize>().ok()) {
            submit(bus, Command::SetPresetEnabled { idx, enabled: on });
        }
        return;
    }
    match rest {
        "arm/set" => submit(bus, if on { Command::Arm } else { Command::Disarm }),
        "snooze/set" => submit(bus, Command::Snooze),
        "dismiss/set" => submit(bus, Command::Dismiss),
        _ => {}
    }
}

/// On (re)connect: subscribe, announce availability, publish HA discovery.
fn on_connect(client: &mut EspMqttClient<'static>, shared: &SharedState) {
    let _ = client.subscribe("smart-alarm-clock/+/set", QoS::AtLeastOnce);
    let _ = client.subscribe("smart-alarm-clock/preset/+/enable/set", QoS::AtLeastOnce);
    enq(client, AVAIL, b"online");

    let dev = serde_json::json!({
        "identifiers": ["smart-alarm-clock"],
        "name": "Smart Alarm Clock",
        "manufacturer": "DIY",
        "model": "v1"
    });

    // Phase sensor.
    disc(client, "sensor", "phase", &serde_json::json!({
        "name": "Phase", "unique_id": "sac_phase",
        "state_topic": format!("{BASE}/phase"),
        "availability_topic": AVAIL, "device": dev,
    }));
    // Armed switch.
    disc(client, "switch", "arm", &serde_json::json!({
        "name": "Armed", "unique_id": "sac_arm",
        "command_topic": format!("{BASE}/arm/set"),
        "state_topic": format!("{BASE}/arm"),
        "payload_on": "ON", "payload_off": "OFF",
        "availability_topic": AVAIL, "device": dev,
    }));
    // Snooze / Dismiss buttons.
    disc(client, "button", "snooze", &serde_json::json!({
        "name": "Snooze", "unique_id": "sac_snooze",
        "command_topic": format!("{BASE}/snooze/set"),
        "availability_topic": AVAIL, "device": dev,
    }));
    disc(client, "button", "dismiss", &serde_json::json!({
        "name": "Dismiss", "unique_id": "sac_dismiss",
        "command_topic": format!("{BASE}/dismiss/set"),
        "availability_topic": AVAIL, "device": dev,
    }));
    // One enable switch per preset.
    let presets = { shared.lock().unwrap().settings.presets.clone() };
    for (i, p) in presets.iter().enumerate() {
        disc(client, "switch", &format!("preset{i}"), &serde_json::json!({
            "name": format!("Alarm: {}", p.label), "unique_id": format!("sac_preset{i}"),
            "command_topic": format!("{BASE}/preset/{i}/enable/set"),
            "state_topic": format!("{BASE}/preset/{i}/enable"),
            "payload_on": "ON", "payload_off": "OFF",
            "availability_topic": AVAIL, "device": dev,
        }));
    }
    log::info!(target: "mqtt", "published HA discovery ({} presets)", presets.len());
}

/// Publish one HA discovery config (retained).
fn disc(client: &mut EspMqttClient<'static>, component: &str, object: &str, payload: &serde_json::Value) {
    let topic = format!("{DISC}/{component}/{BASE}/{object}/config");
    enq(client, &topic, payload.to_string().as_bytes());
}
