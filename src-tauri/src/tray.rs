//! System tray & minimize-to-tray.
//!
//! * Minimize (−)  → minimize biasa; jendela TETAP terlihat di taskbar (default).
//!                   Opsional `minimize_to_tray` untuk menyembunyikannya ke tray.
//! * Close (X)     → disembunyikan ke tray (jika `close_to_tray`); repo tetap berjalan & dipantau.
//! * Klik kiri ikon tray → jendela muncul kembali. Klik kanan → menu:
//!   Buka RepoLaunch · Hentikan semua repo · Keluar.
//! * Tooltip ikon menampilkan jumlah repo yang sedang berjalan.
//!
//! Monitor resource & toast ada di Rust, jadi tetap berjalan saat jendela tersembunyi.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, Window, WindowEvent,
};

use crate::{engine::state, notify};

const TRAY_ID: &str = "main";
const MAIN_WINDOW: &str = "main";

/// Toast "RepoLaunch tetap berjalan di tray" hanya ditampilkan sekali per sesi.
static TRAY_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

pub fn create(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Buka RepoLaunch", true, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, "stop_all", "Hentikan semua repo", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &stop_all, &separator, &quit])?;

    let icon = app.default_window_icon().cloned().expect("ikon aplikasi ada di bundle");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("RepoLaunch — tidak ada repo berjalan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "stop_all" => {
                std::thread::spawn(|| {
                    if let Ok(engine) = state::get() {
                        engine.stop_all("Dihentikan dari menu tray.");
                    }
                });
            }
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn main_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// Memperbarui tooltip tray dengan jumlah repo aktif.
pub fn refresh(app: &AppHandle) {
    let count = state::get().map_or(0, |e| e.running_count());
    let text = match count {
        0 => "RepoLaunch — tidak ada repo berjalan".to_string(),
        n => format!("RepoLaunch — {n} repo berjalan"),
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let settings = state::get().map(|e| e.settings()).unwrap_or_default();
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            if settings.close_to_tray {
                api.prevent_close();
                hide_to_tray(window);
            } else {
                // Keluar sungguhan: hentikan semua repo dengan rapi dulu.
                api.prevent_close();
                quit_app(window.app_handle());
            }
        }
        // Tauri tidak punya event "Minimized"; cek status setelah resize.
        WindowEvent::Resized(_) if settings.minimize_to_tray && window.is_minimized().unwrap_or(false) => {
            hide_to_tray(window);
        }
        _ => {}
    }
}

fn hide_to_tray(window: &Window) {
    let _ = window.hide();
    if !TRAY_HINT_SHOWN.swap(true, Ordering::SeqCst) {
        notify::info(
            window.app_handle(),
            "RepoLaunch tetap berjalan",
            "Aplikasi disembunyikan ke system tray. Repo yang aktif tetap berjalan dan dipantau. \
             Klik ikon tray untuk membuka kembali.",
        );
    }
}

/// Menghentikan semua repo lalu keluar. Dijalankan di thread terpisah agar UI/tray tidak beku.
pub fn quit_app(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Ok(engine) = state::get() {
            engine.stop_all("Aplikasi ditutup.");
        }
        app.exit(0);
    });
}
