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
use esp_idf_svc::sntp::EspSntp;
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

/// Shared stylesheet: dark, warm-ember bedside aesthetic ("dark & silent until
/// summoned"), system fonts only (no external assets — works with no internet).
const STYLE: &str = r##"<style>
:root{--panel:#171009;--line:#2b2015;--text:#ecdcc4;--muted:#9c8a72;--glow:#ffb35c;--ember:#e8792b}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;background:radial-gradient(130% 80% at 50% -15%,#20160b 0%,#0e0b08 62%);color:var(--text);font:16px/1.5 -apple-system,system-ui,"Segoe UI",Roboto,sans-serif;display:flex;align-items:center;justify-content:center;padding:1.5rem}
.card{width:100%;max-width:22rem;background:var(--panel);border:1px solid var(--line);border-radius:18px;padding:1.6rem 1.4rem;box-shadow:0 24px 60px rgba(0,0,0,.55)}
.brand{font-size:.72rem;letter-spacing:.24em;text-transform:uppercase;color:var(--muted);font-weight:600;margin:0 0 1.1rem}
.phase{font-size:.72rem;letter-spacing:.22em;text-transform:uppercase;color:var(--ember);margin-bottom:.15rem}
.clock{font:600 3rem/1 ui-monospace,SFMono-Regular,Menlo,monospace;font-variant-numeric:tabular-nums;letter-spacing:.02em;color:var(--glow);text-shadow:0 0 20px rgba(255,179,92,.45)}
.hint{color:var(--muted);margin:.1rem 0 1.2rem}
.btns{display:grid;grid-template-columns:1fr 1fr;gap:.5rem;margin:1.3rem 0 .2rem}
button{font:inherit;font-weight:600;color:var(--text);background:#1f160c;border:1px solid var(--line);border-radius:12px;padding:.7rem;cursor:pointer;transition:border-color .15s,color .15s}
button:hover{border-color:var(--ember);color:var(--glow)}
button:active{transform:translateY(1px)}
button:focus-visible{outline:2px solid var(--glow);outline-offset:2px}
.primary{background:linear-gradient(180deg,var(--ember),#c85f1c);border-color:transparent;color:#1a0f05}
.primary:hover{color:#1a0f05;filter:brightness(1.08)}
label{display:block;font-size:.8rem;color:var(--muted);margin:1rem 0 .35rem}
input[type=text],input[type=password]{width:100%;background:#120c06;border:1px solid var(--line);border-radius:10px;color:var(--text);padding:.7rem;font:inherit}
input[type=text]:focus,input[type=password]:focus{outline:none;border-color:var(--glow);box-shadow:0 0 0 3px rgba(255,179,92,.15)}
h2{font-size:.72rem;letter-spacing:.22em;text-transform:uppercase;color:var(--muted);margin:1.5rem 0 .3rem}
.preset{display:flex;align-items:center;gap:.7rem;padding:.65rem 0;border-top:1px solid var(--line)}
.preset .name{flex:1}
input[type=time]{background:#120c06;border:1px solid var(--line);border-radius:8px;color:var(--text);padding:.4rem .5rem;font:inherit;font-variant-numeric:tabular-nums}
input[type=time]:focus{outline:none;border-color:var(--glow)}
.sw{position:relative;width:42px;height:24px;flex:none;margin:0}
.sw input{position:absolute;opacity:0;width:100%;height:100%;margin:0;cursor:pointer;z-index:1}
.sw span{position:absolute;inset:0;background:#241a0e;border:1px solid var(--line);border-radius:99px;transition:.15s}
.sw span:before{content:"";position:absolute;width:18px;height:18px;left:2px;top:2px;background:var(--muted);border-radius:50%;transition:.15s}
.sw input:checked+span{background:rgba(232,121,43,.3);border-color:var(--ember)}
.sw input:checked+span:before{transform:translateX(18px);background:var(--glow)}
#out{color:var(--muted);font-size:.86rem;margin-top:.9rem;min-height:1.2em}
@media(prefers-reduced-motion){*{transition:none!important}}
</style>"##;

static PORTAL_HTML: &str = r##"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Smart Alarm Clock setup</title>__STYLE__</head>
<body><div class="card">
<div class="brand">Smart Alarm Clock</div>
<div class="phase">Setup</div>
<p class="hint">Connect the clock to your Wi-Fi network.</p>
<form onsubmit="return save(event)">
<label for="ssid">Network name</label>
<input id="ssid" type="text" placeholder="Wi-Fi SSID" autocomplete="off" autocapitalize="off">
<label for="pass">Password</label>
<input id="pass" type="password" placeholder="Wi-Fi password">
<div class="btns" style="grid-template-columns:1fr;margin-top:1.3rem">
<button class="primary" type="submit">Save &amp; connect</button></div>
</form>
<div id="out"></div>
</div>
<script>
async function save(e){e.preventDefault();out.textContent='Saving…';
 try{const r=await fetch('/api/wifi',{method:'POST',headers:{'Content-Type':'application/json'},
  body:JSON.stringify({ssid:ssid.value,password:pass.value})});
 out.textContent=await r.text();}catch(err){out.textContent='Error: '+err;}
 return false;}
</script></body></html>"##;

/// Control WebUI served at `/` once connected (STA mode).
static STATUS_HTML: &str = r##"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Smart Alarm Clock</title>__STYLE__</head>
<body><div class="card">
<div class="brand">Smart Alarm Clock</div>
<div class="phase" id="phase">&mdash;</div>
<div class="clock" id="now">--:--:--</div>
<div class="btns">
<button onclick="cmd('arm')">Arm</button>
<button onclick="cmd('disarm')">Disarm</button>
<button onclick="cmd('snooze')">Snooze</button>
<button class="primary" onclick="cmd('dismiss')">Dismiss</button>
</div>
<h2>Alarms</h2><div id="presets"></div>
<h2>Home Assistant (MQTT)</h2>
<label for="mh">Broker host</label>
<input id="mh" type="text" placeholder="e.g. 192.168.0.10" autocomplete="off" autocapitalize="off">
<label for="mp">Port</label>
<input id="mp" type="text" value="1883">
<label for="mu">Username <span style="text-transform:none;letter-spacing:0">(optional)</span></label>
<input id="mu" type="text" autocomplete="off" autocapitalize="off">
<label for="mpw">Password <span style="text-transform:none;letter-spacing:0">(optional)</span></label>
<input id="mpw" type="password">
<div class="btns" style="grid-template-columns:1fr;margin-top:.8rem">
<button class="primary" onclick="savemqtt()">Save broker &amp; reboot</button></div>
<div id="mout"></div>
</div>
<script>
let lastPresets="";
async function refresh(){try{const s=await(await fetch('/api/state')).json();
 phase.textContent=s.phase;now.textContent=s.now;
 // Rebuild preset rows only when they change AND you aren't editing one,
 // so the 1s poll never steals focus or clobbers typing.
 const key=JSON.stringify(s.presets);
 if(key!==lastPresets && !presets.contains(document.activeElement)){lastPresets=key;
  presets.innerHTML=s.presets.map(p=>`<div class="preset">
   <label class="sw"><input type="checkbox" ${p.enabled?'checked':''} onchange="tog(${p.idx},this.checked)"><span></span></label>
   <span class="name">${p.label}</span>
   <input type="time" value="${p.time.slice(0,5)}" onchange="settime(${p.idx},this.value)"></div>`).join('');
 }}catch(e){}}
async function post(u,b){await fetch(u,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(b)});}
async function cmd(c){await post('/api/command',{cmd:c});refresh();}
async function tog(idx,enabled){await post('/api/preset/enabled',{idx,enabled});}
async function settime(idx,v){const[h,m]=v.split(':').map(Number);await post('/api/preset/time',{idx,hour:h,minute:m});}
async function savemqtt(){mout.textContent='Saving…';
 try{const r=await fetch('/api/mqtt',{method:'POST',headers:{'Content-Type':'application/json'},
  body:JSON.stringify({host:mh.value,port:Number(mp.value)||1883,username:mu.value,password:mpw.value})});
 mout.textContent=await r.text();}catch(e){mout.textContent='Error: '+e;}}
refresh();setInterval(refresh,1000);
</script></body></html>"##;

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
#[derive(Deserialize)]
struct MqttReq {
    host: String,
    #[serde(default = "default_mqtt_port")]
    port: u16,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}
fn default_mqtt_port() -> u16 {
    1883
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

    // MQTT + Home Assistant (STA mode, if a broker is configured in NVS).
    if !ap_mode {
        match load_mqtt(&nvs_part) {
            Some(mcfg) => {
                let (sh, bs) = (shared.clone(), bus.clone());
                std::thread::Builder::new()
                    .name("mqtt".into())
                    .stack_size(8 * 1024)
                    .spawn(move || crate::mqtt::run(mcfg, sh, bs))
                    .ok();
            }
            None => log::info!(target: "net", "no MQTT broker configured (POST /api/mqtt to set one)"),
        }
    }

    let mut server = EspHttpServer::new(&HttpConfig {
        stack_size: SERVER_STACK,
        max_uri_handlers: 32,
        ..Default::default()
    })
    .expect("http server");
    register(&mut server, shared, bus, nvs_part, ap_mode);
    log::info!(target: "net", "HTTP server listening");

    // Real time via SNTP (STA only — the AP has no upstream). The alarm core
    // reads the system clock once this syncs.
    let _sntp = if ap_mode {
        None
    } else {
        match EspSntp::new_default() {
            Ok(s) => {
                log::info!(target: "net", "SNTP started (syncing time via pool.ntp.org)");
                Some(s)
            }
            Err(e) => {
                log::warn!(target: "net", "SNTP start failed: {e}");
                None
            }
        }
    };

    // Keep `wifi`, `server`, and `_sntp` alive for the lifetime of the process.
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

fn load_mqtt(part: &EspDefaultNvsPartition) -> Option<crate::mqtt::MqttCfg> {
    let nvs = EspNvs::new(part.clone(), "mqtt", false).ok()?;
    let mut hb = [0u8; 64];
    let host = nvs.get_str("host", &mut hb).ok().flatten()?;
    if host.is_empty() {
        return None;
    }
    let host = host.to_string();
    let mut pb = [0u8; 8];
    let port = nvs
        .get_str("port", &mut pb)
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1883);
    let mut ub = [0u8; 64];
    let user = nvs.get_str("user", &mut ub).ok().flatten().unwrap_or("").to_string();
    let mut xb = [0u8; 64];
    let pass = nvs.get_str("pass", &mut xb).ok().flatten().unwrap_or("").to_string();
    Some(crate::mqtt::MqttCfg { host, port, user, pass })
}

fn save_mqtt(
    part: &EspDefaultNvsPartition,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
) -> anyhow::Result<()> {
    let mut nvs = EspNvs::new(part.clone(), "mqtt", true)?;
    nvs.set_str("host", host)?;
    nvs.set_str("port", &port.to_string())?;
    nvs.set_str("user", user)?;
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
                    req.into_ok_response()?
                        .write_all(PORTAL_HTML.replace("__STYLE__", STYLE).as_bytes())?;
                    Ok(())
                })
                .unwrap();
        }
    } else {
        // Connected: `/` is the control WebUI, not the setup form.
        server
            .fn_handler::<anyhow::Error, _>("/", Method::Get, |req| {
                req.into_ok_response()?
                    .write_all(STATUS_HTML.replace("__STYLE__", STYLE).as_bytes())?;
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
        let nvs_part = nvs_part.clone();
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
                        reboot_soon();
                    }
                    _ => return bad_request(req, "need {ssid, password}"),
                }
                Ok(())
            })
            .unwrap();
    }

    // POST /api/wifi/reset — forget WiFi creds and reboot into setup AP.
    {
        let nvs_part = nvs_part.clone();
        server
            .fn_handler::<anyhow::Error, _>("/api/wifi/reset", Method::Post, move |req| {
                save_creds(&nvs_part, "", "")?; // empty ssid -> setup AP on next boot
                log::info!(target: "net", "WiFi creds cleared; rebooting to setup AP");
                req.into_ok_response()?
                    .write_all(b"WiFi settings cleared. Rebooting to setup mode...")?;
                reboot_soon();
                Ok(())
            })
            .unwrap();
    }

    // POST /api/mqtt — save broker settings and reboot to connect.
    {
        server
            .fn_handler::<anyhow::Error, _>("/api/mqtt", Method::Post, move |mut req| {
                let Some(buf) = read_body(&mut req)? else {
                    return bad_request(req, "bad body");
                };
                match serde_json::from_slice::<MqttReq>(&buf) {
                    Ok(r) if !r.host.is_empty() => {
                        save_mqtt(&nvs_part, &r.host, r.port, &r.username, &r.password)?;
                        log::info!(target: "net", "saved MQTT broker {}:{}, rebooting", r.host, r.port);
                        req.into_ok_response()?
                            .write_all(b"Saved. Rebooting to connect to MQTT...")?;
                        reboot_soon();
                    }
                    _ => return bad_request(req, "need {host, port, username, password}"),
                }
                Ok(())
            })
            .unwrap();
    }
}

/// Reboot after a short delay so the HTTP response can flush first.
fn reboot_soon() {
    std::thread::Builder::new()
        .stack_size(2048)
        .spawn(|| {
            std::thread::sleep(core::time::Duration::from_secs(1));
            esp_idf_hal::reset::restart();
        })
        .ok();
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
