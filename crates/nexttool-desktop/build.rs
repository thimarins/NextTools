// The app manifest turns every custom command into an explicit permission
// (`allow-<command>`), so the webview can only call what
// capabilities/default.json lists. See ADR-018.
include!("src/command_list.rs");

fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("tauri build");
}
