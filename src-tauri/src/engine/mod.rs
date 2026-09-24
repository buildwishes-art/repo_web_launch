//! Core RepoLaunch (independen dari Tauri — bisa dites dengan `cargo test` tanpa UI).
//!
//! ```text
//! state ──┬── registry        (repos.json)
//!         ├── git / archive / validation   (instalasi repo ke karantina .staging)
//!         ├── detect          (hanya proyek web: Node, HTML statis, PHP, Django/FastAPI/Flask)
//!         ├── scan            (scan keamanan statis + Windows Defender di karantina)
//!         ├── port            (port checker + dynamic allocation)
//!         ├── sandbox         (spawn di Job Object, env bersih, allowlist)
//!         ├── static_server   (server HTML statis bawaan, dijalankan sebagai proses anak)
//!         ├── monitor         (thread latar: exit, readiness, CPU/RAM, alert, idle)
//!         └── cache / fsutil
//! ```

pub mod archive;
pub mod cache;
pub mod detect;
pub mod fsutil;
pub mod git;
pub mod monitor;
pub mod port;
pub mod registry;
pub mod sandbox;
pub mod scan;
pub mod state;
pub mod static_server;
pub mod types;
pub mod validation;
