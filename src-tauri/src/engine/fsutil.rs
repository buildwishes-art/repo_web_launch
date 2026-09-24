//! Utilitas filesystem yang aman terhadap path traversal dan symlink escape.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

/// Memastikan `target` (setelah kanonikalisasi, symlink di-resolve) berada DI DALAM `base`.
/// Dipakai sebelum setiap operasi destruktif dan sebelum mengeksekusi binary dari repo.
pub fn ensure_within(base: &Path, target: &Path) -> Result<PathBuf> {
    let base = fs::canonicalize(base).with_context(|| format!("Direktori tidak ada: {}", base.display()))?;
    let target =
        fs::canonicalize(target).with_context(|| format!("Path tidak ada: {}", target.display()))?;
    if target == base || !target.starts_with(&base) {
        bail!("Path {} berada di luar area yang diizinkan", target.display());
    }
    Ok(target)
}

/// `remove_dir_all` yang juga menangani file read-only (objek `.git` di Windows).
/// Tidak mengikuti symlink, jadi symlink di dalam repo tidak bisa dipakai untuk menghapus file luar.
pub fn remove_dir_force(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    for entry in WalkDir::new(path).follow_links(false).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = fs::set_permissions(entry.path(), perms);
            }
        }
    }
    fs::remove_dir_all(path).with_context(|| format!("Gagal menghapus {}", path.display()))
}

/// Tulis-lalu-rename agar file JSON tidak pernah setengah tertulis saat crash.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Membaca file teks kecil (manifest/config). `None` jika tidak ada atau melebihi `max_bytes`.
pub fn read_small(path: &Path, max_bytes: u64) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > max_bytes {
        return None;
    }
    fs::read_to_string(path).ok()
}
