//! Settings from `~/.claude-brain/config.json` (all optional). The settings sheet writes them
//! with [`set`], which leaves every other key of the file as it is.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};

pub fn path() -> std::path::PathBuf {
    brain_core::account::home_dir().join(".claude-brain/config.json")
}

fn read() -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path()).ok()?).ok()
}

/// The quick-terminal hotkey (`quick_terminal`), e.g. "cmd+shift+b". Off unless set: a global
/// key overrides the same combination everywhere, including macOS' own shortcuts (⌃⌥Space
/// switches the input source), so only a key the user picked is taken.
pub fn quick_terminal() -> Option<String> {
    let spec = read().and_then(|v| v.get("quick_terminal")?.as_str().map(str::to_string))?;
    (!spec.trim().is_empty() && spec.trim() != "off").then_some(spec)
}

/// The chat history's font family (`chat_font`), e.g. "JetBrains Mono"; by default the
/// terminal's.
pub fn chat_font() -> Option<String> {
    read().and_then(|v| v.get("chat_font")?.as_str().map(str::to_string)).filter(|f| !f.trim().is_empty())
}

/// The chat history's font size (`chat_font_size`) in points.
pub fn chat_font_size() -> Option<f32> {
    read().and_then(|v| v.get("chat_font_size")?.as_f64()).map(|s| s.clamp(9.0, 24.0) as f32)
}

/// Whether Brain posts a notification when a session needs you (`notifications`); on unless
/// turned off.
pub fn notifications() -> bool {
    read().and_then(|v| v.get("notifications")?.as_bool()).unwrap_or(true)
}

/// The colour each account picked (`account_colors`: account → 0…3); accounts without one take
/// the colour of their place in the list.
pub fn account_colors() -> HashMap<String, usize> {
    let Some(table) = read().and_then(|v| v.get("account_colors").cloned()) else { return HashMap::new() };
    table.as_object().map(|t| t.iter().filter_map(|(account, i)| Some((account.clone(), i.as_u64().filter(|i| *i < 4)? as usize))).collect()).unwrap_or_default()
}

/// Changes the file at `config` through `change` and leaves every other key as it is; a missing
/// file starts empty. A file that isn't a JSON object is left alone (the error says so).
pub fn update(config: &Path, change: impl FnOnce(&mut Map<String, Value>)) -> std::io::Result<()> {
    let invalid = |e: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string());
    let mut root = match std::fs::read(config) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Value::Object(Map::new()),
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|e| invalid(&e.to_string()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(e) => return Err(e),
    };
    let object = root.as_object_mut().ok_or_else(|| invalid("config.json is not an object"))?;
    change(object);
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| invalid(&e.to_string()))?;
    std::fs::write(config, text + "\n")
}

/// Sets one key of the config file, or removes it with `None`.
pub fn set(key: &str, value: Option<Value>) -> std::io::Result<()> {
    update(&path(), |object| match value {
        Some(value) => {
            object.insert(key.to_string(), value);
        }
        None => {
            object.remove(key);
        }
    })
}

/// Sets `<table>.<entry>` (a table like `account_colors`), creating the table when needed.
pub fn set_in(config: &Path, table: &str, entry: &str, value: Value) -> std::io::Result<()> {
    update(config, |object| {
        let slot = object.entry(table).or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        slot[entry] = value;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_one_key_keeps_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, "{\n}").unwrap();
        set_in(&config, "account_colors", "second", Value::from(2)).unwrap();
        update(&config, |o| {
            o.insert("quick_terminal".into(), Value::from("cmd+shift+b"));
        })
        .unwrap();
        set_in(&config, "account_colors", "main", Value::from(1)).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"quick_terminal": "cmd+shift+b", "account_colors": {"second": 2, "main": 1}}));

        std::fs::write(&config, "not json").unwrap();
        assert!(update(&config, |_| {}).is_err(), "a broken file is left alone");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "not json");
    }
}
