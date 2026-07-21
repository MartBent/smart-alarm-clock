//! Network worker — WiFi + HTTP REST API + captive portal (a worker thread).
//!
//! Boot flow:
//!   * Saved STA creds in NVS -> join that network (station mode).
//!   * No creds (or join fails) -> **open** SoftAP `smart-alarm-clock` +
//!     captive portal: a DNS hijack (see `dns.rs`) points every lookup at us,
//!     so the OS shows a setup sheet; the config page posts the SSID/password
//!     to `/api/wifi`, which saves them to NVS and reboots into STA.
//!
//! The alarm REST API is available in both modes:
//!   curl http://<ip>/api/state
//!   curl -X POST http://<ip>/api/command        -d '{"cmd":"snooze"}'
//!   curl -X POST http://<ip>/api/preset/enabled -d '{"idx":1,"enabled":true}'
//!   curl -X POST http://<ip>/api/preset/time    -d '{"idx":0,"hour":7,"minute":30}'

use core::convert::TryInto;

use embedded_svc::http::{Headers, Method};
use embedded_svc::io::{Read, Write};
use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};

use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::handle::RawHandle; // brings EspNetif::handle() into scope
use esp_idf_svc::http::server::{
    Configuration as HttpConfig, EspHttpConnection, EspHttpServer, Request,
};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

use serde::Deserialize;

use crate::state::{fmt_hms, phase_str, submit, Command, CommandBus, SharedState};

const AP_SSID: &str = "smart-alarm-clock";
const AP_CHANNEL: u8 = 1;
const NVS_NS: &str = "wifi";
const MAX_BODY: usize = 256;
const SERVER_STACK: usize = 10_240;

/// Paths that OS captive-portal detectors probe — serving the setup page for
/// these makes the "Sign in to network" sheet pop up.
const PORTAL_PATHS: &[&str] = &[
    "/",
    "/generate_204",
    "/gen_204",
    "/hotspot-detect.html",
    "/library/test/success.html",
    "/ncsi.txt",
    "/connecttest.txt",
    "/redirect",
    "/canonical.html",
];

static PORTAL_HTML: &str = r#"<!doctype html><html><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Smart Alarm Clock setup</title></head>
<body style="font-family:sans-serif;max-width:20rem;margin:2rem auto">
<h2>Smart Alarm Clock</h2>
<p>Connect the clock to your WiFi:</p>
<form onsubmit="return save(event)">
<p><input id="ssid" placeholder="WiFi name (SSID)" style="width:100%;padding:.4rem"></p>
<p><input id="pass" type="password" placeholder="WiFi password" style="width:100%;padding:.4rem"></p>
<p><button type="submit" style="width:100%;padding:.5rem">Save &amp; connect</button></p>
</form>
<pre id="out"></pre>
<script>
async function save(e){e.preventDefault();
 out.textContent='Saving…';
 try{const r=await fetch('/api/wifi',{method:'POST',headers:{'Content-Type':'application/json'},
   body:JSON.stringify({ssid:ssid.value,password:pass.value})});
 out.textContent=await r.text();}catch(err){out.textContent='error: '+err;}
 return false;}
</script></body></html>"#;

/// Control WebUI served at `/` once connected (STA mode).
static STATUS_HTML: &str = r#"<!doctype html><html><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Smart Alarm Clock</title>
<style>body{font-family:sans-serif;max-width:24rem;margin:1.5rem auto}
button{padding:.5rem .8rem;margin:.15rem}input[type=time]{padding:.2rem}
.row{display:flex;align-items:center;gap:.5rem;margin:.3rem 0}</style></head>
<body>
<h2>Smart Alarm Clock</h2>
<p>Phase <b id="phase">…</b> · <span id="now"></span></p>
<div>
<button onclick="cmd('arm')">Arm</button>
<button onclick="cmd('disarm')">Disarm</button>
<button onclick="cmd('snooze')">Snooze</button>
<button onclick="cmd('dismiss')">Dismiss</button>
</div>
<h3>Presets</h3><div id="presets"></div>
<script>
async function refresh(){try{const s=await(await fetch('/api/state')).json();
 phase.textContent=s.phase;now.textContent=s.now;
 presets.innerHTML=s.presets.map(p=>`<div class=row>
  <label><input type=checkbox ${p.enabled?'checked':''} onchange="tog(${p.idx},this.checked)"> ${p.label}</label>
  <input type=time value="${p.time.slice(0,5)}" onchange="settime(${p.idx},this.value)"></div>`).join('');
 }catch(e){}}
async function post(u,b){await fetch(u,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(b)});}
async function cmd(c){await post('/api/command',{cmd:c});refresh();}
async function tog(idx,enabled){await post('/api/preset/enabled',{idx,enabled});}
async function settime(idx,v){const[h,m]=v.split(':').map(Number);await post('/api/preset/time',{idx,hour:h,minute:m});}
refresh();setInterval(refresh,1000);
</script></body></html>"#;

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
#[derive(Deserialize)]
struct WifiReq {
    ssid: String,
    password: String,
}

pub fn run(modem: Modem, shared: SharedState, bus: CommandBus) {
    log::info!(target: "net", "worker started");

    let sys_loop = EspSystemEventLoop::take().expect("system event loop");
    let nvs_part = EspDefaultNvsPartition::take().expect("nvs partition");

    let creds = load_creds(&nvs_part);
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs_part.clone())).expect("EspWifi::new"),
        sys_loop,
    )
    .expect("BlockingWifi::wrap");

    let ap_mode = match creds {
        Some((ssid, pass)) => match connect_sta(&mut wifi, &ssid, &pass) {
            Ok(()) => false,
            Err(e) => {
                log::warn!(target: "net", "STA join failed ({e:?}); starting setup AP");
                start_ap(&mut wifi);
                true
            }
        },
        None => {
            log::info!(target: "net", "no saved creds -> setup AP + captive portal");
            start_ap(&mut wifi);
            true
        }
    };

    if ap_mode {
        advertise_dns(&wifi);
        std::thread::Builder::new()
            .name("dns".into())
            .stack_size(4096)
            .spawn(crate::dns::run)
            .ok();
    }

    let mut server = EspHttpServer::new(&HttpConfig {
        stack_size: SERVER_STACK,
        max_uri_handlers: 32,
        ..Default::default()
    })
    .expect("http server");
    register(&mut server, shared, bus, nvs_part, ap_mode);
    log::info!(target: "net", "HTTP server listening");

    // Keep `wifi` and `server` alive for the lifetime of the process.
    loop {
        std::thread::sleep(core::time::Duration::from_secs(3600));
    }
}

fn start_ap(wifi: &mut BlockingWifi<EspWifi<'static>>) {
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: AP_SSID.try_into().unwrap(),
        auth_method: AuthMethod::None, // open network
        ssid_hidden: false,
        channel: AP_CHANNEL,
        max_connections: 4,
        ..Default::default()
    }))
    .expect("set AP config");
    wifi.start().expect("wifi start");
    wifi.wait_netif_up().expect("wifi netif up");
    log::info!(target: "net", "open SoftAP up: ssid='{AP_SSID}' -> http://192.168.71.1");
}

fn connect_sta(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> anyhow::Result<()> {
    log::info!(target: "net", "joining WiFi '{ssid}'");
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.try_into().map_err(|_| anyhow::anyhow!("ssid too long"))?,
        password: pass.try_into().map_err(|_| anyhow::anyhow!("password too long"))?,
        auth_method: if pass.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    let ip = wifi.wifi().sta_netif().get_ip_info()?.ip;
    log::info!(target: "net", "STA connected: http://{ip}");
    Ok(())
}

/// Best-effort: make the AP's DHCP server hand out 192.168.71.1 as the DNS
/// server, so clients send their captive-portal lookups to our DNS hijack.
/// Non-fatal — provisioning still works by browsing to 192.168.71.1 directly.
fn advertise_dns(wifi: &BlockingWifi<EspWifi<'static>>) {
    use esp_idf_sys as sys;
    let netif = wifi.wifi().ap_netif().handle();
    if netif.is_null() {
        log::warn!(target: "net", "no AP netif handle; skipping DHCP DNS");
        return;
    }
    // Known-good esp-idf order: stop -> enable DNS offer -> set DNS info -> start.
    unsafe {
        let r_stop = sys::esp_netif_dhcps_stop(netif);
        let mut offer: u8 = 2; // OFFER_DNS
        let r_opt = sys::esp_netif_dhcps_option(
            netif,
            sys::esp_netif_dhcp_option_mode_t_ESP_NETIF_OP_SET,
            sys::esp_netif_dhcp_option_id_t_ESP_NETIF_DOMAIN_NAME_SERVER,
            &mut offer as *mut u8 as *mut core::ffi::c_void,
            1,
        );
        let mut dns: sys::esp_netif_dns_info_t = core::mem::zeroed();
        dns.ip.type_ = 0; // ESP_IPADDR_TYPE_V4
        dns.ip.u_addr.ip4.addr = u32::from_le_bytes([192, 168, 71, 1]);
        let r_dns =
            sys::esp_netif_set_dns_info(netif, sys::esp_netif_dns_type_t_ESP_NETIF_DNS_MAIN, &mut dns);
        let r_start = sys::esp_netif_dhcps_start(netif);
        log::info!(
            target: "net",
            "DHCP-DNS setup rc: stop={r_stop} offer={r_opt} set_dns={r_dns} start={r_start} (0 = ok)"
        );
    }
}

// ---------------------------------------------------------------------------
// NVS credential storage
// ---------------------------------------------------------------------------

fn load_creds(part: &EspDefaultNvsPartition) -> Option<(String, String)> {
    let nvs = EspNvs::new(part.clone(), NVS_NS, false).ok()?; // Err if namespace absent
    let mut sbuf = [0u8; 33];
    let mut pbuf = [0u8; 65];
    let ssid = nvs.get_str("ssid", &mut sbuf).ok().flatten()?;
    if ssid.is_empty() {
        return None;
    }
    let ssid = ssid.to_string();
    let pass = nvs
        .get_str("pass", &mut pbuf)
        .ok()
        .flatten()
        .unwrap_or("")
        .to_string();
    Some((ssid, pass))
}

fn save_creds(part: &EspDefaultNvsPartition, ssid: &str, pass: &str) -> anyhow::Result<()> {
    let mut nvs = EspNvs::new(part.clone(), NVS_NS, true)?;
    nvs.set_str("ssid", ssid)?;
    nvs.set_str("pass", pass)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

fn register(
    server: &mut EspHttpServer<'static>,
    shared: SharedState,
    bus: CommandBus,
    nvs_part: EspDefaultNvsPartition,
    ap_mode: bool,
) {
    if ap_mode {
        // Setup mode: serve the WiFi-setup form for `/` and OS detection URLs
        // so the captive portal triggers.
        for path in PORTAL_PATHS {
            server
                .fn_handler::<anyhow::Error, _>(path, Method::Get, |req| {
                    req.into_ok_response()?.write_all(PORTAL_HTML.as_bytes())?;
                    Ok(())
                })
                .unwrap();
        }
    } else {
        // Connected: `/` is the control WebUI, not the setup form.
        server
            .fn_handler::<anyhow::Error, _>("/", Method::Get, |req| {
                req.into_ok_response()?.write_all(STATUS_HTML.as_bytes())?;
                Ok(())
            })
            .unwrap();
    }

    // GET /api/state
    {
        let shared = shared.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/state", Method::Get, move |req| {
                req.into_ok_response()?.write_all(state_json(&shared).as_bytes())?;
                Ok(())
            })
            .unwrap();
    }

    // GET /api/presets
    {
        let shared = shared.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/presets", Method::Get, move |req| {
                req.into_ok_response()?.write_all(presets_json(&shared).as_bytes())?;
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

    // POST /api/wifi — save STA creds and reboot into station mode.
    {
        server
            .fn_handler::<anyhow::Error, _>("/api/wifi", Method::Post, move |mut req| {
                let Some(buf) = read_body(&mut req)? else {
                    return bad_request(req, "bad body");
                };
                match serde_json::from_slice::<WifiReq>(&buf) {
                    Ok(r) if !r.ssid.is_empty() => {
                        save_creds(&nvs_part, &r.ssid, &r.password)?;
                        log::info!(target: "net", "saved creds for '{}', rebooting", r.ssid);
                        req.into_ok_response()?
                            .write_all(b"Saved. Rebooting to join your WiFi...")?;
                        // Reboot shortly, after the response flushes.
                        std::thread::Builder::new()
                            .stack_size(2048)
                            .spawn(|| {
                                std::thread::sleep(core::time::Duration::from_secs(1));
                                esp_idf_hal::reset::restart();
                            })
                            .ok();
                    }
                    _ => return bad_request(req, "need {ssid, password}"),
                }
                Ok(())
            })
            .unwrap();
    }
}

fn read_body(req: &mut Request<&mut EspHttpConnection<'_>>) -> anyhow::Result<Option<Vec<u8>>> {
    let len = req.content_len().unwrap_or(0) as usize;
    if len == 0 || len > MAX_BODY {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    req.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn bad_request(req: Request<&mut EspHttpConnection<'_>>, msg: &str) -> anyhow::Result<()> {
    req.into_status_response(400)?
        .write_all(format!("{{\"error\":\"{msg}\"}}").as_bytes())?;
    Ok(())
}

fn state_json(shared: &SharedState) -> String {
    let s = shared.lock().unwrap();
    serde_json::json!({
        "phase": phase_str(s.phase),
        "now": fmt_hms(s.now_secs),
        "snooze_secs": s.settings.snooze_secs,
        "presets": preset_values(&s.settings.presets),
    })
    .to_string()
}

fn presets_json(shared: &SharedState) -> String {
    let s = shared.lock().unwrap();
    serde_json::json!(preset_values(&s.settings.presets)).to_string()
}

fn preset_values(presets: &[crate::state::Preset]) -> Vec<serde_json::Value> {
    presets
        .iter()
        .enumerate()
        .map(|(i, p)| {
            serde_json::json!({ "idx": i, "label": p.label, "time": fmt_hms(p.secs), "enabled": p.enabled })
        })
        .collect()
}
