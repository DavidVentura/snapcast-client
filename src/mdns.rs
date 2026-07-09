use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_SRV: u16 = 33;
const CLASS_IN: u16 = 1;
const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

/// Build a one-shot mDNS PTR query for `service` (e.g. `_snapcast._tcp.local`).
/// Sent from an ephemeral port, this elicits a legacy unicast reply (RFC 6762
/// §6.7), so no multicast membership or port-5353 bind is needed to receive it.
pub fn build_query(service: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(service.len() + 18);
    q.extend_from_slice(&[0, 0]); // id
    q.extend_from_slice(&[0, 0]); // flags: standard query
    q.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    q.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // an/ns/ar count
    for label in service.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0); // root label
    q.extend_from_slice(&TYPE_PTR.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    q
}

/// Advance past a DNS name. A name is a run of length-prefixed labels ending in a
/// zero byte or a compression pointer; we only need to *skip* it, so a pointer is
/// just a two-byte terminator we recognize and never follow.
fn skip_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let b = *buf.get(pos)?;
        if b == 0 {
            return Some(pos + 1);
        }
        if b & 0xC0 == 0xC0 {
            return Some(pos + 2);
        }
        pos = pos.checked_add(1 + b as usize)?;
    }
}

/// Pull the server address out of an mDNS response: the IPv4 from the A record and
/// the port from the SRV record. Returns `None` unless both are present.
pub fn parse_response(buf: &[u8]) -> Option<SocketAddr> {
    if buf.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let records = u16::from_be_bytes([buf[6], buf[7]]) as usize
        + u16::from_be_bytes([buf[8], buf[9]]) as usize
        + u16::from_be_bytes([buf[10], buf[11]]) as usize;

    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(buf, pos)?;
        pos = pos.checked_add(4)?; // qtype + qclass
    }

    let mut ip: Option<IpAddr> = None;
    let mut port: Option<u16> = None;
    for _ in 0..records {
        pos = skip_name(buf, pos)?;
        let rtype = u16::from_be_bytes([*buf.get(pos)?, *buf.get(pos + 1)?]);
        let rdlen = u16::from_be_bytes([*buf.get(pos + 8)?, *buf.get(pos + 9)?]) as usize;
        let rdata_start = pos + 10;
        let rdata_end = rdata_start.checked_add(rdlen)?;
        let rdata = buf.get(rdata_start..rdata_end)?;
        match rtype {
            TYPE_A if rdlen == 4 => {
                ip = Some(IpAddr::V4(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3])))
            }
            TYPE_SRV if rdlen >= 6 => port = Some(u16::from_be_bytes([rdata[4], rdata[5]])),
            _ => {}
        }
        pos = rdata_end;
    }
    Some(SocketAddr::new(ip?, port?))
}

/// Discover the first host advertising `service` on the LAN, or `None` on timeout.
pub fn discover(service: &str, timeout: Duration) -> std::io::Result<Option<SocketAddr>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    sock.set_read_timeout(Some(timeout))?;
    sock.send_to(&build_query(service), (MDNS_GROUP, MDNS_PORT))?;

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 1500];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(addr) = parse_response(&buf[..n]) {
                    return Ok(Some(addr));
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                break
            }
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_is_well_formed() {
        let q = build_query("_snapcast._tcp.local");
        // header: qdcount 1, no answers
        assert_eq!(&q[4..6], &1u16.to_be_bytes());
        // name labels are length-prefixed and root-terminated, then PTR/IN
        assert_eq!(q[12], 9); // "_snapcast"
        assert_eq!(&q[13..22], b"_snapcast");
        assert_eq!(&q[q.len() - 4..q.len() - 2], &TYPE_PTR.to_be_bytes());
        assert_eq!(&q[q.len() - 2..], &CLASS_IN.to_be_bytes());
    }

    #[test]
    fn parse_extracts_ip_and_port_through_compression() {
        // header: 2 records (an=2), rest zero
        let mut r = vec![0, 0, 0x84, 0x00, 0, 0, 0, 2, 0, 0, 0, 0];
        // SRV: name is a compression pointer (never followed), port 1704, target
        // also a pointer
        r.extend_from_slice(&[0xC0, 0x0C]); // name -> pointer
        r.extend_from_slice(&TYPE_SRV.to_be_bytes());
        r.extend_from_slice(&CLASS_IN.to_be_bytes());
        r.extend_from_slice(&[0, 0, 0, 120]); // ttl
        r.extend_from_slice(&8u16.to_be_bytes()); // rdlength
        r.extend_from_slice(&[0, 0, 0, 0, 0x06, 0xA8, 0xC0, 0x0C]); // prio,weight,port=1704,target ptr
        // A: pointer name, 192.168.2.50
        r.extend_from_slice(&[0xC0, 0x0C]);
        r.extend_from_slice(&TYPE_A.to_be_bytes());
        r.extend_from_slice(&CLASS_IN.to_be_bytes());
        r.extend_from_slice(&[0, 0, 0, 120]);
        r.extend_from_slice(&4u16.to_be_bytes());
        r.extend_from_slice(&[192, 168, 2, 50]);

        let addr = parse_response(&r).expect("should find address");
        assert_eq!(addr, "192.168.2.50:1704".parse().unwrap());
    }

    #[test]
    fn parse_returns_none_without_both_records() {
        // only an A record, no SRV -> no port -> None
        let mut r = vec![0, 0, 0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 0];
        r.extend_from_slice(&[0xC0, 0x0C]);
        r.extend_from_slice(&TYPE_A.to_be_bytes());
        r.extend_from_slice(&CLASS_IN.to_be_bytes());
        r.extend_from_slice(&[0, 0, 0, 120]);
        r.extend_from_slice(&4u16.to_be_bytes());
        r.extend_from_slice(&[192, 168, 2, 50]);
        assert!(parse_response(&r).is_none());
    }
}
