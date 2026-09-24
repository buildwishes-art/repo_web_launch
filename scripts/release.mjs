// Salin hasil `tauri build` ke folder release/ di root proyek, opsional langsung instal.
//
//   node scripts/release.mjs            → salin installer + exe portable ke release/
//   node scripts/release.mjs --install  → salin, lalu instal diam-diam (per-user, tanpa admin) & jalankan
//
// Dipanggil otomatis oleh `npm run app:build` dan `npm run app:install` (lihat package.json).

import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const { productName, version } = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const target = join(root, 'src-tauri', 'target', 'release');
const out = join(root, 'release');
const install = process.argv.includes('--install');

const setupName = `${productName}_${version}_x64-setup.exe`;
const artifacts = [
  { from: join(target, 'bundle', 'nsis', setupName), to: setupName },
  { from: join(target, 'bundle', 'msi', `${productName}_${version}_x64_en-US.msi`), to: `${productName}_${version}_x64_en-US.msi` },
  { from: join(target, 'repolaunch.exe'), to: `${productName}-portable.exe` },
];

const mb = (p) => (statSync(p).size / 1024 / 1024).toFixed(1).padStart(6) + ' MB';

// ── 1. Salin artefak ──
mkdirSync(out, { recursive: true });
const fresh = artifacts.filter((a) => existsSync(a.from));
if (fresh.length) {
  // Buang artefak versi lama (hanya file milik produk ini) agar release/ selalu berisi build terbaru.
  for (const f of readdirSync(out)) {
    if (f.startsWith(productName) && /\.(exe|msi)$/i.test(f)) rmSync(join(out, f), { force: true });
  }
  for (const a of fresh) copyFileSync(a.from, join(out, a.to));
  console.log(`\n✔ Artefak ${productName} ${version} disalin ke release/:`);
  for (const a of fresh) console.log(`   ${a.to.padEnd(40)}${mb(join(out, a.to))}`);
} else {
  console.log('ℹ Tidak ada hasil build baru di src-tauri/target/release — memakai isi release/ yang ada.');
}

/**
 * Terminal di dalam aplikasi MSIX ter-sandbox (mis. aplikasi desktop Claude) mem-virtualisasi
 * %LOCALAPPDATA%: installer "berhasil", tetapi file masuk ke Packages\<app>\LocalCache sehingga
 * Explorer/taskbar tidak bisa me-resolve aplikasinya (ikon default/kosong). Deteksi dengan menulis
 * file penanda lalu mencarinya di cache paket.
 */
function sandboxPackage() {
  const local = process.env.LOCALAPPDATA;
  if (!local) return null;
  const marker = `repolaunch-vfs-probe-${process.pid}.tmp`;
  try {
    writeFileSync(join(local, marker), '');
    const pkgs = join(local, 'Packages');
    for (const pkg of existsSync(pkgs) ? readdirSync(pkgs) : []) {
      const probe = join(pkgs, pkg, 'LocalCache', 'Local', marker);
      if (existsSync(probe)) {
        rmSync(probe, { force: true });
        return pkg;
      }
    }
    return null;
  } finally {
    rmSync(join(local, marker), { force: true });
  }
}

// ── 2. Instal (opsional) ──
if (install) {
  const setup = join(out, setupName);
  if (!existsSync(setup)) {
    console.error(`✖ ${setupName} tidak ditemukan di release/. Jalankan "npm run app:build" dulu.`);
    process.exit(1);
  }
  const pkg = sandboxPackage();
  if (pkg) {
    console.error(
      `\n✖ Terminal ini berjalan di dalam aplikasi ter-sandbox (${pkg}); instalasi akan tervirtualisasi\n` +
        `  dan ikon/shortcut RepoLaunch tidak akan berfungsi di taskbar & Start Menu.\n` +
        `  Instal dari File Explorer: klik dua kali release\\${setupName}\n` +
        `  (atau jalankan "npm run release:install" dari Windows Terminal biasa).`,
    );
    process.exit(2);
  }

  // Tutup instance yang sedang berjalan (installed maupun portable) agar file bisa diganti.
  // Proses repo milik RepoLaunch ikut berhenti otomatis (Job Object KILL_ON_JOB_CLOSE).
  for (const exe of ['repolaunch.exe', `${productName}-portable.exe`]) {
    spawnSync('taskkill', ['/IM', exe, '/F'], { stdio: 'ignore' });
  }

  console.log(`\n→ Menginstal ${setupName} (per-user, silent)…`);
  const res = spawnSync(setup, ['/S'], { stdio: 'inherit' });
  if (res.status !== 0) {
    console.error(`✖ Installer keluar dengan kode ${res.status}`);
    process.exit(res.status ?? 1);
  }

  const installed = join(process.env.LOCALAPPDATA ?? '', productName, 'repolaunch.exe');
  if (!existsSync(installed)) {
    console.error(`✖ Instalasi selesai, tapi ${installed} tidak ditemukan.`);
    process.exit(1);
  }
  console.log(`✔ Terinstal di ${installed}`);
  console.log('  Shortcut: Start Menu → RepoLaunch. Uninstall: Settings → Apps → RepoLaunch.');

  spawn(installed, [], { detached: true, stdio: 'ignore' }).unref();
  console.log('✔ RepoLaunch dijalankan.');
}
