//! Git clone menggunakan `gix` (implementasi Git pure-Rust).
//!
//! Kenapa bukan `git` CLI?
//! * Tidak ada shell dan tidak ada parsing argumen → menutup kelas bug argument injection.
//! * Tidak menjalankan hook, tidak mengambil submodule, tidak memakai credential helper sistem.
//! * Tidak butuh Git for Windows terinstal di mesin pengguna.

use std::{num::NonZeroU32, path::Path, sync::atomic::AtomicBool};

use anyhow::{bail, Context, Result};
use url::Url;

/// Shallow clone (depth 1) ke `dest`. `dest` belum boleh ada.
pub fn clone_shallow(url: &Url, dest: &Path) -> Result<()> {
    if dest.exists() {
        bail!("Direktori tujuan sudah ada: {}", dest.display());
    }
    let interrupt = AtomicBool::new(false);
    let depth = NonZeroU32::new(1).expect("1 > 0");

    let mut prepare = gix::prepare_clone(url.as_str(), dest)
        .context("Gagal menyiapkan clone")?
        .with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(depth));
    let (mut checkout, _outcome) = prepare
        .fetch_then_checkout(gix::progress::Discard, &interrupt)
        .context("Gagal mengunduh repositori (cek URL / koneksi / repo privat)")?;
    checkout
        .main_worktree(gix::progress::Discard, &interrupt)
        .context("Gagal melakukan checkout working tree")?;
    Ok(())
}
