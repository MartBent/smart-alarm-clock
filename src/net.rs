//! Network worker — SoftAP + HTTP REST API (a worker thread).
//!
//! Brings up a self-hosted WiFi access point, then serves a JSON REST API whose
//! handlers push [`Command`]s onto the bus and read [`Shared`] — the same core
//! the BOOT button drives. Connect a laptop to the AP and:
//!
//!   curl http://192.168.71.1/api/state
//!   curl -X POST http://192.168.71.1/api/command       -d '{"cmd":"snooze"}'
//!   curl -X POST http://192.168.71.1/api/preset/enabled -d '{"idx":1,"enabled":true}'
//!   curl -X POST http://192.168.71.1/api/preset/time    -d '{"idx":0,"hour":7,"minute":30}'
//!
//! TODO (next): POST /api/wifi to store STA creds in NVS + join; an HTML page.

use core::convert::TryInto;

use embedded_svc::http::{Headers, Method};
use embedded_svc::io::{Read, Write};
use embedded_svc::wifi::{AccessPointConfiguration, AuthMethod, Configuration};

use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::server::{
    Configuration as HttpConfig, EspHttpConnection, EspHttpServer, Request,
};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

use serde::Deserialize;

use crate::state::{fmt_hms, phase_str, submit, Command, CommandBus, SharedState};

const AP_SSID: &str = "smart-alarm-clock";
const AP_PASS: &str = "alarmclock"; // WPA2 needs >= 8 chars
const AP_CHANNEL: u8 = 1;
const MAX_BODY: usize = 256;
const SERVER_STACK: usize = 10_240; // JSON parsing needs stack

#[derive(Deserialize)]
struct CmdReq {
    cmd: String,
}

#[derive(Deserialize)]
struct PresetEnabledReq {
    idx: usize,
    enabled: bool,
}

#[derive(Deserialize)]
struct PresetTimeReq {
    idx: usize,
    hour: u8,
    minute: u8,
}

pub fn run(modem: Modem, shared: SharedState, bus: CommandBus) {
    log::info!(target: "net", "worker started");

    let sys_loop = EspSystemEventLoop::take().expect("system event loop");
    let nvs = EspDefaultNvsPartition::take().expect("nvs partition");
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs)).expect("EspWifi::new"),
        sys_loop,
    )
    .expect("BlockingWifi::wrap");

    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: AP_SSID.try_into().unwrap(),
        password: AP_PASS.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ssid_hidden: false,
        channel: AP_CHANNEL,
        ..Default::default()
    }))
    .expect("set AP configuration");
    wifi.start().expect("wifi start");
    wifi.wait_netif_up().expect("wifi netif up");
    log::info!(target: "net", "SoftAP up: ssid='{AP_SSID}' pass='{AP_PASS}' -> http://192.168.71.1");

    let mut server = EspHttpServer::new(&HttpConfig {
        stack_size: SERVER_STACK,
        ..Default::default()
    })
    .expect("http server");

    register(&mut server, shared, bus);
    log::info!(target: "net", "HTTP server listening on http://192.168.71.1");

    // Keep `wifi` and `server` alive for the lifetime of the process.
    loop {
        std::thread::sleep(core::time::Duration::from_secs(3600));
    }
}

fn register(server: &mut EspHttpServer<'static>, shared: SharedState, bus: CommandBus) {
    // GET / — human hint.
    server
        .fn_handler::<anyhow::Error, _>("/", Method::Get, |req| {
            req.into_ok_response()?.write_all(
                b"smart-alarm-clock REST API\n\
                  GET  /api/state\n\
                  GET  /api/presets\n\
                  POST /api/command        {\"cmd\":\"arm|disarm|snooze|dismiss\"}\n\
                  POST /api/preset/enabled {\"idx\":0,\"enabled\":true}\n\
                  POST /api/preset/time    {\"idx\":0,\"hour\":7,\"minute\":30}\n",
            )?;
            Ok(())
        })
        .unwrap();

    // GET /api/state
    {
        let shared = shared.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/state", Method::Get, move |req| {
                let body = state_json(&shared);
                req.into_ok_response()?.write_all(body.as_bytes())?;
                Ok(())
            })
            .unwrap();
    }

    // GET /api/presets
    {
        let shared = shared.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/presets", Method::Get, move |req| {
                let body = presets_json(&shared);
                req.into_ok_response()?.write_all(body.as_bytes())?;
                Ok(())
            })
            .unwrap();
    }

    // POST /api/command
    {
        let bus = bus.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/command", Method::Post, move |mut req| {
                let Some(buf) = read_body(&mut req)? else {
                    return bad_request(req, "bad body");
                };
                match serde_json::from_slice::<CmdReq>(&buf) {
                    Ok(r) => {
                        let cmd = match r.cmd.as_str() {
                            "arm" => Some(Command::Arm),
                            "disarm" => Some(Command::Disarm),
                            "snooze" => Some(Command::Snooze),
                            "dismiss" => Some(Command::Dismiss),
                            _ => None,
                        };
                        match cmd {
                            Some(c) => {
                                submit(&bus, c);
                                req.into_ok_response()?.write_all(b"{\"ok\":true}")?;
                            }
                            None => return bad_request(req, "unknown cmd"),
                        }
                    }
                    Err(_) => return bad_request(req, "invalid json"),
                }
                Ok(())
            })
            .unwrap();
    }

    // POST /api/preset/enabled
    {
        let bus = bus.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/preset/enabled", Method::Post, move |mut req| {
                let Some(buf) = read_body(&mut req)? else {
                    return bad_request(req, "bad body");
                };
                match serde_json::from_slice::<PresetEnabledReq>(&buf) {
                    Ok(r) => {
                        submit(&bus, Command::SetPresetEnabled { idx: r.idx, enabled: r.enabled });
                        req.into_ok_response()?.write_all(b"{\"ok\":true}")?;
                    }
                    Err(_) => return bad_request(req, "invalid json"),
                }
                Ok(())
            })
            .unwrap();
    }

    // POST /api/preset/time
    {
        let bus = bus.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/preset/time", Method::Post, move |mut req| {
                let Some(buf) = read_body(&mut req)? else {
                    return bad_request(req, "bad body");
                };
                match serde_json::from_slice::<PresetTimeReq>(&buf) {
                    Ok(r) => {
                        let secs = (r.hour as u32) * 3600 + (r.minute as u32) * 60;
                        submit(&bus, Command::SetPresetTime { idx: r.idx, secs });
                        req.into_ok_response()?.write_all(b"{\"ok\":true}")?;
                    }
                    Err(_) => return bad_request(req, "invalid json"),
                }
                Ok(())
            })
            .unwrap();
    }
}

/// Read the request body (bounded). Returns `None` if absent/too large.
fn read_body(req: &mut Request<&mut EspHttpConnection<'_>>) -> anyhow::Result<Option<Vec<u8>>> {
    let len = req.content_len().unwrap_or(0) as usize;
    if len == 0 || len > MAX_BODY {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    req.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// Write a 400 with a small JSON error body.
fn bad_request(req: Request<&mut EspHttpConnection<'_>>, msg: &str) -> anyhow::Result<()> {
    req.into_status_response(400)?
        .write_all(format!("{{\"error\":\"{msg}\"}}").as_bytes())?;
    Ok(())
}

fn state_json(shared: &SharedState) -> String {
    let s = shared.lock().unwrap();
    let presets: Vec<serde_json::Value> = s
        .settings
        .presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            serde_json::json!({ "idx": i, "label": p.label, "time": fmt_hms(p.secs), "enabled": p.enabled })
        })
        .collect();
    serde_json::json!({
        "phase": phase_str(s.phase),
        "now": fmt_hms(s.now_secs),
        "snooze_secs": s.settings.snooze_secs,
        "presets": presets,
    })
    .to_string()
}

fn presets_json(shared: &SharedState) -> String {
    let s = shared.lock().unwrap();
    let presets: Vec<serde_json::Value> = s
        .settings
        .presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            serde_json::json!({ "idx": i, "label": p.label, "time": fmt_hms(p.secs), "enabled": p.enabled })
        })
        .collect();
    serde_json::json!(presets).to_string()
}
