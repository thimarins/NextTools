// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(test)]
mod command_list;
mod commands;
mod ipc;
mod pump;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};

fn main() {
    let data_dir = nexttool_store::default_data_dir();
    if let Err(e) = nexttool_store::init_logging(&data_dir.join("logs"), "warn") {
        eprintln!("log: {e}");
    }
    let (core, recovered) = match ipc::Core::open(data_dir) {
        Ok(v) => v,
        Err(e) => {
            // Without the state store nothing can run safely: show and exit.
            tracing::error!("cannot open app store: {e}");
            eprintln!("NextTool não pôde abrir o estado do aplicativo: {e}");
            std::process::exit(2);
        }
    };
    if !recovered.is_empty() {
        tracing::warn!("{} task(s) recovered as interrupted", recovered.len());
    }
    let core = Arc::new(core);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second instance would run a second scheduler: focus the first.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(core.clone())
        .setup(move |app| {
            pump::start(app.handle().clone(), core.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let core = window.state::<Arc<ipc::Core>>();
                let running = core.mgr.running_ids().len();
                if running > 0 {
                    // Ask the UI to confirm; tasks are paused safely on quit.
                    api.prevent_close();
                    let _ = window.emit("app://close-requested", running);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::app_quit,
            commands::overview,
            commands::tasks_list,
            commands::task_get,
            commands::task_pause,
            commands::task_resume,
            commands::task_cancel,
            commands::size_scan_submit,
            commands::snapshot_summary,
            commands::snapshot_list_dir,
            commands::snapshot_top_files,
            commands::snapshot_top_dirs,
            commands::snapshot_query,
            commands::snapshot_cancel_query,
            commands::snapshot_stats,
            commands::copy_plan,
            commands::copy_submit,
            commands::alerts_list,
            commands::alerts_ack,
            commands::settings_get,
            commands::settings_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NextTool");
}
