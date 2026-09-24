//! Server file statis bawaan untuk repo HTML statis (tanpa perlu Node/Python).
//!
//! Dijalankan sebagai PROSES ANAK: `RepoLaunch.exe --serve-static <root> <port>` (lihat `main.rs`),
//! sehingga diperlakukan sama seperti runtime lain: di dalam Job Object, dimonitor CPU/RAM,
//! bisa di-stop/auto-kill, dan log-nya muncul di log viewer.
//!
//! Keamanan:
//! * Bind hanya ke 127.0.0.1.
//! * Path request di-decode lalu dikanonikalisasi; harus tetap di dalam root (anti `../`, anti symlink escape).
//! * Hanya GET/HEAD. Tidak ada directory listing.

use std::{
    fs::{self, File},
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use tiny_http::{Header, Method, Response, Server, StatusCode};

pub fn run(root: &Path, port: u16) -> Result<()> {
    let root = fs::canonicalize(root).with_context(|| format!("Root tidak ada: {}", root.display()))?;
    let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("Gagal bind port {port}: {e}"))?;
    // Format ini dikenali detektor URL RepoLaunch → link localhost muncul di UI.
    println!("RepoLaunch static server → http://localhost:{port}/  (root: {})", root.display());

    for request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        let (status, response) = if !matches!(method, Method::Get | Method::Head) {
            (405, Response::from_string("Method Not Allowed").with_status_code(StatusCode(405)).boxed())
        } else {
            match resolve(&root, &url) {
                Some(path) => match File::open(&path) {
                    Ok(file) => {
                        let mime = mime_guess::from_path(&path).first_or_octet_stream();
                        let header = Header::from_bytes("Content-Type", mime.essence_str()).expect("header valid");
                        let nosniff = Header::from_bytes("X-Content-Type-Options", "nosniff").expect("header valid");
                        (200, Response::from_file(file).with_header(header).with_header(nosniff).boxed())
                    }
                    Err(_) => (500, Response::from_string("Internal Server Error").with_status_code(StatusCode(500)).boxed()),
                },
                None => (404, Response::from_string("404 Not Found").with_status_code(StatusCode(404)).boxed()),
            }
        };
        println!("{method} {url} → {status}");
        let _ = request.respond(response);
    }
    Ok(())
}

/// Memetakan URL ke file di dalam `root`. `None` jika tidak ada / di luar root.
fn resolve(root: &Path, url: &str) -> Option<PathBuf> {
    let path_part = url.split(['?', '#']).next().unwrap_or("/");
    let decoded = percent_decode(path_part)?;

    let mut rel = PathBuf::new();
    for comp in Path::new(decoded.trim_start_matches('/')).components() {
        match comp {
            Component::Normal(c) => rel.push(c),
            Component::CurDir => {}
            _ => return None, // "..", prefix drive "C:", root → tolak
        }
    }
    let mut candidate = root.join(&rel);
    if candidate.is_dir() {
        candidate.push("index.html");
    } else if !candidate.exists() && candidate.extension().is_none() {
        candidate.set_extension("html"); // /about → about.html
    }
    let canonical = fs::canonicalize(&candidate).ok()?;
    (canonical.starts_with(root) && canonical.is_file()).then_some(canonical)
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    // Karakter kontrol / backslash tidak valid dalam path web.
    if decoded.chars().any(|c| c.is_control() || c == '\\') {
        return None;
    }
    Some(decoded)
}

/// Entry point mode CLI: `--serve-static <root> <port>`.
pub fn run_from_args(args: &[String]) -> Result<()> {
    let [root, port] = args else { bail!("Pemakaian: --serve-static <root> <port>") };
    run(Path::new(root), port.parse().context("Port tidak valid")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_traversal() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("index.html"), "ok").unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        assert!(resolve(&root, "/").is_some());
        assert!(resolve(&root, "/index.html?x=1").is_some());
        for bad in ["/../secret", "/%2e%2e/secret", "/..%5csecret", "/C:/Windows/win.ini", "/%00"] {
            assert!(resolve(&root, bad).is_none(), "{bad}");
        }
    }
}
