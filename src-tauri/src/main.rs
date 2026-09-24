//! RepoLaunch — entry point.
//!
//! Satu executable, dua mode:
//! * normal                              → aplikasi Tauri (UI + tray + core)
//! * `--serve-static <root> <port>`      → server HTML statis (dijalankan core sebagai proses anak
//!                                         di dalam Job Object untuk repo HTML statis)

// Tanpa jendela konsol di build release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("RepoLaunch hanya mendukung Windows (Job Object, toast WinRT, venv Scripts\\python.exe).");

mod commands;
mod engine;
mod notify;
mod tray;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::engine::{
    sandbox::STATIC_SERVER_FLAG,
    state::{self, EventSink},
    types::{CoreEvent, EventKind, RepoStatus},
};

/// Nama event Tauri yang didengarkan frontend (`src/lib/store.svelte.ts`).
pub const CORE_EVENT: &str = "core-event";

/// AppUserModelID proses. HARUS sama dengan `identifier` di tauri.conf.json — installer memberi
/// AUMID ini ke shortcut Start Menu, jadi taskbar memakai nama & ikon "RepoLaunch" dari shortcut
/// (dan toast release juga memakai ID ini).
const APP_USER_MODEL_ID: &str = "com.repolaunch.app";

/// Harus dipanggil sebelum jendela pertama dibuat, agar taskbar mengelompokkan jendela ke AUMID ini.
fn set_app_user_model_id() {
    let wide: Vec<u16> = APP_USER_MODEL_ID.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: pointer ke string UTF-16 ber-null-terminator yang hidup selama pemanggilan.
    unsafe {
        windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(STATIC_SERVER_FLAG) {
        if let Err(e) = engine::static_server::run_from_args(&args[2..]) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }

    set_app_user_model_id();

    tauri::Builder::default()
        // Harus plugin pertama: instance kedua cukup memunculkan jendela yang sudah ada.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            debug_assert_eq!(app.config().identifier, APP_USER_MODEL_ID, "samakan APP_USER_MODEL_ID dengan tauri.conf.json");
            if let Some(window) = app.get_webview_window("main") {
                apply_window_icons(&window);
            }

            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let sink: EventSink = Arc::new(move |ev: &CoreEvent| on_core_event(&handle, ev));
            state::init(&data_dir, sink)?;
            tray::create(app)?;
            Ok(())
        })
        .on_window_event(tray::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::list_repos,
            commands::add_repo_from_git,
            commands::add_repo_from_archive,
            commands::delete_repo,
            commands::validate_git_url,
            commands::start_repo,
            commands::stop_repo,
            commands::get_logs,
            commands::open_repo_url,
            commands::respond_to_alert,
            commands::get_settings,
            commands::update_settings,
            commands::clean_cache,
            commands::cpu_count,
            commands::get_scan_report,
            commands::rescan_repo,
        ])
        .run(tauri::generate_context!())
        .expect("gagal menjalankan RepoLaunch");
}

/// Memasang ikon aplikasi ke jendela: ICON_BIG (taskbar, Alt+Tab) dan ICON_SMALL (title bar).
/// `WebviewWindow::set_icon` Tauri hanya mengisi ICON_SMALL, sehingga tanpa ini taskbar
/// menampilkan ikon default Windows. Ikon diambil dari resource .exe (icons/icon.ico).
fn apply_window_icons(window: &tauri::WebviewWindow) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{
        Shell::ExtractIconExW,
        WindowsAndMessaging::{SendMessageW, HICON, ICON_BIG, ICON_SMALL, WM_SETICON},
    };

    let (Ok(exe), Ok(hwnd)) = (std::env::current_exe(), window.hwnd()) else { return };
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    let mut large: HICON = std::ptr::null_mut();
    let mut small: HICON = std::ptr::null_mut();
    // SAFETY: path ber-null-terminator; handle ikon dimiliki jendela selama proses hidup.
    unsafe {
        if ExtractIconExW(path.as_ptr(), 0, &mut large, &mut small, 1) == 0 {
            return;
        }
        if !large.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, large as isize);
        }
        if !small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
    }
}

/// Semua event core lewat sini: diteruskan ke UI, lalu efek samping yang harus tetap jalan
/// walaupun jendela tersembunyi di tray (toast, tooltip tray) dikerjakan langsung di Rust.
fn on_core_event(app: &AppHandle, ev: &CoreEvent) {
    let _ = app.emit(CORE_EVENT, ev);
    match ev.kind {
        EventKind::ResourceAlert => notify::resource_alert(app, ev),
        EventKind::AutoKilled => notify::info(app, "Repo dihentikan otomatis", ev.message.as_deref().unwrap_or(&ev.repo_name)),
        EventKind::IdleStopped => notify::info(app, "Repo idle dihentikan", ev.message.as_deref().unwrap_or(&ev.repo_name)),
        EventKind::StatusChanged => {
            tray::refresh(app);
            if ev.status == Some(RepoStatus::Crashed) && !tray::main_visible(app) {
                notify::info(app, &format!("{} berhenti", ev.repo_name), ev.message.as_deref().unwrap_or("Proses crash."));
            }
        }
        _ => {}
    }
}
