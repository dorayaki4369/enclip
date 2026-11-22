//! enclip の Windows 常駐アプリ（PoC）。
//!
//! コピーを自動で送り、届いたテキストを条件付きでクリップボードへ入れる。
//! 画面は端末の追加（QR コードと確認コード）と履歴の表示だけを持つ。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard_worker;
mod engine;
mod platform;
mod storage;

use qrcode::QrCode;
use qrcode::render::svg;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State, WindowEvent};

use engine::{Engine, StatusView};
use storage::Storage;

#[tauri::command]
async fn status(engine: State<'_, Engine>) -> Result<StatusView, String> {
    Ok(engine.status().await)
}

#[tauri::command]
async fn set_paused(engine: State<'_, Engine>, paused: bool) -> Result<(), String> {
    engine.set_paused(paused).await;
    Ok(())
}

#[tauri::command]
async fn copy_history(engine: State<'_, Engine>, index: usize) -> Result<(), String> {
    engine.copy_history(index).await
}

/// 端末の追加を始め、QR コードの SVG を返す。
#[tauri::command]
async fn start_invite(engine: State<'_, Engine>) -> Result<String, String> {
    let text = engine.start_invite().await?;
    let code = QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<svg::Color<'_>>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .build())
}

#[tauri::command]
async fn approve_invite(engine: State<'_, Engine>) -> Result<(), String> {
    engine.approve_invite().await
}

#[tauri::command]
async fn cancel_invite(engine: State<'_, Engine>) -> Result<(), String> {
    engine.cancel_invite().await;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let storage = Storage::new(app.path().app_data_dir()?)?;
            let (clipboard, mut copies) = clipboard_worker::spawn();
            let engine = Engine::load(app.handle().clone(), storage, clipboard)?;
            app.manage(engine.clone());

            tauri::async_runtime::spawn(engine.clone().run_sync());
            let copy_engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(text) = copies.recv().await {
                    copy_engine.on_local_copy(text).await;
                }
            });

            build_tray(app, engine)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 閉じるボタンでは終了せず、タスクトレイに残る。
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                // 画面を閉じたらペアリングも終える（仕様 4.1 の 1）。
                let engine = window.state::<Engine>().inner().clone();
                let window = window.clone();
                tauri::async_runtime::spawn(async move {
                    engine.cancel_invite().await;
                    let _ = window.emit("pairing-closed", ());
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            set_paused,
            copy_history,
            start_invite,
            approve_invite,
            cancel_invite
        ])
        .run(tauri::generate_context!())
        .expect("failed to run enclip");
}

fn build_tray(app: &tauri::App, engine: Engine) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "開く", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "一時停止 / 再開", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &quit])?;
    let mut tray = TrayIconBuilder::new().menu(&menu).tooltip("enclip");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(move |app, event| match event.id.as_ref() {
        "open" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "pause" => {
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                let paused = engine.is_paused().await;
                engine.set_paused(!paused).await;
            });
        }
        "quit" => app.exit(0),
        _ => {}
    })
    .build(app)?;
    Ok(())
}
