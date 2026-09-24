//! Validasi input dari pengguna: URL git, nama repo, dan file arsip.
//!
//! Lapisan pertahanan terhadap command/argument injection:
//! 1. Git di-clone dengan `gix` (library), bukan `git` CLI + shell → tidak ada string yang
//!    di-interpretasi shell. Validasi di sini tetap ketat sebagai defense-in-depth.
//! 2. Hanya `https://` ke host publik (menolak `file://`, `ext::`, `ssh`, jaringan internal/SSRF).
//! 3. Nama repo disanitasi ke `[A-Za-z0-9._-]` sehingga aman sebagai nama direktori.
//! 4. Arsip dicek ukuran, ekstensi, DAN magic bytes-nya.

use std::{fs::File, io::Read, net::Ipv4Addr, path::Path};

use anyhow::{bail, Context, Result};
use url::{Host, Url};

pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_URL_LEN: usize = 2048;
const MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

pub fn validate_git_url(raw: &str) -> Result<Url> {
    let s = raw.trim();
    if s.is_empty() || s.len() > MAX_URL_LEN {
        bail!("URL kosong atau terlalu panjang");
    }
    if s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        bail!("URL tidak boleh mengandung spasi atau karakter kontrol");
    }
    if s.starts_with('-') {
        bail!("URL tidak boleh diawali '-' (argument injection)");
    }
    if s.contains("::") {
        bail!("Transport helper git (mis. ext::) tidak diizinkan");
    }

    let url = Url::parse(s).context("Format URL tidak valid")?;
    if url.scheme() != "https" {
        bail!("Hanya URL https:// yang diizinkan");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Jangan menyertakan kredensial di URL");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("URL repositori tidak boleh memiliki query atau fragment");
    }
    let host = url.host().context("URL tidak memiliki host")?;
    if is_internal_host(&host) {
        bail!("Host lokal/jaringan internal tidak diizinkan");
    }
    let has_path = url.path_segments().is_some_and(|mut s| s.any(|seg| !seg.is_empty()));
    if !has_path {
        bail!("URL harus menunjuk ke repositori, mis. https://github.com/owner/repo");
    }
    Ok(url)
}

fn is_internal_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || (o[0] == 100 && (o[1] & 0xC0) == 64) // CGNAT 100.64.0.0/10
}

fn is_internal_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(d) => {
            let d = d.to_ascii_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || !d.contains('.') // hostname intranet tanpa TLD
        }
        Host::Ipv4(ip) => is_internal_v4(*ip),
        Host::Ipv6(ip) => {
            let seg0 = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00 // unique local
                || (seg0 & 0xffc0) == 0xfe80 // link local
                || ip.to_ipv4_mapped().is_some_and(is_internal_v4)
        }
    }
}

/// Mengubah input bebas menjadi nama direktori yang aman di Windows.
pub fn sanitize_repo_name(raw: &str) -> Result<String> {
    let mapped: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' })
        .collect();
    let trimmed = mapped.trim_matches(|c| c == '.' || c == '-');
    let name: String = trimmed.chars().take(MAX_NAME_LEN).collect();
    if name.is_empty() {
        bail!("Nama repo tidak valid");
    }

    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        bail!("'{name}' adalah nama perangkat yang dicadangkan Windows");
    }
    Ok(name)
}

pub fn name_from_url(url: &Url) -> String {
    url.path_segments()
        .and_then(|segs| segs.filter(|s| !s.is_empty()).last())
        .map(|s| s.trim_end_matches(".git").to_string())
        .unwrap_or_else(|| "repo".into())
}

pub fn name_from_archive(path: &Path) -> String {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = file.to_ascii_lowercase();
    let cut = [".tar.gz", ".tgz", ".zip"]
        .iter()
        .find(|ext| lower.ends_with(*ext))
        .map_or(file.len(), |ext| file.len() - ext.len());
    file[..cut].to_string()
}

pub fn validate_archive(path: &Path) -> Result<ArchiveKind> {
    // symlink_metadata: jangan ikuti symlink yang mungkin menunjuk ke file sistem.
    let meta = std::fs::symlink_metadata(path).context("File arsip tidak ditemukan")?;
    if !meta.is_file() {
        bail!("Path bukan file biasa");
    }
    if meta.len() == 0 || meta.len() > MAX_ARCHIVE_BYTES {
        bail!("Ukuran arsip harus antara 1 byte dan {} MB", MAX_ARCHIVE_BYTES / 1024 / 1024);
    }

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .context("Nama file tidak valid")?;
    let kind = if name.ends_with(".zip") {
        ArchiveKind::Zip
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        ArchiveKind::TarGz
    } else {
        bail!("Format tidak didukung. Gunakan .zip, .tar.gz, atau .tgz");
    };

    let mut magic = [0u8; 4];
    File::open(path)?.read_exact(&mut magic).context("Gagal membaca header arsip")?;
    let ok = match kind {
        ArchiveKind::Zip => magic == [0x50, 0x4B, 0x03, 0x04],
        ArchiveKind::TarGz => magic[..2] == [0x1F, 0x8B],
    };
    if !ok {
        bail!("Isi file tidak cocok dengan ekstensinya (magic bytes salah)");
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_https() {
        assert!(validate_git_url("https://github.com/rust-lang/rustlings.git").is_ok());
        assert!(validate_git_url("  https://gitlab.com/group/sub/project  ").is_ok());
    }

    #[test]
    fn rejects_injection_and_ssrf() {
        for bad in [
            "--upload-pack=touch /tmp/pwn",
            "ext::sh -c touch% /tmp/pwned",
            "https://github.com/a/b;rm${IFS}-rf${IFS}/ x",
            "https://github.com/a/b\nfoo",
            "file:///etc/passwd",
            "http://github.com/a/b",
            "ssh://git@github.com/a/b",
            "https://localhost/a/b",
            "https://127.0.0.1/a",
            "https://10.0.0.5/a",
            "https://[::1]/a",
            "https://intranet/a",
            "https://user:pass@github.com/a/b",
            "https://github.com/",
        ] {
            assert!(validate_git_url(bad).is_err(), "harus ditolak: {bad:?}");
        }
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_repo_name("../../etc").unwrap(), "etc");
        assert_eq!(sanitize_repo_name("my app!").unwrap(), "my-app");
        assert!(sanitize_repo_name("CON").is_err());
        assert!(sanitize_repo_name("...").is_err());
    }

    #[test]
    fn archive_names() {
        assert_eq!(name_from_archive(Path::new("/x/My-Repo-main.tar.gz")), "My-Repo-main");
        assert_eq!(name_from_archive(Path::new("site.ZIP")), "site");
    }
}
