#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kaleidoscope::launcher::runtime::{self, Client, Snapshot};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
fn launcher_snapshot() -> Result<Snapshot, String> {
    runtime::snapshot()
}

#[tauri::command]
async fn launcher_choose_path(app: tauri::AppHandle, client: Client) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .add_filter("osu! client", &["exe"])
            .blocking_pick_file();
        match selected {
            Some(file) => runtime::save_client_path(
                client,
                &file
                    .into_path()
                    .map_err(|_| "Choose a local executable".to_owned())?,
            ),
            None => runtime::snapshot(),
        }
    })
    .await
    .map_err(|_| "Client selection could not complete".to_owned())?
}

#[tauri::command]
async fn launcher_start(client: Client) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || runtime::start(client))
        .await
        .map_err(|_| "The client launch could not complete".to_owned())?
}

#[tauri::command]
async fn launcher_stop() -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(runtime::stop)
        .await
        .map_err(|_| "The client close could not complete".to_owned())?
}

#[tauri::command]
async fn launcher_recover() -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(runtime::recover)
        .await
        .map_err(|_| "Plugin cleanup could not complete".to_owned())?
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = runtime::handle_internal(&args) {
        match result {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            launcher_snapshot,
            launcher_choose_path,
            launcher_start,
            launcher_stop,
            launcher_recover
        ])
        .run(tauri::generate_context!())
        .expect("Kaleidoscope desktop initialization failed");
}
