use std::path::Path;

use serde_json::{json, Map, Value};

/// Hook events Brain listens to.
pub const HOOK_EVENTS: [&str; 5] = ["SessionStart", "UserPromptSubmit", "Notification", "Stop", "SessionEnd"];

/// Recognises hook entries written by an earlier install.
const HOOK_MARKER: &str = "brain hook";
pub const PERMISSION_RULE: &str = "Bash(brain report:*)";

const SECTION_START: &str = "<!-- brain:start -->";
const SECTION_END: &str = "<!-- brain:end -->";

pub fn protocol_section() -> String {
    format!(
        "{SECTION_START}
## Brain status protocol

The user tracks all Claude Code sessions in the Brain dashboard. Report there with the `brain` CLI — one line of text, in the user's language:

- Before ending a turn with a question or decision for the user: `brain report --waiting \"<the question>\"`
- After finishing a substantial task: `brain report --done \"<one-line result>\"`
- When starting a long multi-step task: `brain report --doing \"<what you are doing>\"`
- If you end a turn while subagents or background tasks still run and you need nothing from the user, don't report `--waiting`: Brain shows the session as working in the background. If you do ask the user something, report `--waiting` even while work is running.

Session state (working, permission prompts, turn end) is tracked automatically by hooks; only report content. If `brain` fails, ignore it and continue.
{SECTION_END}
"
    )
}

/// Adds Brain's hooks and permission rule to a `settings.json` value.
/// Earlier Brain entries are replaced, everything else is left untouched.
pub fn patch_settings(settings: &mut Value, hook_command: &str) {
    if !settings.is_object() {
        *settings = Value::Object(Map::new());
    }
    let root = settings.as_object_mut().expect("object");

    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks = hooks.as_object_mut().expect("object");
    for event in HOOK_EVENTS {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        if !groups.is_array() {
            *groups = json!([]);
        }
        let groups = groups.as_array_mut().expect("array");
        groups.retain(|group| !group_is_brain(group));
        groups.push(json!({
            "hooks": [{ "type": "command", "command": hook_command, "timeout": 5 }]
        }));
    }

    if let Some(brain) = hook_command.strip_suffix(" hook") {
        wrap_statusline(root, brain);
    }

    let permissions = root.entry("permissions").or_insert_with(|| json!({}));
    if let Some(permissions) = permissions.as_object_mut() {
        let allow = permissions.entry("allow").or_insert_with(|| json!([]));
        if let Some(allow) = allow.as_array_mut() {
            if !allow.iter().any(|rule| rule == PERMISSION_RULE) {
                allow.push(json!(PERMISSION_RULE));
            }
        }
    }
}

/// Puts `brain statusline` in front of the configured status line, so Brain sees the status line
/// JSON (plan limits, context, cost). The previous command runs unchanged after it, with the same
/// input, and its output is what Claude Code shows. Re-running only updates Brain's path.
fn wrap_statusline(root: &mut Map<String, Value>, brain: &str) {
    let current = root.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str).map(str::to_string);
    let previous = match current {
        Some(command) => match wrapped_command(&command) {
            Some(inner) => inner,
            None => Some(command),
        },
        None => None,
    };
    let command = match &previous {
        Some(inner) => format!("{brain} statusline --then {}", shell_single_quote(inner)),
        None => format!("{brain} statusline"),
    };
    let line = root.entry("statusLine").or_insert_with(|| json!({}));
    if !line.is_object() {
        *line = json!({});
    }
    let line = line.as_object_mut().expect("object");
    line.insert("type".into(), json!("command"));
    line.insert("command".into(), json!(command));
}

/// The reverse: restores the wrapped command, or removes a status line Brain added alone.
fn unwrap_statusline(root: &mut Map<String, Value>) {
    let Some(command) = root.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str) else {
        return;
    };
    match wrapped_command(command) {
        Some(Some(inner)) => {
            if let Some(line) = root.get_mut("statusLine").and_then(Value::as_object_mut) {
                line.insert("command".into(), json!(inner));
            }
        }
        Some(None) => {
            root.remove("statusLine");
        }
        None => {}
    }
}

/// `Some(previous)` when `command` is Brain's wrapper (`None` inside: nothing was wrapped).
fn wrapped_command(command: &str) -> Option<Option<String>> {
    let (program, rest) = command.split_once(" statusline")?;
    if !program.trim_matches('\'').ends_with("brain") {
        return None;
    }
    let rest = rest.trim_start();
    if rest.is_empty() {
        return Some(None);
    }
    let quoted = rest.strip_prefix("--then ")?;
    Some(Some(shell_single_unquote(quoted.trim())?))
}

fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Reads `'…'` written by [`shell_single_quote`].
fn shell_single_unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace(r"'\''", "'"))
}

fn group_is_brain(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(HOOK_MARKER))
            })
        })
}

/// Puts exactly one protocol section into a CLAUDE.md: every earlier one goes (between markers,
/// and copies whose markers another edit removed), stray markers too, then the current section
/// takes the place of the first one, or the end of the file.
pub fn patch_claude_md(content: &str) -> String {
    let section = protocol_section();
    let body = section.trim_start_matches(SECTION_START).trim_end_matches(SECTION_END).trim().to_string();
    let mut rest = content.to_string();
    let mut place: Option<usize> = None;
    // Marked sections: a start and the next end after it.
    while let Some(start) = rest.find(SECTION_START) {
        let Some(len) = rest[start..].find(SECTION_END) else { break };
        let end = start + len + SECTION_END.len();
        rest.replace_range(start..end, "");
        place = Some(place.map_or(start, |p| p.min(start)));
    }
    // Unmarked copies of the protocol, and markers left on their own.
    while let Some(at) = rest.find(&body) {
        rest.replace_range(at..at + body.len(), "");
        place = Some(place.map_or(at, |p| p.min(at)));
    }
    for marker in [SECTION_START, SECTION_END] {
        while let Some(at) = rest.find(marker) {
            rest.replace_range(at..at + marker.len(), "");
            place = Some(place.map_or(at, |p| p.min(at)));
        }
    }
    let (before, after) = match place {
        Some(at) => (rest[..at].trim_end().to_string(), rest[at..].trim_start().to_string()),
        None => (rest.trim_end().to_string(), String::new()),
    };
    let mut out = before;
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(section.trim_end());
    if !after.is_empty() {
        out.push_str("\n\n");
        out.push_str(after.trim_end());
    }
    out.push('\n');
    out
}

/// Removes Brain's hooks and permission rule from a `settings.json` value,
/// the reverse of [`patch_settings`]. Containers that only become empty
/// through this removal are dropped; everything else is left untouched.
pub fn unpatch_settings(settings: &mut Value) {
    let Some(root) = settings.as_object_mut() else { return };
    unwrap_statusline(root);

    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        let had_events = !hooks.is_empty();
        hooks.retain(|_, groups| {
            let Some(groups) = groups.as_array_mut() else { return true };
            let before = groups.len();
            groups.retain(|group| !group_is_brain(group));
            !(groups.is_empty() && before > 0)
        });
        if had_events && hooks.is_empty() {
            root.remove("hooks");
        }
    }

    if let Some(permissions) = root.get_mut("permissions").and_then(Value::as_object_mut) {
        if let Some(allow) = permissions.get_mut("allow").and_then(Value::as_array_mut) {
            let before = allow.len();
            allow.retain(|rule| rule != PERMISSION_RULE);
            if allow.is_empty() && before > 0 {
                permissions.remove("allow");
                if permissions.is_empty() {
                    root.remove("permissions");
                }
            }
        }
    }
}

/// Removes the protocol section and the blank line before it, the reverse of
/// [`patch_claude_md`].
pub fn unpatch_claude_md(content: &str) -> String {
    let (Some(start), Some(end)) = (content.find(SECTION_START), content.find(SECTION_END)) else {
        return content.to_string();
    };
    if start > end {
        return content.to_string();
    }
    let before = &content[..start];
    let before = before.strip_suffix('\n').filter(|b| b.ends_with('\n')).unwrap_or(before);
    let after = &content[end + SECTION_END.len()..];
    let after = after.strip_prefix('\n').unwrap_or(after);
    format!("{before}{after}")
}

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    Updated { backup: Option<std::path::PathBuf> },
}

/// Applies `patch` to the file at `path`, writing a timestamped backup first
/// when the file existed and its content changes.
pub fn rewrite_file(path: &Path, patch: impl FnOnce(&str) -> String) -> std::io::Result<Change> {
    let before = std::fs::read_to_string(path).ok();
    let after = patch(before.as_deref().unwrap_or(""));
    if before.as_deref() == Some(after.as_str()) {
        return Ok(Change::Unchanged);
    }
    let backup = match &before {
        Some(content) => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let name = format!("{}.brain-backup-{stamp}", path.file_name().unwrap_or_default().to_string_lossy());
            // Never overwrite an earlier backup, e.g. of an install and an
            // uninstall within the same second.
            let backup = (1..)
                .map(|n| path.with_file_name(if n == 1 { name.clone() } else { format!("{name}-{n}") }))
                .find(|candidate| !candidate.exists())
                .expect("unbounded range");
            std::fs::write(&backup, content)?;
            Some(backup)
        }
        None => None,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, after)?;
    Ok(Change::Updated { backup })
}

/// `settings.json` patcher for [`rewrite_file`]. Invalid JSON is an error
/// rather than being overwritten.
pub fn patch_settings_text(text: &str, hook_command: &str) -> Result<String, serde_json::Error> {
    let mut value: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)?
    };
    patch_settings(&mut value, hook_command);
    let mut out = serde_json::to_string_pretty(&value)?;
    out.push('\n');
    Ok(out)
}

/// `settings.json` un-patcher for [`rewrite_file`]. Returns the text as is
/// when there is nothing to remove, so untouched files keep their formatting.
pub fn unpatch_settings_text(text: &str) -> Result<String, serde_json::Error> {
    if text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let original: Value = serde_json::from_str(text)?;
    let mut value = original.clone();
    unpatch_settings(&mut value);
    if value == original {
        return Ok(text.to_string());
    }
    let mut out = serde_json::to_string_pretty(&value)?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_is_wrapped_once_and_restored_exactly() {
        let original = json!({ "statusLine": { "type": "command", "command": "bash \"$HOME/.claude/statusline-command.sh\"", "padding": 0 } });
        let mut settings = original.clone();

        patch_settings(&mut settings, "/opt/bin/brain hook");
        let once = settings.clone();
        assert_eq!(
            settings["statusLine"]["command"],
            "/opt/bin/brain statusline --then 'bash \"$HOME/.claude/statusline-command.sh\"'"
        );
        assert_eq!(settings["statusLine"]["padding"], 0);

        patch_settings(&mut settings, "/opt/bin/brain hook");
        assert_eq!(settings["statusLine"], once["statusLine"], "no double wrapping");
        patch_settings(&mut settings, "/new/brain hook");
        assert!(settings["statusLine"]["command"].as_str().unwrap().starts_with("/new/brain statusline --then"));

        unpatch_settings(&mut settings);
        assert_eq!(settings["statusLine"], original["statusLine"]);
    }

    #[test]
    fn a_status_line_brain_added_alone_is_removed_again() {
        let mut settings = json!({});
        patch_settings(&mut settings, "brain hook");
        assert_eq!(settings["statusLine"]["command"], "brain statusline");
        unpatch_settings(&mut settings);
        assert!(settings.get("statusLine").is_none());
        assert_eq!(shell_single_unquote(&shell_single_quote("it's")).as_deref(), Some("it's"));
    }

    #[test]
    fn settings_patch_keeps_foreign_hooks_and_is_idempotent() {
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "other-tool" }] }]
            },
            "permissions": { "allow": ["Bash(ls:*)"] }
        });

        patch_settings(&mut settings, "/bin/brain hook");
        let once = settings.clone();
        patch_settings(&mut settings, "/bin/brain hook");

        assert_eq!(settings, once);
        assert_eq!(settings["model"], "opus");
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "other-tool");
        assert_eq!(stop[1]["hooks"][0]["command"], "/bin/brain hook");
        for event in HOOK_EVENTS {
            assert_eq!(settings["hooks"][event].as_array().unwrap().iter().filter(|g| group_is_brain(g)).count(), 1);
        }
        assert_eq!(settings["permissions"]["allow"], json!(["Bash(ls:*)", PERMISSION_RULE]));
    }

    #[test]
    fn claude_md_section_is_appended_once_and_replaced_in_place() {
        let original = "# Prefs\n\nBe nice.\n";
        let first = patch_claude_md(original);
        assert!(first.starts_with("# Prefs\n\nBe nice.\n\n<!-- brain:start -->"));
        assert_eq!(patch_claude_md(&first), first);

        let stale = first.replace("Brain status protocol", "Old title") + "\n## After\n";
        let refreshed = patch_claude_md(&stale);
        assert_eq!(refreshed.matches(SECTION_START).count(), 1);
        assert!(refreshed.contains("Brain status protocol"));
        assert!(refreshed.ends_with("<!-- brain:end -->\n\n## After\n"));
    }

    #[test]
    fn duplicated_sections_and_stray_markers_collapse_into_one() {
        // Another edit removed the first start marker; each install then appended a section.
        let section = protocol_section();
        let body = section.trim_start_matches(SECTION_START).trim_end_matches(SECTION_END).trim();
        let broken = format!("# Prefs\n\n{body}\n<!-- brain:end -->\n\n{section}\n\n{section}\n");

        let fixed = patch_claude_md(&broken);

        assert_eq!(fixed.matches("## Brain status protocol").count(), 1);
        assert_eq!(fixed.matches(SECTION_START).count(), 1);
        assert_eq!(fixed.matches(SECTION_END).count(), 1);
        assert!(fixed.starts_with("# Prefs\n\n<!-- brain:start -->"));
        assert_eq!(patch_claude_md(&fixed), fixed, "installing again changes nothing");
    }

    #[test]
    fn settings_unpatch_restores_the_original_and_keeps_foreign_hooks() {
        let originals = [
            json!({}),
            json!({
                "model": "opus",
                "hooks": {
                    "Stop": [{ "hooks": [{ "type": "command", "command": "other-tool" }] }],
                    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "guard" }] }]
                },
                "permissions": { "allow": ["Bash(ls:*)"], "deny": ["Bash(rm:*)"] }
            }),
        ];
        for original in originals {
            let mut settings = original.clone();
            patch_settings(&mut settings, "/bin/brain hook");
            unpatch_settings(&mut settings);
            assert_eq!(settings, original);
            unpatch_settings(&mut settings);
            assert_eq!(settings, original);
        }

        let text = "{\n    \"model\": \"opus\"\n}";
        assert_eq!(unpatch_settings_text(text).unwrap(), text);
        assert!(unpatch_settings_text("{ broken").is_err());
    }

    #[test]
    fn claude_md_unpatch_restores_the_original() {
        for original in ["", "# Prefs\n\nBe nice.\n"] {
            let patched = patch_claude_md(original);
            assert_eq!(unpatch_claude_md(&patched), original);
            assert_eq!(unpatch_claude_md(original), original);
        }

        let with_text_after = patch_claude_md("# Prefs\n") + "\n## After\n";
        assert_eq!(unpatch_claude_md(&with_text_after), "# Prefs\n\n## After\n");
    }

    #[test]
    fn rewrite_backs_up_changed_files_and_refuses_broken_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ broken").unwrap();
        assert!(patch_settings_text("{ broken", "brain hook").is_err());

        std::fs::write(&path, "{}").unwrap();
        let change = rewrite_file(&path, |t| patch_settings_text(t, "brain hook").unwrap()).unwrap();
        let Change::Updated { backup: Some(backup) } = change else { panic!("expected backup") };
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{}");
        assert_eq!(
            rewrite_file(&path, |t| patch_settings_text(t, "brain hook").unwrap()).unwrap(),
            Change::Unchanged
        );

        // An uninstall right after the install keeps the first backup.
        let change = rewrite_file(&path, |t| unpatch_settings_text(t).unwrap()).unwrap();
        let Change::Updated { backup: Some(second) } = change else { panic!("expected backup") };
        assert_ne!(second, backup);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{}");
    }
}
