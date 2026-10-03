//! macOS integrations: notifications. Terminal control lives in `terminal`.

use std::process::Command;

pub fn notify(title: &str, subtitle: &str, body: &str) {
    let script = format!(
        "display notification {} with title {} subtitle {} sound name \"Glass\"",
        applescript_string(body),
        applescript_string(title),
        applescript_string(subtitle),
    );
    let _ = Command::new("osascript").args(["-e", &script]).spawn();
}

fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
