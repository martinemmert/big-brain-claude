//! A new Claude account made from the existing ones: their setup and their memory in one new
//! config folder (`~/.claude-<name>`), ready for `/login` with another Claude account.
//!
//! What comes along: `settings.json` (merged), `CLAUDE.md`, the status line script, skills,
//! agents, commands and output styles, plugins with their marketplaces, user MCP servers, and
//! every project's memory notes. Where the accounts differ, the first (main) wins, except for
//! memory notes, where the newer one does (memory moves on); [`plan`] names each such case
//! before [`create`] writes anything.
//!
//! What never comes along: the login (`oauthAccount`, the keychain), conversations and their
//! history, jobs, caches and telemetry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::account::Account;

/// Folders whose entries are copied by name: a skill, an agent, a command, an output style.
const ENTRY_DIRS: [&str; 4] = ["skills", "agents", "commands", "output-styles"];
/// Single files where the first account's copy wins.
const PLAIN_FILES: [&str; 3] = ["CLAUDE.md", "statusline-command.sh", "keybindings.json"];
/// Plugin folders copied as trees (the first account's files win).
const PLUGIN_TREES: [&str; 3] = ["marketplaces", "cache", "data"];

/// What the new account gets, item by item.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub target: PathBuf,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// What it is, as a path in the new folder (`skills/fux`, `settings.json`, `memory: …`).
    pub what: String,
    /// The accounts it comes from, first one first.
    pub from: Vec<String>,
    /// Where the accounts disagree and the first one's version was kept.
    pub conflicts: Vec<String>,
}

impl Plan {
    pub fn conflicts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.items.iter().flat_map(|i| i.conflicts.iter().map(move |c| (i.what.as_str(), c.as_str())))
    }
}

/// The folder for an account name, or why the name can't be one.
pub fn target_dir(home: &Path, name: &str) -> Result<PathBuf, String> {
    let valid = !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid || name == "main" || name.starts_with('-') {
        return Err(format!("“{name}” can't name an account: lowercase letters, digits and hyphens, not “main”"));
    }
    let dir = home.join(format!(".claude-{name}"));
    if dir.exists() {
        return Err(format!("{} exists already", dir.display()));
    }
    Ok(dir)
}

/// What [`create`] would do, without writing anything.
pub fn plan(sources: &[Account], home: &Path, target: &Path) -> Plan {
    let mut items = Vec::new();
    run(sources, home, target, false, &mut items).ok();
    Plan { target: target.to_path_buf(), items }
}

/// Makes the new account folder from `sources` (the first one wins on differences) and returns
/// what went into it. The folder must not exist yet.
pub fn create(sources: &[Account], home: &Path, target: &Path) -> Result<Plan, String> {
    if target.exists() {
        return Err(format!("{} exists already", target.display()));
    }
    let mut items = Vec::new();
    std::fs::create_dir_all(target.join("sessions")).map_err(|e| e.to_string())?;
    run(sources, home, target, true, &mut items).map_err(|e| e.to_string())?;
    Ok(Plan { target: target.to_path_buf(), items })
}

fn run(sources: &[Account], home: &Path, target: &Path, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    settings(sources, target, write, items)?;
    for name in PLAIN_FILES {
        plain_file(sources, target, name, write, items)?;
    }
    for dir in ENTRY_DIRS {
        entries(sources, target, dir, write, items)?;
    }
    plugins(sources, target, write, items)?;
    mcp_servers(sources, home, target, write, items)?;
    memory(sources, target, write, items)?;
    Ok(())
}

// ---- settings -------------------------------------------------------------------------------

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

fn settings(sources: &[Account], target: &Path, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let mut merged: Option<Value> = None;
    let mut from = Vec::new();
    let mut conflicts = Vec::new();
    for source in sources {
        let Some(value) = read_json(&source.config_dir.join("settings.json")) else { continue };
        from.push(source.id.clone());
        match merged.as_mut() {
            None => merged = Some(value),
            Some(merged) => merge(merged, &value, "", &source.id, &mut conflicts),
        }
    }
    let Some(merged) = merged else { return Ok(()) };
    if write {
        write_json(&target.join("settings.json"), &merged)?;
    }
    items.push(Item { what: "settings.json".into(), from, conflicts });
    Ok(())
}

/// Adds what `other` has and `into` lacks: keys missing in an object, items missing in a list
/// (rules, hooks, plugins). Where both have a different plain value, `into` keeps its own and
/// the place is noted.
pub fn merge(into: &mut Value, other: &Value, path: &str, other_name: &str, conflicts: &mut Vec<String>) {
    match (into, other) {
        (Value::Object(mine), Value::Object(theirs)) => {
            for (key, value) in theirs {
                let at = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match mine.get_mut(key) {
                    Some(existing) => merge(existing, value, &at, other_name, conflicts),
                    None => {
                        mine.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(mine), Value::Array(theirs)) => {
            for value in theirs {
                if !mine.contains(value) {
                    mine.push(value.clone());
                }
            }
        }
        (mine, theirs) if mine != theirs => conflicts.push(format!("{path}: {other_name} has {}, kept the first", short(theirs))),
        _ => {}
    }
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(60) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    }
}

// ---- files and folders ----------------------------------------------------------------------

fn plain_file(sources: &[Account], target: &Path, name: &str, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let copies: Vec<(&Account, Vec<u8>)> = sources.iter().filter_map(|s| std::fs::read(s.config_dir.join(name)).ok().map(|b| (s, b))).collect();
    let Some((first, content)) = copies.first() else { return Ok(()) };
    let conflicts = copies[1..].iter().filter(|(_, other)| other != content).map(|(s, _)| format!("{}'s differs, kept the first", s.id)).collect();
    if write {
        let dst = target.join(name);
        std::fs::write(&dst, content)?;
        copy_permissions(&first.config_dir.join(name), &dst)?;
    }
    items.push(Item { what: name.into(), from: copies.iter().map(|(s, _)| s.id.clone()).collect(), conflicts });
    Ok(())
}

/// Every entry of `dir` in any account, by name; the first account's wins.
fn entries(sources: &[Account], target: &Path, dir: &str, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let mut found: BTreeMap<String, Vec<(&Account, PathBuf)>> = BTreeMap::new();
    for source in sources {
        for entry in std::fs::read_dir(source.config_dir.join(dir)).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            found.entry(name).or_default().push((source, entry.path()));
        }
    }
    for (name, copies) in found {
        let path = &copies[0].1;
        let conflicts = copies[1..].iter().filter(|(_, other)| !same_tree(path, other)).map(|(s, _)| format!("{}'s differs, kept the first", s.id)).collect();
        if write {
            copy_tree(path, &target.join(dir).join(&name))?;
        }
        items.push(Item { what: format!("{dir}/{name}"), from: copies.iter().map(|(s, _)| s.id.clone()).collect(), conflicts });
    }
    Ok(())
}

/// Copies a file or folder; on APFS the files are clones that take no extra space. Files
/// already there stay (so the first account's win when trees are layered).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        if dst.symlink_metadata().is_err() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(std::fs::read_link(src)?, dst)?;
        }
    } else if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if !dst.exists() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        copy_file(src, dst)?;
    }
    Ok(())
}

/// A clone where the file system makes them (APFS: no extra space, the plugins alone are
/// hundreds of MB), else a copy.
fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let (Ok(from), Ok(to)) = (std::ffi::CString::new(src.as_os_str().as_bytes()), std::ffi::CString::new(dst.as_os_str().as_bytes())) else {
            return std::fs::copy(src, dst).map(|_| ());
        };
        // SAFETY: both are valid NUL-terminated paths that live through the call.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    std::fs::copy(src, dst).map(|_| ())
}

fn copy_permissions(src: &Path, dst: &Path) -> std::io::Result<()> {
    let permissions = std::fs::metadata(src)?.permissions();
    std::fs::set_permissions(dst, permissions)
}

/// Whether two files or folders hold the same (folders: same names, same file contents).
fn same_tree(a: &Path, b: &Path) -> bool {
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.is_dir() && mb.is_dir() => {
            let names = |p: &Path| -> Vec<std::ffi::OsString> {
                let mut n: Vec<_> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.file_name()).collect();
                n.sort();
                n
            };
            let (na, nb) = (names(a), names(b));
            na == nb && na.iter().all(|n| same_tree(&a.join(n), &b.join(n)))
        }
        (Ok(ma), Ok(mb)) if ma.is_file() && mb.is_file() => ma.len() == mb.len() && std::fs::read(a).ok() == std::fs::read(b).ok(),
        (Ok(ma), Ok(mb)) if ma.file_type().is_symlink() && mb.file_type().is_symlink() => std::fs::read_link(a).ok() == std::fs::read_link(b).ok(),
        _ => false,
    }
}

// ---- plugins --------------------------------------------------------------------------------

/// The plugin and marketplace lists merged, their paths pointed at the new folder, and the
/// plugins' files copied along.
fn plugins(sources: &[Account], target: &Path, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let target_plugins = format!("{}/plugins/", target.display());
    for file in ["installed_plugins.json", "known_marketplaces.json"] {
        let mut merged: Option<Value> = None;
        let mut from = Vec::new();
        for source in sources {
            let Some(mut value) = read_json(&source.config_dir.join("plugins").join(file)) else { continue };
            from.push(source.id.clone());
            // Paths into any account's plugin folder (one account's list may point into
            // another's) now point into the new one.
            for account in sources {
                rewrite_paths(&mut value, &format!("{}/plugins/", account.config_dir.display()), &target_plugins);
            }
            // Differing install times and versions are each account's bookkeeping, not a choice:
            // they aren't listed as conflicts.
            match merged.as_mut() {
                None => merged = Some(value),
                Some(merged) => merge(merged, &value, "", &source.id, &mut Vec::new()),
            }
        }
        let Some(merged) = merged else { continue };
        if write {
            write_json(&target.join("plugins").join(file), &merged)?;
        }
        items.push(Item { what: format!("plugins/{file}"), from, conflicts: Vec::new() });
    }
    let mut from = Vec::new();
    for source in sources {
        let mut copied = false;
        for tree in PLUGIN_TREES {
            let src = source.config_dir.join("plugins").join(tree);
            for entry in std::fs::read_dir(&src).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Leftovers of interrupted installs.
                if name.starts_with('.') || name.starts_with("temp_git_") {
                    continue;
                }
                copied = true;
                if write {
                    copy_tree(&entry.path(), &target.join("plugins").join(tree).join(&name))?;
                }
            }
        }
        if copied {
            from.push(source.id.clone());
        }
    }
    if !from.is_empty() {
        items.push(Item { what: "plugins/ (files of the installed plugins)".into(), from, conflicts: Vec::new() });
    }
    Ok(())
}

fn rewrite_paths(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::String(text) if text.starts_with(from) => *text = format!("{to}{}", &text[from.len()..]),
        Value::Array(list) => list.iter_mut().for_each(|v| rewrite_paths(v, from, to)),
        Value::Object(map) => map.values_mut().for_each(|v| rewrite_paths(v, from, to)),
        _ => {}
    }
}

// ---- MCP servers ----------------------------------------------------------------------------

/// Where Claude Code keeps an account's global state: `~/.claude.json` for the default folder,
/// `.claude.json` inside the folder for any other.
fn global_json(account: &Account, home: &Path) -> PathBuf {
    if account.config_dir == home.join(".claude") {
        home.join(".claude.json")
    } else {
        account.config_dir.join(".claude.json")
    }
}

/// The user MCP servers only: the rest of that file is the login, caches and per-project state.
fn mcp_servers(sources: &[Account], home: &Path, target: &Path, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let mut servers = Value::Object(Map::new());
    let mut from = Vec::new();
    let mut conflicts = Vec::new();
    for source in sources {
        let Some(found) = read_json(&global_json(source, home)).and_then(|v| v.get("mcpServers").cloned()) else { continue };
        if found.as_object().is_none_or(|m| m.is_empty()) {
            continue;
        }
        from.push(source.id.clone());
        merge(&mut servers, &found, "mcpServers", &source.id, &mut conflicts);
    }
    let Some(map) = servers.as_object().filter(|m| !m.is_empty()) else { return Ok(()) };
    if write {
        write_json(&target.join(".claude.json"), &serde_json::json!({ "mcpServers": map }))?;
    }
    let names: Vec<&str> = map.keys().map(String::as_str).collect();
    items.push(Item { what: format!("MCP servers: {}", names.join(", ")), from, conflicts });
    Ok(())
}

// ---- memory ---------------------------------------------------------------------------------

/// Every project's memory notes: files by name, the newer one where both accounts have it
/// (memory moves on, the older copy is the stale one); `MEMORY.md` merged line by line, one
/// line per note, so both accounts' index entries stay.
fn memory(sources: &[Account], target: &Path, write: bool, items: &mut Vec<Item>) -> std::io::Result<()> {
    let mut projects: BTreeMap<String, Vec<&Account>> = BTreeMap::new();
    for source in sources {
        for entry in std::fs::read_dir(source.config_dir.join("projects")).into_iter().flatten().flatten() {
            let memory = entry.path().join("memory");
            let has_notes = std::fs::read_dir(&memory).into_iter().flatten().flatten().next().is_some();
            if has_notes {
                projects.entry(entry.file_name().to_string_lossy().into_owned()).or_default().push(source);
            }
        }
    }
    for (project, accounts) in projects {
        let dst = target.join("projects").join(&project).join("memory");
        let mut conflicts = Vec::new();
        let mut index: Vec<String> = Vec::new();
        // Per note: the copy that goes in, and which account it comes from.
        let mut notes: BTreeMap<String, (PathBuf, String)> = BTreeMap::new();
        for account in &accounts {
            let src = account.config_dir.join("projects").join(&project).join("memory");
            for entry in std::fs::read_dir(&src).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == "MEMORY.md" {
                    for line in std::fs::read_to_string(entry.path()).unwrap_or_default().lines() {
                        let note = linked_note(line);
                        let known = index.iter().any(|l| l == line || (note.is_some() && linked_note(l) == note));
                        if line.trim().is_empty() || !known {
                            index.push(line.to_string());
                        }
                    }
                    continue;
                }
                match notes.get(&name) {
                    Some((kept, _)) if same_tree(kept, &entry.path()) => {}
                    Some((kept, kept_from)) => {
                        let newer = modified(&entry.path()) > modified(kept);
                        let (winner, loser) = if newer { (account.id.clone(), kept_from.clone()) } else { (kept_from.clone(), account.id.clone()) };
                        conflicts.push(format!("{name}: {winner}'s is newer, {loser}'s left out"));
                        if newer {
                            notes.insert(name, (entry.path(), account.id.clone()));
                        }
                    }
                    None => {
                        notes.insert(name, (entry.path(), account.id.clone()));
                    }
                }
            }
        }
        if write {
            for (name, (path, _)) in &notes {
                copy_tree(path, &dst.join(name))?;
            }
        }
        if write && !index.is_empty() {
            std::fs::create_dir_all(&dst)?;
            let mut text = index.join("\n");
            text.push('\n');
            std::fs::write(dst.join("MEMORY.md"), text)?;
        }
        items.push(Item { what: format!("memory: {}", readable_project(&project)), from: accounts.iter().map(|a| a.id.clone()).collect(), conflicts });
    }
    Ok(())
}

/// The note a `MEMORY.md` line links to: `- [Title](note.md) — hook` → `note.md`.
fn linked_note(line: &str) -> Option<&str> {
    let start = line.find("](")? + 2;
    let end = start + line[start..].find(')')?;
    Some(&line[start..end])
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `-Users-me-Work-app` → `Work-app` (Claude Code's folder name for a project path).
fn readable_project(folder: &str) -> String {
    let home = crate::account::home_dir().display().to_string().replace('/', "-");
    folder.strip_prefix(&home).map(|rest| rest.trim_start_matches('-').to_string()).filter(|r| !r.is_empty()).unwrap_or_else(|| folder.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn account(home: &Path, id: &str) -> Account {
        let dir = if id == "main" { home.join(".claude") } else { home.join(format!(".claude-{id}")) };
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        Account::from_config_dir(dir)
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn settings_lists_are_joined_and_main_keeps_its_values() {
        let mut mine = json!({ "model": "opus", "permissions": { "allow": ["Bash(git:*)"] }, "enabledPlugins": { "a@x": true } });
        let theirs = json!({ "model": "sonnet", "permissions": { "allow": ["Bash(git:*)", "WebSearch"], "defaultMode": "auto" }, "enabledPlugins": { "b@x": true } });
        let mut conflicts = Vec::new();
        merge(&mut mine, &theirs, "", "second", &mut conflicts);
        assert_eq!(
            mine,
            json!({ "model": "opus", "permissions": { "allow": ["Bash(git:*)", "WebSearch"], "defaultMode": "auto" }, "enabledPlugins": { "a@x": true, "b@x": true } })
        );
        assert_eq!(conflicts, vec!["model: second has \"sonnet\", kept the first".to_string()]);
    }

    #[test]
    fn names_must_be_plain_and_new() {
        let home = tempfile::tempdir().unwrap();
        assert!(target_dir(home.path(), "work").is_ok());
        for bad in ["", "main", "Work", "a/b", "../x", "-x", "with space"] {
            assert!(target_dir(home.path(), bad).is_err(), "{bad}");
        }
        account(home.path(), "work");
        assert!(target_dir(home.path(), "work").is_err());
    }

    #[test]
    fn a_new_account_gets_setup_and_memory_but_no_login_or_conversations() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let main = account(h, "main");
        let second = account(h, "second");
        put(&main.config_dir.join("settings.json"), r#"{"permissions":{"allow":["A"]},"theme":"dark"}"#);
        put(&second.config_dir.join("settings.json"), r#"{"permissions":{"allow":["B"]},"theme":"light"}"#);
        put(&main.config_dir.join("CLAUDE.md"), "main rules");
        put(&second.config_dir.join("CLAUDE.md"), "second rules");
        put(&main.config_dir.join("skills/fux/SKILL.md"), "fux by main");
        put(&second.config_dir.join("skills/fux/SKILL.md"), "fux by second");
        put(&second.config_dir.join("skills/haushalt/SKILL.md"), "only in second");
        put(&main.config_dir.join("agents/expert.md"), "expert");
        // second's plugin list points into main's plugin folder, as on Martin's Mac
        let main_plugins = format!("{}/plugins/", main.config_dir.display());
        put(&second.config_dir.join("plugins/installed_plugins.json"), &format!(r#"{{"plugins":{{"p@m":[{{"installPath":"{main_plugins}cache/m/p/1"}}]}}}}"#));
        put(&main.config_dir.join("plugins/cache/m/p/1/plugin.json"), "{}");
        put(&main.config_dir.join("plugins/cache/temp_git_123/x"), "leftover");
        put(&h.join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"me@x"},"mcpServers":{"xcode":{"command":"x"}}}"#);
        put(&second.config_dir.join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"other@x"},"mcpServers":{}}"#);
        put(&main.config_dir.join("projects/-w-app/memory/MEMORY.md"), "- [A](a.md) — a\n");
        put(&main.config_dir.join("projects/-w-app/memory/a.md"), "note a");
        put(&second.config_dir.join("projects/-w-app/memory/MEMORY.md"), "- [A](a.md) — a\n- [B](b.md) — b\n");
        put(&second.config_dir.join("projects/-w-app/memory/b.md"), "note b");
        // the same note in both: second's is newer, so it goes in; its index line too only once
        put(&main.config_dir.join("projects/-w-app/memory/c.md"), "c, old");
        put(&second.config_dir.join("projects/-w-app/memory/c.md"), "c, updated");
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(main.config_dir.join("projects/-w-app/memory/c.md")).unwrap().set_modified(an_hour_ago).unwrap();
        put(&main.config_dir.join("projects/-w-app/memory/MEMORY.md"), "- [A](a.md) — a\n- [C](c.md) — c\n");
        put(&second.config_dir.join("projects/-w-app/memory/MEMORY.md"), "- [A](a.md) — a\n- [B](b.md) — b\n- [C](c.md) — c, updated\n");
        put(&second.config_dir.join("projects/-w-app/abc.jsonl"), "a conversation");
        put(&main.config_dir.join("history.jsonl"), "history");

        let target = target_dir(h, "work").unwrap();
        let sources = [main, second];
        let preview = plan(&sources, h, &target);
        assert!(!target.exists(), "the plan writes nothing");
        let made = create(&sources, h, &target).unwrap();
        assert_eq!(preview.items, made.items);

        let read = |p: &str| std::fs::read_to_string(target.join(p)).unwrap();
        let settings: Value = serde_json::from_str(&read("settings.json")).unwrap();
        assert_eq!(settings, json!({ "permissions": { "allow": ["A", "B"] }, "theme": "dark" }));
        assert_eq!(read("CLAUDE.md"), "main rules");
        assert_eq!(read("skills/fux/SKILL.md"), "fux by main");
        assert_eq!(read("skills/haushalt/SKILL.md"), "only in second");
        assert_eq!(read("agents/expert.md"), "expert");
        assert!(read("plugins/installed_plugins.json").contains(&format!("{}/plugins/cache/m/p/1", target.display())));
        assert!(target.join("plugins/cache/m/p/1/plugin.json").exists());
        assert!(!target.join("plugins/cache/temp_git_123").exists());
        assert_eq!(read(".claude.json").trim(), "{\n  \"mcpServers\": {\n    \"xcode\": {\n      \"command\": \"x\"\n    }\n  }\n}");
        assert_eq!(read("projects/-w-app/memory/MEMORY.md"), "- [A](a.md) — a\n- [C](c.md) — c\n- [B](b.md) — b\n");
        assert_eq!(read("projects/-w-app/memory/b.md"), "note b");
        assert_eq!(read("projects/-w-app/memory/c.md"), "c, updated");
        assert!(!target.join("projects/-w-app/abc.jsonl").exists());
        assert!(!target.join("history.jsonl").exists());
        assert!(target.join("sessions").is_dir(), "Brain lists it right away");

        let conflicts: Vec<(&str, &str)> = made.conflicts().collect();
        assert!(conflicts.contains(&("settings.json", "theme: second has \"light\", kept the first")));
        assert!(conflicts.contains(&("CLAUDE.md", "second's differs, kept the first")));
        assert!(conflicts.contains(&("skills/fux", "second's differs, kept the first")));
        assert!(conflicts.contains(&("memory: -w-app", "c.md: second's is newer, main's left out")));
        assert!(create(&sources, h, &target).is_err(), "never over an existing folder");
    }
}
