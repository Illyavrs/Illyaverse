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

fn html_attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for (offset, _) in tag.match_indices(name) {
        if offset == 0 || !tag.as_bytes()[offset - 1].is_ascii_whitespace() {
            continue;
        }
        let after_name = tag[offset + name.len()..].trim_start();
        let Some(value) = after_name.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let quote = value.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &value[quote.len_utf8()..];
        return value.find(quote).map(|end| &value[..end]);
    }
    None
}

fn verify_embedded_interface() -> Result<(), String> {
    if tauri::is_dev() {
        return Err("The launcher was compiled in Tauri development mode".to_owned());
    }
    let context: tauri::Context<tauri::Wry> = tauri::tauri_build_context!();
    if !matches!(
        context.config().build.frontend_dist,
        Some(tauri::utils::config::FrontendDist::Directory(_))
    ) {
        return Err("The launcher must embed a local frontend directory".to_owned());
    }
    let assets = context.assets();
    let index = assets
        .get(&tauri::utils::assets::AssetKey::from("index.html"))
        .ok_or_else(|| "The embedded interface has no index.html".to_owned())?;
    let html = std::str::from_utf8(&index)
        .map_err(|_| "The embedded index.html is not UTF-8".to_owned())?;
    if !html.contains("<body") || !html.contains("</body>") {
        return Err("The embedded index.html has no body".to_owned());
    }
    let mut counts = [0_usize; 2];
    // Tauri serializes the trusted build output with quoted HTML attributes.
    for tag in html.split('<').filter_map(|part| part.split_once('>')) {
        let tag = tag.0;
        let (kind, attribute) = if tag.starts_with("script ") {
            (0, "src")
        } else if tag.starts_with("link ") && html_attribute(tag, "rel") == Some("stylesheet") {
            (1, "href")
        } else {
            continue;
        };
        let Some(reference) = html_attribute(tag, attribute) else {
            continue;
        };
        let path = reference.split(['?', '#']).next().unwrap_or_default();
        if path.is_empty() || path.starts_with("//") || path.contains(':') {
            return Err(format!("The interface asset must be local: {reference}"));
        }
        let content = assets
            .get(&tauri::utils::assets::AssetKey::from(path))
            .ok_or_else(|| format!("The embedded interface is missing {path}"))?;
        if content.is_empty() {
            return Err(format!("The embedded interface asset is empty: {path}"));
        }
        counts[kind] += 1;
    }
    if counts.contains(&0) {
        return Err("The embedded interface must include JavaScript and CSS".to_owned());
    }
    let asset_count = assets.iter().count();
    println!(
        "Production interface verified: {asset_count} embedded assets, {} script references, {} stylesheet references",
        counts[0], counts[1]
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // This proof must exit before launcher commands, user-data access, or GUI initialization.
    if args.iter().any(|arg| arg == "--verify-embedded-interface") {
        let result = if args.len() == 1 {
            verify_embedded_interface()
        } else {
            Err("Use --verify-embedded-interface without other arguments".to_owned())
        };
        match result {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
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
        .run(tauri::tauri_build_context!())
        .expect("Kaleidoscope desktop initialization failed");
}
