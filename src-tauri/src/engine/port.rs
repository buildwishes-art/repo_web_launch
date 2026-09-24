//! Port checker & Dynamic Port Allocation.
//!
//! Algoritma `allocate`:
//! 1. Port bawaan repo (dari .env / package.json / config / default framework) jika bebas.
//! 2. Jika dipakai → cari di sekitar port asli (3000 → 3001..3050) agar tetap mudah dikenali.
//! 3. Jika masih gagal → pindai rentang aman 20000–29999 mulai offset acak.
//! 4. Fallback terakhir → port ephemeral dari OS (bind ke :0).
//!
//! Port dianggap "terpakai" jika: di-reserve oleh repo aktif lain di RepoLaunch, ATAU ada yang
//! menerima koneksi, ATAU tidak bisa di-bind di 127.0.0.1 / 0.0.0.0 / [::1].
//! Catatan: tetap ada jendela TOCTOU kecil antara cek dan bind oleh proses anak.

use std::{
    collections::HashSet,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

/// Port yang diblokir browser (ERR_UNSAFE_PORT) — link localhost ke sana tidak akan bisa dibuka.
const BROWSER_BLOCKED: &[u16] = &[
    1719, 1720, 1723, 2049, 3659, 4045, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668, 6669,
    6697, 10080,
];
const NEARBY_SPAN: u16 = 50;
const SAFE_RANGE_START: u16 = 20_000;
const SAFE_RANGE_LEN: u16 = 10_000;

#[derive(Debug, Clone, Copy)]
pub struct Allocation {
    pub port: u16,
    /// `true` jika port bawaan terpakai dan repo dialihkan ke port lain.
    pub relocated: bool,
}

pub fn is_port_free(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    if is_listening(port) {
        return false;
    }
    // Setiap bind dilepas di akhir statement, jadi tidak saling bentrok.
    let v4_local = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok();
    if !v4_local {
        return false;
    }
    let v4_any = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok();
    if !v4_any {
        return false;
    }
    // IPv6 bisa saja tidak tersedia — hanya AddrInUse yang berarti terpakai.
    match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
        Ok(_) => true,
        Err(e) => e.kind() != ErrorKind::AddrInUse,
    }
}

/// Apakah ada proses yang menerima koneksi di localhost:port (IPv4 atau IPv6)?
pub fn is_listening(port: u16) -> bool {
    [IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)]
        .into_iter()
        .any(|ip| TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(150)).is_ok())
}

pub fn allocate(preferred: Option<u16>, reserved: &HashSet<u16>) -> Result<Allocation> {
    let usable = |p: u16| {
        p >= 1024 && !BROWSER_BLOCKED.contains(&p) && !reserved.contains(&p) && is_port_free(p)
    };

    if let Some(p) = preferred {
        if usable(p) {
            return Ok(Allocation { port: p, relocated: false });
        }
        let base = p.max(1024);
        for candidate in base.saturating_add(1)..=base.saturating_add(NEARBY_SPAN) {
            if candidate != p && usable(candidate) {
                return Ok(Allocation { port: candidate, relocated: true });
            }
        }
    }

    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let offset = (seed % u32::from(SAFE_RANGE_LEN)) as u16;
    for i in 0..SAFE_RANGE_LEN {
        let candidate = SAFE_RANGE_START + (offset + i) % SAFE_RANGE_LEN;
        if usable(candidate) {
            return Ok(Allocation { port: candidate, relocated: preferred.is_some() });
        }
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("Tidak ada port yang tersedia")?;
    let port = listener.local_addr()?.port();
    Ok(Allocation { port, relocated: preferred.is_some() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_busy_port() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let p = l.local_addr().unwrap().port();
        assert!(!is_port_free(p));
    }

    #[test]
    fn relocates_when_busy() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let p = l.local_addr().unwrap().port();
        let a = allocate(Some(p), &HashSet::new()).unwrap();
        assert!(a.relocated);
        assert_ne!(a.port, p);
    }

    #[test]
    fn respects_reservations() {
        // Port bebas tapi di-reserve repo lain → harus dialihkan.
        let free = allocate(None, &HashSet::new()).unwrap().port;
        let reserved: HashSet<u16> = [free].into();
        let a = allocate(Some(free), &reserved).unwrap();
        assert!(a.relocated);
        assert_ne!(a.port, free);
    }
}
