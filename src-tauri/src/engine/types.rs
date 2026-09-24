//! DTO yang dikirim ke frontend (Tauri IPC/event) dan disimpan ke disk.
//!
//! Semua tipe diserialisasi `camelCase` supaya cocok dengan `src/lib/types.ts`.
//! Enum diserialisasi sebagai string, mis. `RepoStatus::Running` → `"running"`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Git,
    Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepoStatus {
    Stopped,
    /// Menjalankan langkah setup (npm install, composer install, pip install, venv).
    Installing,
    /// Server sudah di-spawn, menunggu port menerima koneksi.
    Starting,
    Running,
    Crashed,
}

/// Snapshot satu repo: data registry + status runtime.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub id: String,
    pub name: String,
    pub source_kind: SourceKind,
    /// URL git atau nama file arsip asal.
    pub source: String,
    /// Label stack web, mis. "Vite", "Next.js", "HTML statis", "Laravel", "Django".
    pub stack: String,
    /// Unix epoch (detik).
    pub created_at: i64,
    pub status: RepoStatus,
    pub port: Option<u16>,
    /// Link localhost (hanya diisi saat Running).
    pub url: Option<String>,
    pub pid: Option<u32>,
    /// Dinormalisasi terhadap total core (0–100).
    pub cpu_percent: f64,
    pub memory_mb: f64,
    /// Pesan penting, mis. relokasi port atau alasan crash.
    pub notice: Option<String>,
    /// Label langkah yang sedang berjalan, mis. "Menginstal dependensi".
    pub phase: Option<String>,
    pub scan_verdict: ScanVerdict,
    pub scan_summary: Option<String>,
}

// ───────────────────────────── Security scan ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Info,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanVerdict {
    /// Belum pernah dipindai — akan dipindai otomatis saat Start.
    NotScanned,
    Clean,
    /// Ada temuan berisiko tinggi, atau ≥ 3 jenis temuan sedang. Start diblokir kecuali dipaksa.
    Suspicious,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinding {
    pub severity: Severity,
    /// ID aturan, mis. "ps_encoded", "disguised_pe".
    pub rule: String,
    pub title: String,
    /// Path relatif terhadap root repo.
    pub file: Option<String>,
    pub line: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub verdict: ScanVerdict,
    pub summary: String,
    /// "Repo ini terdeteksi tidak normal" jika `verdict == Suspicious`.
    pub note: Option<String>,
    /// Diurutkan dari severity tertinggi.
    pub findings: Vec<ScanFinding>,
    pub scanned_files: u32,
    /// Hasil lapis antivirus, mis. "Windows Defender: tidak ada ancaman".
    pub antivirus: String,
    pub scanned_at: i64,
}

// ───────────────────────────── Event ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    StatusChanged,
    Metrics,
    /// Ambang terlampaui selama `sustain_secs` → toast Windows ber-aksi (dikirim dari Rust).
    ResourceAlert,
    /// Resource kembali normal sebelum pengguna merespons.
    AlertCleared,
    /// Dihentikan karena tidak ada respons dalam `auto_kill_after_secs`.
    AutoKilled,
    IdleStopped,
    UrlDetected,
}

/// Event push dari core. Diteruskan ke frontend sebagai event Tauri `"core-event"`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreEvent {
    pub kind: EventKind,
    pub repo_id: String,
    pub repo_name: String,
    pub status: Option<RepoStatus>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub cpu_percent: Option<f64>,
    pub memory_mb: Option<f64>,
    pub message: Option<String>,
}

// ───────────────────────────── Pengaturan ─────────────────────────────

/// Disimpan sebagai `settings.json` di app data dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Ambang CPU (% dari total mesin).
    pub cpu_threshold_percent: f64,
    /// Ambang RAM (MB) untuk seluruh pohon proses repo.
    pub memory_threshold_mb: f64,
    /// Berapa lama ambang harus terlampaui terus-menerus sebelum notifikasi dikirim.
    pub sustain_secs: u32,
    /// 0 = menunggu jawaban pengguna; >0 = kill otomatis jika tidak dijawab.
    pub auto_kill_after_secs: u32,
    /// Setelah pengguna menjawab "Tidak", jangan peringatkan lagi selama ini.
    pub snooze_secs: u32,
    /// 0 = nonaktif. Repo idle (CPU rendah & tanpa output log) selama ini akan dihentikan.
    pub idle_timeout_mins: u32,
    pub idle_cpu_percent: f64,
    /// 0 = tanpa batas. Membatasi proses ke N core pertama (affinity).
    pub cpu_cores_limit: u32,
    /// 0 = tanpa batas. Hard cap CPU (%) via Job Object.
    pub cpu_rate_limit_percent: u32,
    /// 0 = tanpa batas. Hard limit memori (MB) via Job Object.
    pub memory_hard_limit_mb: u32,
    /// Prioritas BELOW_NORMAL via Job Object.
    pub low_priority: bool,
    pub clean_cache_on_stop: bool,
    /// Default `false` → `npm install --ignore-scripts` / `composer install --no-scripts`.
    pub allow_install_scripts: bool,
    /// Sertakan Windows Defender dalam scan keamanan repo.
    pub antivirus_scan: bool,
    /// Tombol minimize menyembunyikan jendela ke system tray (menghilang dari taskbar).
    /// Default `false`: minimize biasa, jendela tetap terlihat di taskbar.
    pub minimize_to_tray: bool,
    /// Tombol close (X) menyembunyikan ke tray alih-alih keluar. Keluar lewat menu tray.
    pub close_to_tray: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            cpu_threshold_percent: 80.0,
            memory_threshold_mb: 1024.0,
            sustain_secs: 30,
            auto_kill_after_secs: 0,
            snooze_secs: 600,
            idle_timeout_mins: 30,
            idle_cpu_percent: 1.0,
            cpu_cores_limit: 0,
            cpu_rate_limit_percent: 0,
            memory_hard_limit_mb: 0,
            low_priority: true,
            clean_cache_on_stop: false,
            allow_install_scripts: false,
            antivirus_scan: true,
            minimize_to_tray: false,
            close_to_tray: true,
        }
    }
}
