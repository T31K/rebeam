// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn credential_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rebeam")
        .join("device-token")
}

#[tauri::command]
fn load_device_token() -> Option<String> {
    std::fs::read_to_string(credential_path()).ok().map(|token| token.trim().to_string())
}

#[tauri::command]
fn save_device_token(token: String) -> Result<(), String> {
    let path = credential_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    std::fs::write(path, token).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, load_device_token, save_device_token])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
