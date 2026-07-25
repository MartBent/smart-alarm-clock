//! Captive-portal DNS (a worker thread, AP mode only).
//!
//! Answers every DNS query with the AP's own IP (192.168.71.1) so a client's
//! OS connectivity check resolves to us and hits our HTTP server — which is how
//! the "Sign in to network" captive-portal sheet gets triggered.

use std::net::UdpSocket;

const AP_IP: [u8; 4] = [192, 168, 71, 1];

pub fn run() {
    let socket = match UdpSocket::bind("0.0.0.0:53") {
        Ok(s) => s,
        Err(e) => {
            log::warn!(target: "dns", "bind :53 failed: {e}");
            return;
        }
    };
    log::info!(target: "dns", "captive DNS up on :53 -> 192.168.71.1");

    let mut buf = [0u8; 512];
    loop {
        let (len, src) = match socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => continue,
        };
        log::info!(target: "dns", "query from {} for '{}'", src, qname(&buf[..len]));
        if let Some(reply) = build_reply(&buf[..len]) {
            let _ = socket.send_to(&reply, src);
        }
    }
}

/// Extract the queried domain name (for logging).
fn qname(q: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut i = 12;
    while let Some(&label) = q.get(i) {
        if label == 0 {
            break;
        }
        let label = label as usize;
        if i + 1 + label > q.len() {
            break;
        }
        parts.push(String::from_utf8_lossy(&q[i + 1..i + 1 + label]).into_owned());
        i += 1 + label;
    }
    parts.join(".")
}

/// Build a minimal reply that points the (first) queried name at `AP_IP`.
fn build_reply(q: &[u8]) -> Option<Vec<u8>> {
    if q.len() < 12 {
        return None;
    }
    if u16::from_be_bytes([q[4], q[5]]) == 0 {
        return None; // no question
    }

    // Walk the QNAME labels to find the end of the question section.
    let mut i = 12;
    loop {
        let label = *q.get(i)? as usize;
        i += 1;
        if label == 0 {
            break;
        }
        i += label;
        if i >= q.len() {
            return None;
        }
    }
    i += 4; // QTYPE + QCLASS
    if i > q.len() {
        return None;
    }

    let mut r = Vec::with_capacity(i + 16);
    r.extend_from_slice(&q[0..2]); // transaction id
    r.extend_from_slice(&[0x81, 0x80]); // flags: response + recursion available
    r.extend_from_slice(&[0x00, 0x01]); // QDCOUNT
    r.extend_from_slice(&[0x00, 0x01]); // ANCOUNT
    r.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    r.extend_from_slice(&[0x00, 0x00]); // ARCOUNT
    r.extend_from_slice(&q[12..i]); // echo the question
    // Answer: pointer to the name at offset 12, type A, class IN, TTL 60, A record.
    r.extend_from_slice(&[0xC0, 0x0C]);
    r.extend_from_slice(&[0x00, 0x01]);
    r.extend_from_slice(&[0x00, 0x01]);
    r.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]);
    r.extend_from_slice(&[0x00, 0x04]);
    r.extend_from_slice(&AP_IP);
    Some(r)
}
