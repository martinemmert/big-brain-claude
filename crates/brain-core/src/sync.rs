//! Keeps the Claude accounts' setup in step: `settings.json`, `CLAUDE.md`, the status line
//! script, skills, agents, commands, output styles, user MCP servers and every project's memory
//! notes.
//!
//! After each sync Brain keeps the state the accounts agreed on (`~/.claude-brain/sync/base.json`).
//! Next time every account is compared with it item by item (a setting, an entry of a list, a
//! skill, a note): what one account changed since goes to the others, deletions included; where
//! several changed the same item in different ways, that's a conflict. The newest change is the
//! suggestion. Memory notes take it right away (memory moves on), anything else waits for the
//! user, who may also let an account keep its own version (`keep.json`): that item is then left
//! out for that account.
//!
//! An account the base doesn't know yet, or one without any item of a kind (no skills at all,
//! no memory for a project), only receives: what it lacks is missing, not deleted. Before the
//! first sync nothing is known to be deleted, so the accounts are joined.
//!
//! Never touched: the login, conversations, the plugins' files, `skills/synced` (claude.ai's own
//! sync, per Claude account). What a sync replaces or deletes is kept in `backup/<time>/` first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::account::Account;
use crate::account_setup::{copy_tree, global_json, linked_note, read_json, readable_project, write_json, ENTRY_DIRS, PLAIN_FILES};

/// Skill folders claude.ai fills itself, different for each Claude account.
const OWN_ENTRIES: [&str; 1] = ["skills/synced"];
/// Stands for the account's own folder in settings and MCP servers.
const CONFIG: &str = "<config>";
/// Backups older than this go.
const BACKUP_DAYS: u64 = 30;
/// Settings that grant rights or run commands: they never spread on their own at a session start,
/// only after the user saw them in Brain's dialog. Otherwise whatever got into one account (a
/// hook, an allowed command, an API key helper) would reach every other one unseen.
const REVIEWED_SETTINGS: [&str; 10] = [
    "permissions",
    "hooks",
    "env",
    "statusLine",
    "apiKeyHelper",
    "awsAuthRefresh",
    "awsCredentialExport",
    "otelHeadersHelper",
    "enabledPlugins",
    "extraKnownMarketplaces",
];

/// Where Brain keeps the base, the exceptions and the backups.
pub fn default_dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/sync")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Setting,
    File,
    Entry,
    Mcp,
    /// A memory note, or a line of a project's `MEMORY.md`.
    Memory,
}

/// What a sync would do.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// No sync ran yet: the accounts are joined.
    pub first: bool,
    /// What differs, conflicts first.
    pub items: Vec<Item>,
    /// What an account keeps different: (account, label).
    pub kept: Vec<(String, String)>,
    /// Items all accounts agree on by now, but differently from the base.
    settled: Vec<(String, Option<Value>)>,
    accounts: Vec<String>,
}

impl Plan {
    pub fn conflicts(&self) -> usize {
        self.items.iter().filter(|i| i.conflict).count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub label: String,
    /// Each account's version, in account order.
    pub versions: Vec<Version>,
    /// The account whose version goes to the others; for a conflict, the suggestion.
    pub take: String,
    /// Several accounts changed it differently: the user decides.
    pub conflict: bool,
    /// Grants rights or runs commands (hooks, permissions, MCP servers …): a session start
    /// leaves it for the user, like a conflict.
    pub review: bool,
}

impl Item {
    /// The accounts that get another version than they have.
    pub fn receivers(&self) -> Vec<&str> {
        let value = self.versions.iter().find(|v| v.account == self.take).map(|v| &v.value);
        self.versions.iter().filter(|v| Some(&v.value) != value).map(|v| v.account.as_str()).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    pub account: String,
    /// `None`: the account doesn't have it. Settings and MCP servers: the value; files, skills,
    /// notes: a hash of their content.
    pub value: Option<Value>,
    /// When its file last changed (Unix seconds).
    pub at: Option<i64>,
    /// Changed since the last sync.
    pub changed: bool,
}

impl Version {
    /// How it reads next to the others: a setting's value, else when its file changed; `—` when
    /// the account doesn't have it.
    pub fn describe(&self, kind: Kind) -> String {
        match (&self.value, kind) {
            (None, _) => "—".into(),
            (Some(value), Kind::Setting | Kind::Mcp) => crate::account_setup::short(value),
            (Some(_), _) => self
                .at
                .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
                .map(|t| t.with_timezone(&chrono::Local).format("%d.%m. %H:%M").to_string())
                .unwrap_or_else(|| "✓".into()),
        }
    }
}

/// The user's answer for an item: whose version goes everywhere, and which accounts keep their
/// own from now on instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub id: String,
    pub take: String,
    pub keep: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Items brought in step.
    pub synced: usize,
    /// Conflicts still open.
    pub open: usize,
    pub backup: Option<PathBuf>,
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct Base {
    accounts: Vec<String>,
    items: BTreeMap<String, Value>,
}

/// What each account keeps different: item ids by account.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Keep {
    #[serde(default)]
    keep: BTreeMap<String, BTreeSet<String>>,
}

impl Keep {
    fn has(&self, account: &str, id: &str) -> bool {
        self.keep.get(account).is_some_and(|ids| ids.contains(id))
    }
}

fn load<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    write_json(path, &serde_json::to_value(value).map_err(std::io::Error::other)?)
}

/// The open conflicts after the last sync, for Brain's title bar.
pub fn open_conflicts(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("open")).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

// ---- reading the accounts -------------------------------------------------------------------

struct Seen {
    value: Value,
    at: Option<SystemTime>,
}

struct Snapshot {
    account: Account,
    items: BTreeMap<String, Seen>,
    /// Groups whose file or folder is there: without it the account only receives (a missing
    /// `settings.json` deletes nothing elsewhere).
    present: BTreeSet<String>,
    /// Groups whose file couldn't be read (a broken `settings.json`): left out this time.
    unreadable: BTreeSet<String>,
}

fn snapshot(account: &Account, home: &Path) -> Snapshot {
    let mut snap = Snapshot { account: account.clone(), items: BTreeMap::new(), present: BTreeSet::new(), unreadable: BTreeSet::new() };
    let dir = &account.config_dir;
    let settings = dir.join("settings.json");
    if settings.exists() {
        match read_json(&settings) {
            Some(mut value) => {
                snap.present.insert("settings".into());
                normalize(&mut value, dir);
                let at = modified(&settings);
                let mut found = BTreeMap::new();
                flatten(&value, &mut Vec::new(), &mut found);
                for (id, value) in found {
                    snap.items.insert(id, Seen { value, at });
                }
            }
            None => {
                snap.unreadable.insert("settings".into());
            }
        }
    }
    for name in PLAIN_FILES {
        let path = dir.join(name);
        if let Some((hash, at)) = tree_hash(&path) {
            snap.present.insert(format!("file {name}"));
            snap.items.insert(format!("file {name}"), Seen { value: Value::String(hash), at });
        }
    }
    for sub in ENTRY_DIRS {
        if dir.join(sub).is_dir() {
            snap.present.insert(format!("entry {sub}"));
        }
        for (name, path) in names_in(&dir.join(sub)) {
            let rel = format!("{sub}/{name}");
            if OWN_ENTRIES.contains(&rel.as_str()) {
                continue;
            }
            if let Some((hash, at)) = tree_hash(&path) {
                snap.items.insert(format!("entry {rel}"), Seen { value: Value::String(hash), at });
            }
        }
    }
    let global = global_json(account, home);
    if global.exists() {
        match read_json(&global) {
            Some(state) => {
                snap.present.insert("mcp".into());
                let at = modified(&global);
                for (name, mut server) in state.get("mcpServers").and_then(Value::as_object).cloned().unwrap_or_default() {
                    normalize(&mut server, dir);
                    snap.items.insert(format!("mcp {name}"), Seen { value: server, at });
                }
            }
            None => {
                snap.unreadable.insert("mcp".into());
            }
        }
    }
    for (project, path) in names_in(&dir.join("projects")) {
        let memory = path.join("memory");
        if memory.is_dir() {
            snap.present.insert(format!("memory {project}"));
        }
        for (name, note) in names_in(&memory) {
            if name == "MEMORY.md" {
                let at = modified(&note);
                for line in std::fs::read_to_string(&note).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()) {
                    snap.items.insert(index_id(&project, index_key(line)), Seen { value: Value::String(line.to_string()), at });
                }
            } else if let Some((hash, at)) = tree_hash(&note) {
                snap.items.insert(format!("memory {project}/{name}"), Seen { value: Value::String(hash), at });
            }
        }
    }
    snap
}

/// The entries of a folder by name, without hidden ones (`.DS_Store`, `.trash`, venvs).
fn names_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path).and_then(|m| m.modified()).ok()
}

/// A settings value as leaves: a plain value, an empty object, or one entry of a list (`true`).
/// Path segments are keys, or `{"[]": entry}` for a list's entry; by item id.
fn flatten(value: &Value, path: &mut Vec<Value>, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, value) in map {
                path.push(Value::String(key.clone()));
                flatten(value, path, out);
                path.pop();
            }
        }
        Value::Array(list) => {
            for entry in list {
                path.push(serde_json::json!({ "[]": entry }));
                out.insert(setting_id(path), Value::Bool(true));
                path.pop();
            }
        }
        _ if path.is_empty() => {}
        _ => {
            out.insert(setting_id(path), value.clone());
        }
    }
}

fn setting_id(path: &[Value]) -> String {
    format!("settings {}", Value::Array(path.to_vec()))
}

fn index_key(line: &str) -> &str {
    linked_note(line).unwrap_or(line)
}

fn index_id(project: &str, key: &str) -> String {
    format!("index {project}\t{key}")
}

/// Items of a group arrive together: an account without any of them only receives.
fn group_of(id: &str) -> String {
    let (kind, rest) = id.split_once(' ').unwrap_or((id, ""));
    match kind {
        "settings" | "mcp" => kind.to_string(),
        "file" => id.to_string(),
        "entry" => format!("entry {}", rest.split('/').next().unwrap_or_default()),
        "memory" => format!("memory {}", rest.split('/').next().unwrap_or_default()),
        "index" => format!("memory {}", rest.split('\t').next().unwrap_or_default()),
        _ => id.to_string(),
    }
}

/// MCP servers and the settings in [`REVIEWED_SETTINGS`].
fn needs_review(id: &str) -> bool {
    match id.split_once(' ') {
        Some(("mcp", _)) => true,
        Some(("settings", path)) => {
            let path: Vec<Value> = serde_json::from_str(path).unwrap_or_default();
            path.first().and_then(Value::as_str).is_some_and(|key| REVIEWED_SETTINGS.contains(&key))
        }
        _ => false,
    }
}

fn kind_of(id: &str) -> Kind {
    match id.split(' ').next().unwrap_or_default() {
        "settings" => Kind::Setting,
        "file" => Kind::File,
        "entry" => Kind::Entry,
        "mcp" => Kind::Mcp,
        _ => Kind::Memory,
    }
}

/// How an item reads in a list: `settings.json: model`, `skills/fux`, `memory app: note.md`.
fn label_of(id: &str) -> String {
    let (kind, rest) = id.split_once(' ').unwrap_or((id, ""));
    match kind {
        "settings" => {
            let path: Vec<Value> = serde_json::from_str(rest).unwrap_or_default();
            let mut text = String::new();
            for segment in path {
                match segment {
                    Value::String(key) => {
                        if !text.is_empty() {
                            text.push('.');
                        }
                        text.push_str(&key);
                    }
                    entry => text.push_str(&format!(" ∋ {}", crate::account_setup::short(entry.get("[]").unwrap_or(&Value::Null)))),
                }
            }
            format!("settings.json: {text}")
        }
        "file" | "entry" => rest.to_string(),
        "mcp" => format!("MCP: {rest}"),
        "memory" => {
            let (project, note) = rest.split_once('/').unwrap_or((rest, ""));
            format!("memory {}: {note}", readable_project(project))
        }
        "index" => {
            let (project, key) = rest.split_once('\t').unwrap_or((rest, ""));
            format!("memory {}: MEMORY.md ({key})", readable_project(project))
        }
        _ => id.to_string(),
    }
}

/// The account's own folder as `<config>` in every string (`/u/.claude-x/hooks/a.sh` →
/// `<config>/hooks/a.sh`), so the accounts compare equal.
fn normalize(value: &mut Value, dir: &Path) {
    let dir = dir.display().to_string();
    replace_strings(value, &|text| {
        if text == dir {
            return Some(CONFIG.to_string());
        }
        text.contains(&format!("{dir}/")).then(|| text.replace(&format!("{dir}/"), &format!("{CONFIG}/")))
    });
}

fn denormalize(value: &mut Value, dir: &Path) {
    let dir = dir.display().to_string();
    replace_strings(value, &|text| text.contains(CONFIG).then(|| text.replace(CONFIG, &dir)));
}

fn replace_strings(value: &mut Value, change: &dyn Fn(&str) -> Option<String>) {
    match value {
        Value::String(text) => {
            if let Some(new) = change(text) {
                *text = new;
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|v| replace_strings(v, change)),
        Value::Object(map) => {
            // Keys can hold paths too (hook matchers don't, but MCP env might).
            let entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            for (key, mut v) in entries {
                replace_strings(&mut v, change);
                map.insert(change(&key).unwrap_or(key), v);
            }
        }
        _ => {}
    }
}

/// FNV-1a: stable across Brain versions, so the base stays valid after an update.
struct Fnv(u64);

impl Fnv {
    fn feed(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
}

/// A file's or folder's content as a hash, and when it last changed (the newest file in it).
fn tree_hash(path: &Path) -> Option<(String, Option<SystemTime>)> {
    std::fs::symlink_metadata(path).ok()?;
    let mut hash = Fnv(0xcbf2_9ce4_8422_2325);
    let mut newest = None;
    feed_tree(&mut hash, &mut newest, path, "");
    Some((format!("{:016x}", hash.0), newest))
}

fn feed_tree(hash: &mut Fnv, newest: &mut Option<SystemTime>, path: &Path, rel: &str) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if let Ok(at) = meta.modified() {
        *newest = (*newest).max(Some(at));
    }
    if meta.file_type().is_symlink() {
        hash.feed(format!("L{rel}\0{}\0", std::fs::read_link(path).map(|t| t.display().to_string()).unwrap_or_default()).as_bytes());
    } else if meta.is_dir() {
        hash.feed(format!("D{rel}\0").as_bytes());
        for (name, child) in names_in(path) {
            feed_tree(hash, newest, &child, &format!("{rel}/{name}"));
        }
    } else {
        hash.feed(format!("F{rel}\0{}\0", meta.len()).as_bytes());
        hash.feed(&std::fs::read(path).unwrap_or_default());
    }
}

// ---- planning -------------------------------------------------------------------------------

/// What a sync would do now, without writing anything.
pub fn plan(dir: &Path, accounts: &[Account], home: &Path) -> Plan {
    let base: Option<Base> = load(&dir.join("base.json"));
    let keep: Keep = load(&dir.join("keep.json")).unwrap_or_default();
    let snaps: Vec<Snapshot> = accounts.iter().map(|a| snapshot(a, home)).collect();
    make_plan(base, &keep, &snaps)
}

fn make_plan(base: Option<Base>, keep: &Keep, snaps: &[Snapshot]) -> Plan {
    let first = base.is_none();
    let base = base.unwrap_or_default();
    let mut ids: BTreeSet<&String> = base.items.keys().collect();
    for snap in snaps {
        ids.extend(snap.items.keys());
    }
    let mut items = Vec::new();
    let mut settled = Vec::new();
    for id in ids {
        let group = group_of(id);
        let before = base.items.get(id);
        let mut versions = Vec::new();
        let mut overwritten = false;
        for snap in snaps {
            if keep.has(&snap.account.id, id) || snap.unreadable.contains(&group) {
                continue;
            }
            let seen = snap.items.get(id);
            let value = seen.map(|s| s.value.clone());
            let known = base.accounts.contains(&snap.account.id) && snap.present.contains(&group);
            // A receiver only adds: what it lacks isn't deleted.
            let changed = if known { value.as_ref() != before } else { value.is_some() && before.is_none() };
            // A receiver's own different version would be overwritten without asking.
            overwritten |= !known && before.is_some() && value.is_some() && value.as_ref() != before;
            let at = seen.and_then(|s| s.at).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            versions.push(Version { account: snap.account.id.clone(), value, at, changed });
        }
        if versions.is_empty() {
            continue;
        }
        let changed: Vec<&Version> = versions.iter().filter(|v| v.changed).collect();
        let mut distinct: Vec<&Option<Value>> = Vec::new();
        for v in &changed {
            if !distinct.contains(&&v.value) {
                distinct.push(&v.value);
            }
        }
        let kind = kind_of(id);
        let disputed = distinct.len() > 1 || overwritten;
        // The newest change, or the base's version when nobody changed it.
        let candidates: Vec<&Version> = if disputed && changed.is_empty() {
            // Only a receiver differs: the version the others agree on.
            versions.iter().filter(|v| v.value.as_ref() == before).collect()
        } else if disputed {
            versions.iter().filter(|v| v.changed || (v.value.is_some() && v.value.as_ref() != before)).collect()
        } else if let Some(first_change) = changed.first() {
            vec![*first_change]
        } else {
            versions.iter().filter(|v| v.value.as_ref() == before).collect()
        };
        // Ties go to the first account (main), as when an account is made.
        let Some(take) = candidates.iter().rev().max_by_key(|v| v.at).map(|v| v.account.clone()) else { continue };
        let target = versions.iter().find(|v| v.account == take).map(|v| v.value.clone()).unwrap_or_default();
        if versions.iter().all(|v| v.value == target) {
            if target.as_ref() != before {
                settled.push((id.clone(), target));
            }
            continue;
        }
        items.push(Item { id: id.clone(), kind, label: label_of(id), versions, take, conflict: disputed && kind != Kind::Memory, review: needs_review(id) });
    }
    items.sort_by_key(|i| !i.conflict);
    let kept = keep.keep.iter().flat_map(|(account, ids)| ids.iter().map(move |id| (account.clone(), label_of(id)))).collect();
    Plan { first, items, kept, settled, accounts: snaps.iter().map(|s| s.account.id.clone()).collect() }
}

// ---- applying -------------------------------------------------------------------------------

/// Brings the accounts in step as planned: every item without a conflict, and the conflicts the
/// user decided. Takes the lock, so two syncs never write at once.
pub fn apply(dir: &Path, accounts: &[Account], home: &Path, plan: &Plan, decisions: &[Decision]) -> Result<Outcome, String> {
    let _lock = Lock::take(dir)?;
    run(dir, accounts, home, plan, decisions, false)
}

/// At the start of a session: what changed without a conflict goes everywhere. Only after the
/// first sync (made by hand, where the user sees what happens).
pub fn on_session_start(dir: &Path, accounts: &[Account], home: &Path) -> Result<Option<Outcome>, String> {
    if !dir.join("base.json").exists() {
        return Ok(None);
    }
    let _lock = Lock::take(dir)?;
    let plan = plan(dir, accounts, home);
    run(dir, accounts, home, &plan, &[], true).map(Some)
}

struct Op {
    account: Account,
    id: String,
    /// The version's owner and value: `None` deletes.
    from: Account,
    value: Option<Value>,
}

/// `unattended`: a session start, where nobody looks; what needs a review stays open.
fn run(dir: &Path, accounts: &[Account], home: &Path, plan: &Plan, decisions: &[Decision], unattended: bool) -> Result<Outcome, String> {
    let mut base: Base = load(&dir.join("base.json")).unwrap_or_default();
    let mut keep: Keep = load(&dir.join("keep.json")).unwrap_or_default();
    let account = |id: &str| accounts.iter().find(|a| a.id == id).cloned();
    let mut ops = Vec::new();
    let mut outcome = Outcome::default();
    for item in &plan.items {
        let decision = decisions.iter().find(|d| d.id == item.id);
        let (take, keeping) = match decision {
            Some(d) => (d.take.as_str(), d.keep.as_slice()),
            None if !item.conflict && !(unattended && item.review) => (item.take.as_str(), &[][..]),
            None => {
                outcome.open += 1;
                continue;
            }
        };
        let Some(chosen) = item.versions.iter().find(|v| v.account == take) else { continue };
        let Some(from) = account(take) else { continue };
        for version in item.versions.iter().filter(|v| v.value != chosen.value) {
            if keeping.contains(&version.account) {
                keep.keep.entry(version.account.clone()).or_default().insert(item.id.clone());
            } else if let Some(target) = account(&version.account) {
                ops.push(Op { account: target, id: item.id.clone(), from: from.clone(), value: chosen.value.clone() });
            }
        }
        match &chosen.value {
            Some(value) => base.items.insert(item.id.clone(), value.clone()),
            None => base.items.remove(&item.id),
        };
        outcome.synced += 1;
    }
    for (id, value) in &plan.settled {
        match value {
            Some(value) => base.items.insert(id.clone(), value.clone()),
            None => base.items.remove(id),
        };
    }
    let backup = dir.join("backup").join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    write_ops(&ops, home, &backup).map_err(|e| e.to_string())?;
    if !ops.is_empty() {
        outcome.backup = Some(backup);
    }
    base.accounts = plan.accounts.clone();
    save(&dir.join("base.json"), &base).map_err(|e| e.to_string())?;
    save(&dir.join("keep.json"), &keep).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("open"), outcome.open.to_string()).map_err(|e| e.to_string())?;
    prune_backups(&dir.join("backup"));
    Ok(outcome)
}

fn write_ops(ops: &[Op], home: &Path, backup: &Path) -> std::io::Result<()> {
    // Settings and MCP servers: one read and one write per file.
    let mut settings: BTreeMap<PathBuf, (Account, Vec<&Op>)> = BTreeMap::new();
    let mut servers: BTreeMap<PathBuf, (Account, Vec<&Op>)> = BTreeMap::new();
    let mut indexes: BTreeMap<PathBuf, (Account, Vec<&Op>)> = BTreeMap::new();
    for op in ops {
        let (kind, rest) = op.id.split_once(' ').unwrap_or_default();
        let dir = &op.account.config_dir;
        match kind {
            "settings" => settings.entry(dir.join("settings.json")).or_insert_with(|| (op.account.clone(), Vec::new())).1.push(op),
            "mcp" => servers.entry(global_json(&op.account, home)).or_insert_with(|| (op.account.clone(), Vec::new())).1.push(op),
            "index" => {
                let project = rest.split('\t').next().unwrap_or_default();
                indexes.entry(dir.join("projects").join(project).join("memory/MEMORY.md")).or_insert_with(|| (op.account.clone(), Vec::new())).1.push(op);
            }
            // A file, a skill, a note: the chosen account's copy replaces this one's.
            "file" | "entry" | "memory" => {
                let rel = if kind == "memory" { format!("projects/{}", rest.replacen('/', "/memory/", 1)) } else { rest.to_string() };
                let dst = dir.join(&rel);
                if std::fs::symlink_metadata(&dst).is_ok() {
                    let saved = backup.join(&op.account.id).join(&rel);
                    std::fs::create_dir_all(saved.parent().unwrap_or(backup))?;
                    std::fs::rename(&dst, &saved)?;
                }
                if op.value.is_some() {
                    copy_tree(&op.from.config_dir.join(&rel), &dst)?;
                }
            }
            _ => {}
        }
    }
    for (path, (account, ops)) in settings {
        let mut value = read_json(&path).unwrap_or_else(|| Value::Object(Map::new()));
        let before = value.clone();
        for op in ops {
            let mut segments: Value = serde_json::from_str(op.id.split_once(' ').map(|(_, p)| p).unwrap_or("[]")).unwrap_or_default();
            denormalize(&mut segments, &account.config_dir);
            let segments = segments.as_array().cloned().unwrap_or_default();
            let mut new = op.value.clone();
            if let Some(v) = new.as_mut() {
                denormalize(v, &account.config_dir);
            }
            set_path(&mut value, &segments, new);
        }
        if value != before {
            keep_copy(&path, &backup.join(&account.id).join("settings.json"))?;
            write_json(&path, &value)?;
        }
    }
    for (path, (account, ops)) in servers {
        let mut state = read_json(&path).unwrap_or_else(|| Value::Object(Map::new()));
        let before = state.clone();
        if let Some(root) = state.as_object_mut() {
            let map = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
            if let Some(map) = map.as_object_mut() {
                for op in ops {
                    let name = op.id.split_once(' ').map(|(_, n)| n).unwrap_or_default().to_string();
                    match op.value.clone() {
                        Some(mut server) => {
                            denormalize(&mut server, &account.config_dir);
                            map.insert(name, server);
                        }
                        None => {
                            map.remove(&name);
                        }
                    }
                }
            }
        }
        if state != before {
            keep_copy(&path, &backup.join(&account.id).join(".claude.json"))?;
            write_json(&path, &state)?;
        }
    }
    for (path, (account, ops)) in indexes {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        for op in ops {
            let key = op.id.split_once('\t').map(|(_, k)| k).unwrap_or_default();
            let new = op.value.as_ref().and_then(Value::as_str).map(str::to_string);
            match (lines.iter().position(|l| !l.trim().is_empty() && index_key(l) == key), new) {
                (Some(at), Some(line)) => lines[at] = line,
                (Some(at), None) => {
                    lines.remove(at);
                }
                (None, Some(line)) => lines.push(line),
                (None, None) => {}
            }
        }
        let mut new = lines.join("\n");
        new.push('\n');
        if new != text {
            keep_copy(&path, &backup.join(&account.id).join(path.strip_prefix(&account.config_dir).unwrap_or(&path)))?;
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")))?;
            std::fs::write(&path, new)?;
        }
    }
    Ok(())
}

/// Sets (or with `None` removes) the leaf at `path`; a list entry is added or removed. Objects
/// left empty by a removal go too.
fn set_path(value: &mut Value, path: &[Value], new: Option<Value>) {
    let Some((last, parents)) = path.split_last() else { return };
    if new.is_none() {
        remove_path(value, path);
        return;
    }
    let mut at = value;
    for segment in parents {
        let Value::String(key) = segment else { return };
        if !at.is_object() {
            *at = Value::Object(Map::new());
        }
        at = at.as_object_mut().expect("an object").entry(key.clone()).or_insert_with(|| Value::Object(Map::new()));
    }
    match last {
        Value::String(key) => {
            if !at.is_object() {
                *at = Value::Object(Map::new());
            }
            at.as_object_mut().expect("an object").insert(key.clone(), new.unwrap_or_default());
        }
        entry => {
            let entry = entry.get("[]").cloned().unwrap_or_default();
            if !at.is_array() {
                *at = Value::Array(Vec::new());
            }
            let list = at.as_array_mut().expect("a list");
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
    }
}

/// Removes the leaf or list entry at `path`; returns whether its parent object is empty now.
fn remove_path(value: &mut Value, path: &[Value]) -> bool {
    let Some((first, rest)) = path.split_first() else { return false };
    match (value, first) {
        (Value::Object(map), Value::String(key)) => {
            if rest.is_empty() {
                map.remove(key);
            } else if let Some(child) = map.get_mut(key) {
                if remove_path(child, rest) {
                    map.remove(key);
                }
            }
            map.is_empty()
        }
        (Value::Array(list), entry) if rest.is_empty() => {
            let entry = entry.get("[]").cloned().unwrap_or_default();
            list.retain(|v| *v != entry);
            false
        }
        _ => false,
    }
}

fn keep_copy(path: &Path, saved: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(saved.parent().unwrap_or(Path::new("/")))?;
    std::fs::copy(path, saved).map(|_| ())
}

fn prune_backups(dir: &Path) {
    let limit = std::time::Duration::from_secs(BACKUP_DAYS * 24 * 3600);
    for (_, path) in names_in(dir) {
        let old = modified(&path).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// One sync at a time (sessions starting together, Brain's button): a lock file, held while
/// writing. The next one waits a little; a process started meanwhile (between fork and exec it
/// shares the descriptor) can hold the lock a moment longer.
struct Lock(#[allow(dead_code)] std::fs::File);

impl Lock {
    fn take(dir: &Path) -> Result<Lock, String> {
        use std::os::unix::io::AsRawFd;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("lock")).map_err(|e| e.to_string())?;
        for _ in 0..100 {
            // SAFETY: a valid descriptor of an open file; the lock goes when the file closes.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Lock(file));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err("another sync is running".into())
    }
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

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn settings(account: &Account) -> Value {
        serde_json::from_str(&read(&account.config_dir.join("settings.json"))).unwrap()
    }

    /// Makes `path` look changed `secs` ago.
    fn age(path: &Path, secs: u64) {
        let at = SystemTime::now() - std::time::Duration::from_secs(secs);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
    }

    fn sync(dir: &Path, accounts: &[Account], home: &Path, decisions: &[Decision]) -> (Plan, Outcome) {
        let plan = plan(dir, accounts, home);
        let outcome = apply(dir, accounts, home, &plan, decisions).unwrap();
        (plan, outcome)
    }

    #[test]
    fn the_first_sync_joins_the_accounts_and_asks_where_values_differ() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second) = (account(h, "main"), account(h, "second"));
        put(&main.config_dir.join("settings.json"), r#"{"model":"opus","permissions":{"allow":["A"]},"statusLine":{"command":"MAIN/sl.sh"}}"#.replace("MAIN", &main.config_dir.display().to_string()).as_str());
        put(&second.config_dir.join("settings.json"), r#"{"model":"sonnet","permissions":{"allow":["B"]},"statusLine":{"command":"SECOND/sl.sh"}}"#.replace("SECOND", &second.config_dir.display().to_string()).as_str());
        put(&main.config_dir.join("skills/fux/SKILL.md"), "fux");
        put(&second.config_dir.join("skills/synced/abc/SKILL.md"), "claude.ai's own");
        put(&second.config_dir.join("projects/-w-app/memory/note.md"), "a note");
        put(&second.config_dir.join("projects/-w-app/memory/MEMORY.md"), "- [Note](note.md) — a note\n");
        let accounts = [main.clone(), second.clone()];

        assert_eq!(on_session_start(&dir, &accounts, h).unwrap(), None, "the first sync is made by hand");
        let (plan, outcome) = sync(&dir, &accounts, h, &[]);

        assert!(plan.first);
        let conflicts: Vec<&str> = plan.items.iter().filter(|i| i.conflict).map(|i| i.label.as_str()).collect();
        assert_eq!(conflicts, vec!["settings.json: model"], "the account's own folder isn't a difference");
        assert_eq!(outcome.open, 1);
        assert_eq!(settings(&second)["permissions"]["allow"], json!(["B", "A"]));
        assert_eq!(settings(&main)["permissions"]["allow"], json!(["A", "B"]));
        assert_eq!(settings(&main)["model"], json!("opus"), "an open conflict stays as it is");
        assert_eq!(read(&second.config_dir.join("skills/fux/SKILL.md")), "fux");
        assert!(!main.config_dir.join("skills/synced").exists());
        assert_eq!(read(&main.config_dir.join("projects/-w-app/memory/note.md")), "a note");
        assert_eq!(read(&main.config_dir.join("projects/-w-app/memory/MEMORY.md")), "- [Note](note.md) — a note\n");

        // Decided later: second's model, everywhere.
        let (plan, outcome) = sync(&dir, &accounts, h, &[Decision { id: plan.items[0].id.clone(), take: "second".into(), keep: Vec::new() }]);
        assert_eq!(plan.conflicts(), 1);
        assert_eq!((outcome.synced, outcome.open), (1, 0));
        assert_eq!(settings(&main)["model"], json!("sonnet"));
        assert!(plan_is_empty(&dir, &accounts, h));
    }

    fn plan_is_empty(dir: &Path, accounts: &[Account], home: &Path) -> bool {
        plan(dir, accounts, home).items.is_empty()
    }

    #[test]
    fn a_change_or_a_deletion_in_one_account_goes_to_the_others() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second, third) = (account(h, "main"), account(h, "second"), account(h, "third"));
        let accounts = [main.clone(), second.clone(), third.clone()];
        for a in &accounts {
            put(&a.config_dir.join("settings.json"), r#"{"effort":"high","permissions":{"allow":["A","B"]},"enabledPlugins":{"x@m":true}}"#);
            put(&a.config_dir.join("skills/old/SKILL.md"), "old");
            put(&a.config_dir.join("CLAUDE.md"), "rules");
        }
        sync(&dir, &accounts, h, &[]);

        // third: /effort, a rule less, a plugin off; second: a skill gone, CLAUDE.md edited.
        put(&third.config_dir.join("settings.json"), r#"{"effort":"medium","permissions":{"allow":["A"]}}"#);
        std::fs::remove_dir_all(second.config_dir.join("skills/old")).unwrap();
        put(&second.config_dir.join("CLAUDE.md"), "rules, edited");
        let (plan, outcome) = sync(&dir, &accounts, h, &[]);

        assert_eq!(plan.conflicts(), 0);
        assert_eq!(outcome.synced, 5);
        for a in &accounts {
            assert_eq!(settings(a), json!({ "effort": "medium", "permissions": { "allow": ["A"] } }), "{}", a.id);
            assert!(!a.config_dir.join("skills/old").exists());
            assert_eq!(read(&a.config_dir.join("CLAUDE.md")), "rules, edited");
        }
        let backup = outcome.backup.unwrap();
        assert_eq!(read(&backup.join("main/skills/old/SKILL.md")), "old", "what goes is kept");
        assert_eq!(read(&backup.join("main/CLAUDE.md")), "rules");
        assert!(plan_is_empty(&dir, &accounts, h));
    }

    #[test]
    fn different_changes_are_a_conflict_the_newest_suggested_and_keep_leaves_one_out() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second, third) = (account(h, "main"), account(h, "second"), account(h, "third"));
        let accounts = [main.clone(), second.clone(), third.clone()];
        for a in &accounts {
            put(&a.config_dir.join("settings.json"), r#"{"plugins":{"sp":true}}"#);
        }
        sync(&dir, &accounts, h, &[]);
        put(&second.config_dir.join("settings.json"), r#"{"plugins":{"sp":false}}"#);
        put(&third.config_dir.join("settings.json"), r#"{"plugins":{"sp":"later"}}"#);
        age(&second.config_dir.join("settings.json"), 60);

        let plan = plan(&dir, &accounts, h);
        let item = &plan.items[0];
        assert!(item.conflict);
        assert_eq!(item.take, "third", "the newest change");
        assert_eq!(on_session_start(&dir, &accounts, h).unwrap().unwrap().open, 1, "a session start leaves conflicts alone");

        // main's value for all, but second keeps its own from now on.
        let plan = super::plan(&dir, &accounts, h);
        apply(&dir, &accounts, h, &plan, &[Decision { id: plan.items[0].id.clone(), take: "main".into(), keep: vec!["second".into()] }]).unwrap();
        assert_eq!(settings(&third)["plugins"]["sp"], json!(true));
        assert_eq!(settings(&second)["plugins"]["sp"], json!(false));
        let plan = super::plan(&dir, &accounts, h);
        assert!(plan.items.is_empty());
        assert_eq!(plan.kept, vec![("second".to_string(), "settings.json: plugins.sp".to_string())]);

        // main changes it again: third follows, second stays.
        put(&main.config_dir.join("settings.json"), r#"{"plugins":{"sp":"again"}}"#);
        on_session_start(&dir, &accounts, h).unwrap();
        assert_eq!(settings(&third)["plugins"]["sp"], json!("again"));
        assert_eq!(settings(&second)["plugins"]["sp"], json!(false));
    }

    #[test]
    fn rights_and_commands_wait_for_the_user_at_a_session_start() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second) = (account(h, "main"), account(h, "second"));
        let accounts = [main.clone(), second.clone()];
        for a in &accounts {
            put(&a.config_dir.join("settings.json"), r#"{"model":"opus","permissions":{"allow":["A"]}}"#);
        }
        sync(&dir, &accounts, h, &[]);
        put(&main.config_dir.join("settings.json"), r#"{"model":"sonnet","permissions":{"allow":["A","Bash(*)"]},"hooks":{"Stop":[{"command":"x"}]}}"#);
        put(&h.join(".claude.json"), r#"{"mcpServers":{"evil":{"command":"y"}}}"#);

        let outcome = on_session_start(&dir, &accounts, h).unwrap().unwrap();

        assert_eq!(outcome.open, 3, "the allowed command, the hook and the MCP server");
        assert_eq!(settings(&second), json!({ "model": "sonnet", "permissions": { "allow": ["A"] } }));
        assert!(!second.config_dir.join(".claude.json").exists());
        assert_eq!(open_conflicts(&dir), 3, "Brain's title bar shows them");
        // In the dialog they go along.
        sync(&dir, &accounts, h, &[]);
        assert_eq!(settings(&second)["permissions"]["allow"], json!(["A", "Bash(*)"]));
        assert_eq!(open_conflicts(&dir), 0);
    }

    #[test]
    fn memory_takes_the_newer_note_and_joins_the_index() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second) = (account(h, "main"), account(h, "second"));
        let accounts = [main.clone(), second.clone()];
        let memory = |a: &Account, f: &str| a.config_dir.join("projects/-w-app/memory").join(f);
        for a in &accounts {
            put(&memory(a, "MEMORY.md"), "- [C](c.md) — c\n");
            put(&memory(a, "c.md"), "c");
        }
        sync(&dir, &accounts, h, &[]);
        put(&memory(&main, "c.md"), "c from main");
        put(&memory(&second, "c.md"), "c from second");
        age(&memory(&main, "c.md"), 60);
        put(&memory(&main, "MEMORY.md"), "- [C](c.md) — c\n- [D](d.md) — d\n");
        put(&memory(&main, "d.md"), "d");

        let outcome = on_session_start(&dir, &accounts, h).unwrap().unwrap();

        assert_eq!(outcome.open, 0, "memory never waits");
        assert_eq!(read(&memory(&main, "c.md")), "c from second");
        assert_eq!(read(&memory(&second, "d.md")), "d");
        assert_eq!(read(&memory(&second, "MEMORY.md")), "- [C](c.md) — c\n- [D](d.md) — d\n");
    }

    #[test]
    fn a_new_or_broken_account_only_receives() {
        let home = tempfile::tempdir().unwrap();
        let (h, dir) = (home.path(), home.path().join("sync"));
        let (main, second) = (account(h, "main"), account(h, "second"));
        for a in [&main, &second] {
            put(&a.config_dir.join("settings.json"), r#"{"model":"opus","theme":"dark"}"#);
            put(&a.config_dir.join("skills/fux/SKILL.md"), "fux");
        }
        sync(&dir, &[main.clone(), second.clone()], h, &[]);
        // A fresh account with its own few settings, and second's settings.json broken.
        let fresh = account(h, "fresh");
        put(&fresh.config_dir.join("settings.json"), r#"{"theme":"light"}"#);
        put(&second.config_dir.join("settings.json"), "{ broken");
        let accounts = [main.clone(), second.clone(), fresh.clone()];

        let plan = plan(&dir, &accounts, h);
        let conflicts: Vec<&str> = plan.items.iter().filter(|i| i.conflict).map(|i| i.label.as_str()).collect();
        assert_eq!(conflicts, vec!["settings.json: theme"], "its own value isn't overwritten without asking");
        apply(&dir, &accounts, h, &plan, &[]).unwrap();

        assert_eq!(settings(&main), json!({ "model": "opus", "theme": "dark" }), "nothing deleted elsewhere");
        assert_eq!(settings(&fresh)["model"], json!("opus"));
        assert_eq!(read(&fresh.config_dir.join("skills/fux/SKILL.md")), "fux");
        assert_eq!(read(&second.config_dir.join("settings.json")), "{ broken", "a broken file is left alone");
    }

    #[test]
    fn settings_paths_round_trip() {
        let mut value = json!({ "a": { "b": 1 }, "list": ["x"] });
        set_path(&mut value, &[json!("a"), json!("c")], Some(json!(2)));
        set_path(&mut value, &[json!("list"), json!({ "[]": "y" })], Some(json!(true)));
        set_path(&mut value, &[json!("list"), json!({ "[]": "x" })], None);
        assert_eq!(value, json!({ "a": { "b": 1, "c": 2 }, "list": ["y"] }));
        set_path(&mut value, &[json!("a"), json!("b")], None);
        set_path(&mut value, &[json!("a"), json!("c")], None);
        assert_eq!(value, json!({ "list": ["y"] }), "an object left empty goes");
    }
}
