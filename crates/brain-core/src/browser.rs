//! Which Chrome profiles there are and which one an account uses: Brain opens links there, and
//! the account's CLAUDE.md tells Claude.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// A Chrome profile: its directory (`Profile 9`) and the name shown in Chrome (`Claude Second`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeProfile {
    pub dir: String,
    pub name: String,
}

pub const CHROME_APP: &str = "Google Chrome";

/// Where Brain keeps its settings.
pub fn config_path() -> PathBuf {
    crate::account::home_dir().join(".claude-brain/config.json")
}

fn local_state_path() -> PathBuf {
    crate::account::home_dir().join("Library/Application Support/Google/Chrome/Local State")
}

fn chrome_installed() -> bool {
    Path::new("/Applications/Google Chrome.app").exists() || crate::account::home_dir().join("Applications/Google Chrome.app").exists()
}

/// The profiles in Chrome's `Local State` (`profile.info_cache`), by directory. Empty without the
/// file or without Chrome.
pub fn read_profiles(local_state: &Path) -> Vec<ChromeProfile> {
    let Some(value) = std::fs::read(local_state).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return Vec::new() };
    let Some(cache) = value.pointer("/profile/info_cache").and_then(Value::as_object) else { return Vec::new() };
    let mut profiles: Vec<ChromeProfile> =
        cache.iter().map(|(dir, info)| ChromeProfile { dir: dir.clone(), name: info.get("name").and_then(Value::as_str).unwrap_or(dir).to_string() }).collect();
    profiles.sort_by_key(|p| natural_key(&p.dir));
    profiles
}

/// "Profile 10" after "Profile 9", "Default" first.
fn natural_key(dir: &str) -> (u64, String) {
    (dir.rsplit(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0), dir.to_string())
}

/// The profiles of the installed Chrome; none when Chrome isn't installed.
pub fn chrome_profiles() -> Vec<ChromeProfile> {
    if chrome_installed() {
        read_profiles(&local_state_path())
    } else {
        Vec::new()
    }
}

/// The profile whose name has the account's id as a word ("second" in "Claude Second"), when
/// exactly one has.
pub fn suggest_profile<'a>(account: &str, profiles: &'a [ChromeProfile]) -> Option<&'a ChromeProfile> {
    let mut named = profiles.iter().filter(|p| p.name.split(|c: char| !c.is_alphanumeric()).any(|w| w.eq_ignore_ascii_case(account)));
    let first = named.next()?;
    named.next().is_none().then_some(first)
}

/// What the config holds for the account: `None` when nothing, `Some("")` for the default browser.
pub fn stored_profile(config: &Path, account: &str) -> Option<String> {
    let value: Value = serde_json::from_slice(&std::fs::read(config).ok()?).ok()?;
    Some(value.get("chrome_profiles")?.get(account)?.as_str()?.to_string())
}

/// The profile directory an account's links open in: the stored one, or the suggestion by name
/// when nothing is stored, as long as Chrome has that profile (a profile it doesn't know would
/// make it create a new one). `""` stored means the default browser.
pub fn profile_for(config: &Path, account: &str, profiles: &[ChromeProfile]) -> Option<String> {
    match stored_profile(config, account) {
        Some(dir) => profiles.iter().find(|p| p.dir == dir).map(|p| p.dir.clone()),
        None => suggest_profile(account, profiles).map(|p| p.dir.clone()),
    }
}

/// Sets `chrome_profiles.<account>` and leaves every other key of the file as it is. `None`
/// stores `""` (default browser).
pub fn set_profile(config: &Path, account: &str, dir: Option<&str>) -> std::io::Result<()> {
    let invalid = |e: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string());
    let mut root = match std::fs::read(config) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|e| invalid(&e.to_string()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(e) => return Err(e),
    };
    let object = root.as_object_mut().ok_or_else(|| invalid("config.json is not an object"))?;
    let table = object.entry("chrome_profiles").or_insert_with(|| Value::Object(Map::new()));
    if !table.is_object() {
        *table = Value::Object(Map::new());
    }
    table[account] = Value::String(dir.unwrap_or_default().to_string());
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| invalid(&e.to_string()))?;
    std::fs::write(config, text + "\n")
}

/// The profile an account's links open in, from the real config and Chrome; `None` for the default browser.
pub fn effective_profile(account: &str) -> Option<ChromeProfile> {
    let profiles = chrome_profiles();
    let dir = profile_for(&config_path(), account, &profiles)?;
    profiles.into_iter().find(|p| p.dir == dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<ChromeProfile> {
        [("Profile 6", "fastbill.com"), ("Profile 9", "Claude Second"), ("Profile 10", "Claude Third")]
            .map(|(dir, name)| ChromeProfile { dir: dir.into(), name: name.into() })
            .into()
    }

    #[test]
    fn reads_profiles_from_local_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Local State");
        std::fs::write(&path, r#"{"profile":{"info_cache":{"Profile 10":{"name":"Claude Third"},"Default":{"name":"Martin"},"Profile 9":{"name":"Claude Second"}}},"other":1}"#).unwrap();
        let read = read_profiles(&path);
        assert_eq!(read.iter().map(|p| (p.dir.as_str(), p.name.as_str())).collect::<Vec<_>>(), [("Default", "Martin"), ("Profile 9", "Claude Second"), ("Profile 10", "Claude Third")]);
        assert!(read_profiles(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn suggests_by_name_only_when_unique() {
        let p = profiles();
        assert_eq!(suggest_profile("second", &p).unwrap().dir, "Profile 9");
        assert_eq!(suggest_profile("third", &p).unwrap().dir, "Profile 10");
        assert!(suggest_profile("main", &p).is_none());
        let mut twice = p;
        twice.push(ChromeProfile { dir: "Profile 11".into(), name: "Second Test".into() });
        assert!(suggest_profile("second", &twice).is_none());
    }

    #[test]
    fn stored_profile_wins_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let p = profiles();
        assert_eq!(profile_for(&config, "second", &p).as_deref(), Some("Profile 9"));
        set_profile(&config, "main", Some("Profile 6")).unwrap();
        set_profile(&config, "second", None).unwrap();
        set_profile(&config, "third", Some("Profile 99")).unwrap();
        assert_eq!(profile_for(&config, "main", &p).as_deref(), Some("Profile 6"));
        assert_eq!(profile_for(&config, "second", &p), None);
        assert_eq!(profile_for(&config, "third", &p), None);
    }

    #[test]
    fn setting_a_profile_keeps_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, r#"{"language":"de","chat_font_size":14,"chrome_profiles":{"third":"Profile 10"}}"#).unwrap();
        set_profile(&config, "second", Some("Profile 9")).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        assert_eq!(value["language"], "de");
        assert_eq!(value["chat_font_size"], 14);
        assert_eq!(value["chrome_profiles"]["third"], "Profile 10");
        assert_eq!(value["chrome_profiles"]["second"], "Profile 9");
    }
}
