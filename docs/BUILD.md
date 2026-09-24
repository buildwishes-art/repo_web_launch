# Build RepoLaunch (Windows)

## 1. Prasyarat

| Kebutuhan | Keterangan |
|---|---|
| Node.js ≥ 20 | untuk frontend (Vite + Svelte) dan Tauri CLI |
| Rust stable ≥ 1.80 (`x86_64-pc-windows-msvc`) | https://rustup.rs |
| Visual Studio 2022 Build Tools | workload **"Desktop development with C++"** |
| WebView2 Runtime | sudah ada di Windows 11; installer mengunduhnya otomatis bila belum ada |

Runtime web yang ingin dijalankan (opsional, sesuai repo): Node.js, PHP + Composer, Python.
Repo HTML statis tidak butuh runtime apa pun.

## 2. Development

```bash
npm install
```

```bash
npm run app:dev
```

`app:dev` = `tauri dev`: menjalankan Vite di `http://localhost:1420` lalu membuka jendela Tauri
dengan hot-reload untuk UI dan rebuild otomatis untuk Rust.

Mengerjakan UI saja tanpa Rust (IPC tiruan dari `src/dev-mock.ts`, tidak ikut build produksi):

```bash
npm run dev
```

## 3. Build installer

```bash
npm run app:build
```

Hasil build otomatis disalin (oleh `scripts/release.mjs`) ke folder **`release/`** di root proyek;
artefak versi lama di folder itu diganti:
* `RepoLaunch_<versi>_x64-setup.exe` — installer per-user (tanpa admin)
* `RepoLaunch_<versi>_x64_en-US.msi` — installer MSI
* `RepoLaunch-portable.exe` — tanpa instalasi

Build + langsung instal (silent, per-user) + jalankan aplikasinya:

```bash
npm run app:install
```

Instal ulang dari isi `release/` tanpa build ulang:

```bash
npm run release:install
```

Aplikasi terpasang di `%LOCALAPPDATA%\RepoLaunch\` dan muncul di Start Menu serta Settings → Apps
(untuk uninstall). Instance yang sedang berjalan ditutup otomatis sebelum instalasi.

Installer mendaftarkan shortcut Start Menu dengan AppUserModelID `com.repolaunch.app`, sehingga
toast notification tampil atas nama "RepoLaunch". (Saat `app:dev`, toast tampil atas nama PowerShell.)

## 4. Test & pemeriksaan

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Mencakup: validasi URL (injection/SSRF), sanitasi nama, zip-slip, flatten arsip, port checker & relokasi,
deteksi proyek web (Vite/Next/static/PHP/Laravel/FastAPI) & penolakan repo non-web, scanner keamanan,
dan anti path-traversal server statis.

```bash
npm run check
```

## 5. Troubleshooting

| Gejala | Solusi |
|---|---|
| `link.exe not found` | Instal VS Build Tools (workload C++), buka terminal baru. |
| Build pertama sangat lama | Normal (kompilasi gix, tauri, rustls). Build berikutnya inkremental. |
| `Runtime 'npm' tidak ditemukan` | Instal Node.js dan pastikan ada di PATH **sebelum** membuka RepoLaunch. |
| Python gagal membuat venv | Matikan alias "App execution aliases → python.exe" (stub Microsoft Store) atau instal Python dari python.org. |
| Toast tidak muncul | Settings → System → Notifications; pastikan aplikasi diinstal lewat installer (build release). |
| Ikon taskbar default/kosong setelah `app:install` dari terminal aplikasi ter-sandbox (mis. aplikasi desktop Claude) | Instalasi tervirtualisasi ke `Packages\<app>\LocalCache`. `release.mjs` kini menolak instal di kondisi ini — instal dengan klik dua kali `release\RepoLaunch_<versi>_x64-setup.exe` dari File Explorer. |
| Ikon baru tidak muncul setelah mengganti `icons/icon.ico` | Sudah ditangani `build.rs` (`rerun-if-changed=icons`). Gunakan ICO standar: BMP/DIB untuk ≤ 64 px, PNG hanya untuk 256 px. |
| Ikon tray tidak terlihat | Klik panah "^" di taskbar; seret ikon RepoLaunch ke area yang selalu tampil. |
| API gix/sysinfo berubah | Versi dipin di Cargo.toml; cek changelog `prepare_clone` (gix) dan `refresh_processes_specifics` (sysinfo) saat upgrade. |
