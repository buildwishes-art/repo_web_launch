//! Pembersihan cache build otomatis/manual.
//!
//! Hanya menghapus direktori cache yang *dapat dibuat ulang* (bukan node_modules atau .venv),
//! dan setiap target diverifikasi berada di dalam direktori repo sebelum dihapus.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use walkdir::WalkDir;

use super::fsutil;

const CACHE_DIRS: &[&str] = &[
    "node_modules/.cache",
    "node_modules/.vite",
    ".next/cache",
    ".nuxt",
    ".parcel-cache",
    ".turbo",
    ".angular/cache",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    "target/debug/incremental",
    "target/release/incremental",
];

/// Mengembalikan jumlah byte yang dibebaskan.
pub fn clean_repo(repo_dir: &Path) -> Result<u64> {
    let mut freed = 0;
    for rel in CACHE_DIRS {
        let candidate = repo_dir.join(rel);
        if !candidate.is_dir() {
            continue;
        }
        // Menolak symlink yang mengarah ke luar repo.
        let Ok(safe) = fsutil::ensure_within(repo_dir, &candidate) else { continue };
        freed += fsutil::dir_size(&safe);
        fsutil::remove_dir_force(&safe)?;
    }

    // __pycache__ tersebar di seluruh source tree (lewati dependensi & .git).
    let pycaches: Vec<PathBuf> = WalkDir::new(repo_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("node_modules" | ".venv" | ".git" | "target")))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir() && e.file_name() == "__pycache__")
        .map(|e| e.into_path())
        .collect();
    for dir in pycaches {
        freed += fsutil::dir_size(&dir);
        let _ = fsutil::remove_dir_force(&dir);
    }
    Ok(freed)
}

/// Mengosongkan isi direktori (direktorinya sendiri dipertahankan).
pub fn clean_dir_contents(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut freed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            freed += fsutil::dir_size(&path);
            let _ = fsutil::remove_dir_force(&path);
        } else {
            freed += entry.metadata().map_or(0, |m| m.len());
            let _ = fs::remove_file(&path);
        }
    }
    freed
}
