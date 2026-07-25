#!/usr/bin/env python3
"""Simulate the Smart Alarm Clock's on-device HTTP API + SSE stream.

Stands in for the real ESP32 firmware (src/net.rs) so the Home Assistant
integration — or a browser / curl — can be exercised with no hardware and no
WiFi provisioning. Pure stdlib; no pip installs.

Contract mirrored from the firmware:
    GET  /api/state              -> {phase, now, snooze_secs, alarms:[...]}
    GET  /api/alarms             -> [{idx,time,enabled}, ...]  (fixed pool of slots)
    POST /api/command            {"cmd":"arm|disarm|snooze|dismiss"}
    POST /api/alarm/enabled      {"idx":int,"enabled":bool}
    POST /api/alarm/time         {"idx":int,"hour":int,"minute":int,"second"?:int}
    POST /api/wifi | /api/wifi/reset | /api/mqtt   -> {"ok":true}  (stubs)
    GET  /api/events   (on the SSE port, default 8081)  -> text/event-stream,
         emits `data: <state json>\n\n` on every material change + heartbeats.

The device serves the API on :80 and SSE on :81. Here they default to 8080 /
8081 so it runs without root; override with --api-port / --sse-port.

Usage:
    python3 tools/sim_device.py                 # 0.0.0.0:8080 (API) + :8081 (SSE)
    python3 tools/sim_device.py --api-port 80 --sse-port 81   # device-accurate (needs root)
    curl localhost:8080/api/state
    curl -N localhost:8081/api/events
"""

from __future__ import annotations

import argparse
import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# --- Shared state (single source of truth, like alarm.rs) --------------------

_PHASES_ACTIVE = {"armed", "ringing", "snoozed"}

# Fixed pool of alarm slots. "Customizable" = enable the ones you want and set
# their times; a disabled slot is effectively "no alarm". Mirrors NUM_PRESETS in
# the firmware (src/state.rs).
NUM_PRESETS = 8


class Device:
    """In-memory model of the alarm clock, with a ticking clock + state machine."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._cond = threading.Condition(self._lock)  # notified on material change
        self.version = 0
        self.phase = "idle"
        self.snooze_secs = 10
        # Fixed pool of slots (secs-since-midnight, enabled) — matches
        # Settings::default(): slot 0 on at 07:00, the rest off. Set times/enable
        # from HA; index is the stable id.
        self.presets = [{"secs": 0, "enabled": False} for _ in range(NUM_PRESETS)]
        self.presets[0] = {"secs": 7 * 3600, "enabled": True}
        self.now_secs = self._wallclock_secs()
        self._snooze_deadline: int | None = None  # now_secs at which snooze re-rings
        self._fired_idx: int | None = None  # slot currently ringing/snoozed
        # Derive the initial phase from what's enabled (no explicit arm).
        self.phase = self._derive_phase_locked()

    # -- helpers --------------------------------------------------------------

    @staticmethod
    def _wallclock_secs() -> int:
        t = time.localtime()
        return t.tm_hour * 3600 + t.tm_min * 60 + t.tm_sec

    @staticmethod
    def _hms(secs: int) -> str:
        secs %= 86400
        return f"{secs // 3600:02d}:{(secs % 3600) // 60:02d}:{secs % 60:02d}"

    def _bump(self) -> None:
        """Record a material change and wake SSE listeners. Call under the lock."""
        self.version += 1
        self._cond.notify_all()

    def snapshot_json(self) -> str:
        with self._lock:
            return self._state_json_locked()

    def _state_json_locked(self) -> str:
        return json.dumps(
            {
                "phase": self.phase,
                "now": self._hms(self.now_secs),
                "snooze_secs": self.snooze_secs,
                "alarms": [
                    {
                        "idx": i,
                        "time": self._hms(p["secs"]),
                        "enabled": p["enabled"],
                    }
                    for i, p in enumerate(self.presets)
                ],
            }
        )

    def presets_json(self) -> str:
        with self._lock:
            return json.dumps(
                [
                    {
                        "idx": i,
                        "time": self._hms(p["secs"]),
                        "enabled": p["enabled"],
                    }
                    for i, p in enumerate(self.presets)
                ]
            )

    # -- command handlers (return True if applied) ----------------------------

    def _derive_phase_locked(self) -> str:
        """Quiet phase from what's enabled: armed if any alarm on, else idle."""
        return "armed" if any(p["enabled"] for p in self.presets) else "idle"

    def command(self, cmd: str) -> bool:
        # No arm/disarm: enabling an alarm arms the clock; dismiss disables the
        # slot that fired (one-shot).
        with self._lock:
            if cmd == "snooze":
                if self.phase == "ringing":
                    self.phase = "snoozed"
                    self._snooze_deadline = self.now_secs + self.snooze_secs
            elif cmd == "dismiss":
                if self.phase in ("ringing", "snoozed"):
                    if self._fired_idx is not None:
                        self.presets[self._fired_idx]["enabled"] = False
                        self._fired_idx = None
                    self._snooze_deadline = None
                    self.phase = self._derive_phase_locked()
            else:
                return False
            self._bump()
            return True

    def set_preset_enabled(self, idx: int, enabled: bool) -> bool:
        with self._lock:
            if not (0 <= idx < len(self.presets)):
                return False
            self.presets[idx]["enabled"] = bool(enabled)
            # Enabling/disabling arms/idles the clock (only while quiet).
            if self.phase in ("idle", "armed"):
                self.phase = self._derive_phase_locked()
            self._bump()
            return True

    def set_preset_time(self, idx: int, hour: int, minute: int, second: int = 0) -> bool:
        with self._lock:
            if not (0 <= idx < len(self.presets)):
                return False
            self.presets[idx]["secs"] = (hour * 3600 + minute * 60 + second) % 86400
            self._bump()
            return True

    # -- background ticker: advance the clock + run the state machine ---------

    def tick_forever(self) -> None:
        while True:
            time.sleep(1)
            with self._lock:
                prev = self.now_secs
                self.now_secs = self._wallclock_secs()
                self._run_state_machine_locked(prev, self.now_secs)

    def _run_state_machine_locked(self, prev: int, now: int) -> None:
        # Snooze expiry -> ring again.
        if self.phase == "snoozed" and self._snooze_deadline is not None:
            if self._crossed(prev, now, self._snooze_deadline % 86400):
                self.phase = "ringing"
                self._snooze_deadline = None
                self._bump()
            return
        # Quiet phases: fire when the wall clock crosses an enabled slot,
        # otherwise keep the derived armed/idle phase in sync with what's enabled.
        if self.phase in ("idle", "armed"):
            for i, p in enumerate(self.presets):
                if p["enabled"] and self._crossed(prev, now, p["secs"]):
                    self.phase = "ringing"
                    self._fired_idx = i
                    self._bump()
                    return
            derived = self._derive_phase_locked()
            if derived != self.phase:
                self.phase = derived
                self._bump()

    @staticmethod
    def _crossed(prev: int, now: int, target: int) -> bool:
        """True if `target` lies in (prev, now], handling midnight wrap."""
        if prev == now:
            return False
        if prev < now:
            return prev < target <= now
        return target > prev or target <= now  # wrapped past midnight


DEV = Device()


# --- HTTP handlers -----------------------------------------------------------


def _read_json(handler: BaseHTTPRequestHandler) -> dict | None:
    length = int(handler.headers.get("Content-Length", 0) or 0)
    if length <= 0:
        return None
    try:
        return json.loads(handler.rfile.read(length))
    except (ValueError, json.JSONDecodeError):
        return None


class ApiHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # noqa: A003 - quieter, tagged log
        print(f"[api] {self.address_string()} {fmt % args}")

    def _send(self, code: int, body: bytes, ctype="application/json") -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def _ok(self) -> None:
        self._send(200, b'{"ok":true}')

    def _bad(self, msg: str) -> None:
        self._send(400, json.dumps({"error": msg}).encode())

    def do_GET(self) -> None:  # noqa: N802
        if self.path.split("?")[0] == "/api/state":
            self._send(200, DEV.snapshot_json().encode())
        elif self.path.split("?")[0] == "/api/alarms":
            self._send(200, DEV.presets_json().encode())
        elif self.path == "/":
            self._send(200, _INDEX_HTML.encode(), ctype="text/html")
        else:
            self._send(404, b'{"error":"not found"}')

    def do_POST(self) -> None:  # noqa: N802
        path = self.path.split("?")[0]
        if path in ("/api/wifi", "/api/wifi/reset", "/api/mqtt"):
            _read_json(self)  # consume, ignore
            self._ok()
            return
        body = _read_json(self)
        if body is None:
            self._bad("invalid json")
            return
        if path == "/api/command":
            if DEV.command(str(body.get("cmd", ""))):
                self._ok()
            else:
                self._bad("unknown cmd")
        elif path == "/api/alarm/enabled":
            if DEV.set_preset_enabled(int(body.get("idx", -1)), bool(body.get("enabled"))):
                self._ok()
            else:
                self._bad("bad idx")
        elif path == "/api/alarm/time":
            if DEV.set_preset_time(
                int(body.get("idx", -1)),
                int(body.get("hour", 0)),
                int(body.get("minute", 0)),
                int(body.get("second", 0)),
            ):
                self._ok()
            else:
                self._bad("bad idx")
        else:
            self._send(404, b'{"error":"not found"}')


class SseHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # noqa: A003
        print(f"[sse] {self.address_string()} {fmt % args}")

    def do_GET(self) -> None:  # noqa: N802
        if self.path.split("?")[0] != "/api/events":
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        try:
            last = None
            # Push initial state immediately.
            self._push(DEV.snapshot_json())
            while True:
                with DEV._cond:
                    # Wait for a material change, or wake to heartbeat.
                    DEV._cond.wait(timeout=15)
                    version = DEV.version
                    state = DEV._state_json_locked()
                if version != last:
                    last = version
                    self._push(state)
                else:
                    self.wfile.write(b": ping\n\n")  # keep-alive comment
                    self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _push(self, state_json: str) -> None:
        self.wfile.write(f"data: {state_json}\n\n".encode())
        self.wfile.flush()


_INDEX_HTML = (
    "<!doctype html><meta charset=utf-8><title>Sim Alarm Clock</title>"
    "<h1>Smart Alarm Clock — simulator</h1>"
    "<p>REST API on this port; SSE on the sse-port at <code>/api/events</code>.</p>"
    "<pre id=s>loading…</pre>"
    "<script>setInterval(async()=>{s.textContent="
    "JSON.stringify(await (await fetch('/api/state')).json(),null,2)},1000)</script>"
)


def main() -> None:
    ap = argparse.ArgumentParser(description="Simulate the ESP alarm-clock API + SSE.")
    ap.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")
    ap.add_argument("--api-port", type=int, default=8080, help="REST port (device uses 80)")
    ap.add_argument("--sse-port", type=int, default=8081, help="SSE port (device uses 81)")
    args = ap.parse_args()

    # Honour the container/host timezone (mount /etc/localtime) so the simulated
    # wall clock matches the times you set from HA, not UTC.
    if hasattr(time, "tzset"):
        time.tzset()
    print(f"[sim] local time is now {time.strftime('%H:%M:%S %Z')}")

    threading.Thread(target=DEV.tick_forever, daemon=True).start()

    api = ThreadingHTTPServer((args.host, args.api_port), ApiHandler)
    sse = ThreadingHTTPServer((args.host, args.sse_port), SseHandler)
    threading.Thread(target=sse.serve_forever, daemon=True).start()

    print(f"[sim] REST API  http://{args.host}:{args.api_port}/api/state")
    print(f"[sim] SSE       http://{args.host}:{args.sse_port}/api/events")
    print("[sim] Ctrl-C to stop.")
    try:
        api.serve_forever()
    except KeyboardInterrupt:
        print("\n[sim] bye")


if __name__ == "__main__":
    main()
