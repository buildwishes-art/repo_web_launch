<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" alt="RepoLaunch" />
</p>

<h1 align="center">RepoLaunch</h1>

<p align="center">
  <b>Cara paling simpel untuk <i>install</i>, <i>scan</i>, dan <i>start</i> semua repositori berbasis web — satu klik.</b><br/>
  Aplikasi desktop Windows · Tauri 2 (Rust) + Svelte 5
</p>

---

## Tujuan

Menjalankan proyek web dari GitHub biasanya berarti: clone, baca README, tebak perintah
(`npm run dev`? `php artisan serve`? `python manage.py runserver`?), instal dependensi,
bentrok port dengan proyek lain, lalu berharap tidak ada script berbahaya di dalamnya.

**RepoLaunch memangkas semua itu menjadi tiga langkah:**

| 1. **Install** | 2. **Scan** | 3. **Start** |
|---|---|---|
| Tempel link Git (https) atau pilih arsip `.zip` / `.tar.gz`. | Repo dipindai keamanannya di folder karantina **sebelum** boleh dijalankan. | Klik **Start** — RepoLaunch mendeteksi cara menjalankannya, memilih port yang aman, dan menampilkan link `http://localhost:…` yang bisa langsung diklik. |

Aplikasi ini **khusus repositori berbasis web**. Repo yang bukan proyek web (CLI tool, library,
dll.) ditolak saat instalasi.

## Proyek web yang didukung

| Jenis | Terdeteksi dari | Dijalankan dengan |
|---|---|---|
| **Node.js** — Vite, Next.js, Nuxt, Astro, SvelteKit, Remix, Angular, Create React App, Gatsby, Parcel, Webpack, Express/Fastify/Koa/Hono/NestJS | `package.json` + framework web | `npm` / `pnpm` / `yarn` / `bun run dev` (dependensi diinstal otomatis) |
| **HTML statis** | `index.html` di root, `public/`, `dist/`, `docs/`, `build/`, `site/` | server statis bawaan — **tidak perlu Node** |
| **PHP / Laravel** | `index.php`, `artisan` | `php -S` / `php artisan serve` (+ `composer install`) |
| **Python** — Django, FastAPI, Flask | `manage.py`, `FastAPI(`, `Flask(` | virtualenv otomatis + `runserver` / `uvicorn` / `flask run` |

## Fitur

### 🛡️ Scan keamanan sebelum dijalankan
Setiap repo baru dipindai di folder karantina — tanpa mengeksekusi kode apa pun dari repo:
* **Analisis statis**: executable yang disamarkan (mis. `logo.png` berisi `.exe`), ekstensi ganda,
  script `postinstall` yang mengunduh/mengeksekusi kode, PowerShell ter-encode, `eval(atob(...))`,
  reverse shell, crypto-miner, pencuri kredensial browser/wallet, autostart registry,
  task VS Code yang otomatis jalan, registry paket palsu, dan lainnya.
* **Windows Defender** memindai folder karantina.

Jika mencurigakan, muncul **"Note: Repo ini terdeteksi tidak normal"** beserta daftar temuannya,
dan tombol Start diblokir sampai pengguna mengonfirmasi secara eksplisit.

### 🔌 Port otomatis tanpa bentrok
Port bawaan repo dideteksi (`.env`, script `package.json`, `vite.config`, default framework).
Jika sudah dipakai repo lain atau aplikasi lain, RepoLaunch otomatis memindahkan ke port bebas
dan memberi tahu — misalnya *"Port 3000 dipakai repo 'api'. Dialihkan ke 3001."*

### 🚀 One-click start & link localhost
Deteksi perintah yang tepat, instal dependensi, jalankan, lalu tampilkan link localhost begitu
server benar-benar siap menerima koneksi. Log stdout/stderr bisa dilihat kapan saja.

### 📊 Pemantauan resource
CPU & RAM setiap repo dipantau (termasuk seluruh proses anaknya). Jika melewati ambang
(default CPU > 80% atau RAM > 1 GB selama 30 detik), muncul notifikasi Windows:
*"Penggunaan resource untuk repo X terlalu tinggi. Apakah Anda ingin menghentikannya?"*
dengan tombol **Ya (Hentikan)** / **Tidak** — plus opsi hentikan otomatis bila tidak dijawab.

### ⚙️ Hemat resource & aman
* Setiap proses berjalan di **Windows Job Object**: batas core CPU, hard cap CPU/RAM, prioritas rendah,
  dan seluruh proses ikut berhenti bila RepoLaunch ditutup (tidak ada proses yatim).
* Environment dibersihkan (token/rahasia tidak diteruskan), hanya runtime dari allowlist yang boleh dijalankan,
  `npm install --ignore-scripts` secara default.
* Repo idle dihentikan otomatis; pembersihan cache build satu klik.

### 🗂️ System tray
Tombol close (X) menyembunyikan RepoLaunch ke tray — repo tetap berjalan & dipantau.
Klik kanan ikon tray: **Buka · Hentikan semua repo · Keluar**.

## Instalasi

1. Unduh `RepoLaunch_<versi>_x64-setup.exe` dari halaman **[Releases](https://github.com/buildwishes-art/repo_web_launch/releases)**
   (atau build sendiri, lihat di bawah).
2. Klik dua kali installer — instalasi per-user, tidak butuh admin.
3. Buka **RepoLaunch** dari Start Menu.

Instal runtime sesuai repo yang ingin dijalankan: [Node.js](https://nodejs.org), PHP + Composer, atau Python.
Repo HTML statis tidak butuh runtime apa pun.

**Persyaratan:** Windows 10/11 64-bit (WebView2 sudah ada di Windows 11; installer mengunduhnya bila belum ada).

## Cara pakai

1. **Tambah repo** → tempel URL `https://github.com/owner/repo` atau pilih arsip → **Install & scan**.
2. Tunggu hasil scan. Badge 🛡️ **Aman** berarti siap dijalankan.
3. Klik **Start** → tunggu status **Berjalan** → klik link `http://localhost:…`.
4. **Stop** untuk menghentikan, menu ⋮ untuk log, scan ulang, bersihkan cache, atau hapus repo.

## Build dari source

Prasyarat: Node.js ≥ 20, Rust stable, Visual Studio 2022 Build Tools (workload C++).

```bash
npm install
npm run app:dev      # mode development (hot reload)
npm run app:build    # installer .exe/.msi + exe portable → folder release/
cargo test --manifest-path src-tauri/Cargo.toml
```

Detail: [docs/BUILD.md](docs/BUILD.md) · Arsitektur & model keamanan: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

## Struktur proyek

```text
src/            UI (Svelte 5 + TypeScript)
src-tauri/      Aplikasi Rust
  src/engine/   inti: deteksi proyek web, scan keamanan, port, sandbox, monitor
  src/tray.rs   system tray   ·   src/notify.rs   notifikasi Windows
scripts/        otomatisasi rilis (salin hasil build ke release/)
docs/           dokumentasi build & arsitektur
```

## Catatan keamanan

Scan keamanan bersifat heuristik dan sandbox proses bersifat *containment* — keduanya mengurangi
risiko, tetapi tidak menjamin repo 100% aman. Jangan menjalankan repo dari sumber yang tidak Anda percaya.

## Lisensi

[MIT](LICENSE) © 2026 buildwishes-art
