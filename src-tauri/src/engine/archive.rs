//! Ekstraksi arsip yang aman.
//!
//! Proteksi:
//! * Zip-slip / path traversal: `enclosed_name()` (zip) & cek komponen path (tar).
//! * Symlink, hardlink, device, FIFO dilewati → tidak bisa menulis ke luar direktori tujuan.
//! * Zip bomb: batas jumlah entri dan total byte hasil ekstraksi (dihitung dari byte nyata).
//! * Permission/xattr dari arsip tidak diterapkan (tidak ada bit setuid/executable liar).

use std::{
    fs::{self, File},
    io::{self, BufReader, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};

use super::validation::ArchiveKind;

const MAX_ENTRIES: usize = 200_000;
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub fn extract(archive: &Path, kind: ArchiveKind, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    match kind {
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::TarGz => extract_tar_gz(archive, dest),
    }
    .context("Gagal mengekstrak arsip")
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    if zip.len() > MAX_ENTRIES {
        bail!("Arsip berisi terlalu banyak file ({})", zip.len());
    }
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("Entri berbahaya (path traversal): {}", entry.name()))?;
        let is_symlink = entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        if is_symlink {
            continue;
        }
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let remaining = MAX_UNPACKED_BYTES.saturating_sub(total);
        let mut out = File::create(&out_path)?;
        // Batasi dengan byte nyata, bukan ukuran yang diklaim header (bisa dipalsukan).
        let written = io::copy(&mut (&mut entry).take(remaining + 1), &mut out)?;
        total += written;
        if total > MAX_UNPACKED_BYTES {
            bail!("Hasil ekstraksi melebihi {} GB (kemungkinan zip bomb)", MAX_UNPACKED_BYTES >> 30);
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(BufReader::new(File::open(archive)?));
    let mut ar = tar::Archive::new(gz);
    ar.set_preserve_permissions(false);
    ar.set_preserve_mtime(false);
    ar.set_unpack_xattrs(false);
    ar.set_overwrite(true);

    let (mut count, mut total) = (0usize, 0u64);
    for entry in ar.entries()? {
        let mut entry = entry?;
        count += 1;
        if count > MAX_ENTRIES {
            bail!("Arsip berisi terlalu banyak file");
        }
        let et = entry.header().entry_type();
        if !(et.is_file() || et.is_dir()) {
            continue; // symlink, hardlink, device, fifo, pax header
        }
        let path = entry.path()?.into_owned();
        if path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            bail!("Entri berbahaya (path traversal): {}", path.display());
        }
        total += entry.size();
        if total > MAX_UNPACKED_BYTES {
            bail!("Hasil ekstraksi terlalu besar (kemungkinan tar bomb)");
        }
        // unpack_in juga menolak path yang keluar dari `dest` (defense-in-depth).
        if !entry.unpack_in(dest)? {
            bail!("Entri ditolak: {}", path.display());
        }
    }
    Ok(())
}

/// Direktori yang akan menjadi root repo (dipakai juga oleh scan keamanan di karantina).
pub fn content_root(staging: &Path, flatten: bool) -> Result<PathBuf> {
    if flatten {
        let entries: Vec<_> = fs::read_dir(staging)?
            .filter_map(Result::ok)
            .filter(|e| e.file_name() != "__MACOSX")
            .collect();
        if entries.len() == 1 && entries[0].file_type()?.is_dir() {
            return Ok(entries[0].path());
        }
    }
    Ok(staging.to_path_buf())
}

/// Memindahkan hasil staging ke direktori final.
/// Jika `flatten` dan arsip hanya berisi satu folder root (pola `repo-main/` dari GitHub),
/// folder itu yang dijadikan root repo.
pub fn promote(staging: &Path, dest: &Path, flatten: bool) -> Result<()> {
    let src = content_root(staging, flatten)?;
    fs::rename(&src, dest).context("Gagal memindahkan repo ke workspace")?;
    if src != staging {
        let _ = fs::remove_dir_all(staging);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_zip_slip() {
        let dir = tempfile::tempdir().unwrap();
        let zpath = dir.path().join("evil.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&zpath).unwrap());
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("../evil.txt", opts).unwrap();
            w.write_all(b"pwned").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.path().join("out");
        assert!(extract(&zpath, ArchiveKind::Zip, &dest).is_err());
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn flattens_single_root() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("stage");
        fs::create_dir_all(staging.join("repo-main")).unwrap();
        fs::write(staging.join("repo-main").join("package.json"), "{}").unwrap();
        let dest = dir.path().join("final");
        promote(&staging, &dest, true).unwrap();
        assert!(dest.join("package.json").is_file());
    }
}
