//! Command IPC yang dipanggil frontend (`src/lib/api.ts`).
//!
//! * Frontend hanya mengirim `id` repo — tidak pernah PID, path eksekusi, atau perintah shell.
//! * Command berat dijalankan di thread blocking (Tauri menjalankan command non-async di
//!   main thread, jadi clone/scan/stop yang lama akan membekukan UI jika tidak dipindah).
//! * Error `anyhow` dikirim ke frontend sebagai string (ditampilkan apa adanya).

use std::sync::Arc;

use crate::engine::{
    state::{self, Engine},
    types::{AppSettings, RepoInfo, RepoStatus, ScanReport},
    validation,
};

type CmdResult<T> = Result<T, String>;

async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce(Arc<Engine>) -> anyhow::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || f(state::get()?))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

// ── Repo ──

#[tauri::command]
pub async fn list_repos() -> CmdResult<Vec<RepoInfo>> {
    blocking(|e| Ok(e.list())).await
}

/// Clone → scan di karantina → workspace. Bisa beberapa menit (scan Defender).
#[tauri::command]
pub async fn add_repo_from_git(url: String, name: Option<String>) -> CmdResult<RepoInfo> {
    blocking(move |e| e.add_git(&url, name.as_deref())).await
}

#[tauri::command]
pub async fn add_repo_from_archive(path: String, name: Option<String>) -> CmdResult<RepoInfo> {
    blocking(move |e| e.add_archive(&path, name.as_deref())).await
}

#[tauri::command]
pub async fn delete_repo(id: String) -> CmdResult<()> {
    blocking(move |e| e.delete(&id)).await
}

/// Validasi cepat untuk form. `null` = valid.
#[tauri::command]
pub fn validate_git_url(url: String) -> Option<String> {
    validation::validate_git_url(&url).err().map(|e| e.to_string())
}

// ── Proses ──

#[tauri::command]
pub async fn start_repo(id: String, allow_suspicious: bool) -> CmdResult<RepoInfo> {
    blocking(move |e| e.start(&id, allow_suspicious)).await
}

#[tauri::command]
pub async fn stop_repo(id: String) -> CmdResult<()> {
    blocking(move |e| {
        e.stop(&id, crate::engine::types::EventKind::StatusChanged, Some("Dihentikan oleh pengguna.".into()))
    })
    .await
}

#[tauri::command]
pub async fn get_logs(id: String) -> CmdResult<Vec<String>> {
    blocking(move |e| Ok(e.logs(&id))).await
}

/// Membuka link localhost repo di browser default. URL diambil dari state core (bukan dari
/// frontend), sehingga webview tidak bisa dipakai untuk membuka URL/berkas sembarangan.
#[tauri::command]
pub async fn open_repo_url(id: String) -> CmdResult<()> {
    let url = blocking(move |e| {
        e.list()
            .into_iter()
            .find(|r| r.id == id && r.status == RepoStatus::Running)
            .and_then(|r| r.url)
            .ok_or_else(|| anyhow::anyhow!("Repo belum berjalan"))
    })
    .await?;
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn respond_to_alert(id: String, kill: bool) -> CmdResult<()> {
    blocking(move |e| e.respond_alert(&id, kill)).await
}

// ── Pengaturan & resource ──

#[tauri::command]
pub async fn get_settings() -> CmdResult<AppSettings> {
    blocking(|e| Ok(e.settings())).await
}

#[tauri::command]
pub async fn update_settings(settings: AppSettings) -> CmdResult<AppSettings> {
    blocking(move |e| e.update_settings(settings)).await
}

/// `repo_id = null` → semua repo yang tidak berjalan. Mengembalikan MB yang dibebaskan.
#[tauri::command]
pub async fn clean_cache(repo_id: Option<String>) -> CmdResult<f64> {
    blocking(move |e| e.clean_cache(repo_id.as_deref())).await
}

#[tauri::command]
pub fn cpu_count() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

// ── Security scan ──

#[tauri::command]
pub async fn get_scan_report(id: String) -> CmdResult<Option<ScanReport>> {
    blocking(move |e| e.scan_report(&id)).await
}

#[tauri::command]
pub async fn rescan_repo(id: String) -> CmdResult<ScanReport> {
    blocking(move |e| e.rescan(&id)).await
}
