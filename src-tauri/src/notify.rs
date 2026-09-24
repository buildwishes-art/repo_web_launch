//! Toast notification Windows (dikirim langsung dari Rust, jadi tetap muncul walaupun
//! jendela tersembunyi di tray).
//!
//! AppUserModelID: build release memakai `identifier` dari tauri.conf.json (didaftarkan
//! installer NSIS/MSI lewat shortcut Start Menu). Build debug memakai AUMID PowerShell
//! karena exe dev belum terdaftar — toast akan tampil atas nama "Windows PowerShell".

use tauri::AppHandle;
use tauri_winrt_notification::{Duration, Sound, Toast};

use crate::{
    engine::{state, types::CoreEvent},
    tray,
};

const ACTION_KILL: &str = "kill";
const ACTION_KEEP: &str = "keep";

fn app_id(app: &AppHandle) -> String {
    if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    }
}

/// "Penggunaan resource untuk repo X terlalu tinggi. Apakah Anda ingin menghentikannya?"
/// dengan tombol [Ya (Hentikan)] [Tidak]. Klik badan toast → buka jendela RepoLaunch.
pub fn resource_alert(app: &AppHandle, ev: &CoreEvent) {
    let handle = app.clone();
    let repo_id = ev.repo_id.clone();
    let result = Toast::new(&app_id(app))
        .title(&format!("Resource tinggi: {}", ev.repo_name))
        .text1(&format!(
            "Penggunaan resource untuk repo {} terlalu tinggi. Apakah Anda ingin menghentikannya?",
            ev.repo_name
        ))
        .text2(ev.message.as_deref().unwrap_or_default())
        .duration(Duration::Long)
        .sound(Some(Sound::Default))
        .add_button("Ya (Hentikan)", ACTION_KILL)
        .add_button("Tidak", ACTION_KEEP)
        .on_activated(move |action| {
            match action.as_deref() {
                Some(a @ (ACTION_KILL | ACTION_KEEP)) => {
                    let kill = a == ACTION_KILL;
                    let id = repo_id.clone();
                    // Callback berjalan di thread WinRT; stop bisa lama → pindahkan.
                    std::thread::spawn(move || {
                        if let Ok(engine) = state::get() {
                            let _ = engine.respond_alert(&id, kill);
                        }
                    });
                }
                _ => tray::show_main(&handle),
            }
            Ok(())
        })
        .show();
    if let Err(e) = result {
        eprintln!("[repolaunch] gagal menampilkan toast: {e:?}");
    }
}

pub fn info(app: &AppHandle, title: &str, body: &str) {
    let handle = app.clone();
    let result = Toast::new(&app_id(app))
        .title(title)
        .text1(body)
        .sound(None)
        .on_activated(move |_| {
            tray::show_main(&handle);
            Ok(())
        })
        .show();
    if let Err(e) = result {
        eprintln!("[repolaunch] gagal menampilkan toast: {e:?}");
    }
}
