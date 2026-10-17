//! Output styles: how a session answers. Claude Code reads `outputStyle` from its settings at
//! start (`claude --settings '{"outputStyle":"Concise"}'`) and switches with `/output-style
//! <name>` during a session, from the next message on.
//!
//! The built-in ones, an account's own (`<config dir>/output-styles/*.md`) and a project's
//! (`.claude/output-styles/*.md` from the folder up to the repository's root, the nearest wins).

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Style {
    /// What `outputStyle` and `/output-style` take (case matters for `outputStyle`).
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Claude Code's own styles; Default has no style instructions.
pub const BUILT_IN: [&str; 5] = ["Default", "Proactive", "Concise", "Explanatory", "Learning"];

/// The styles a session in `cwd` of the account in `config_dir` can use: built-in first, then
/// the account's, then the project's, each name once.
pub fn available(config_dir: &Path, cwd: Option<&Path>) -> Vec<Style> {
    let mut styles: Vec<Style> = BUILT_IN.iter().map(|name| Style { name: name.to_string(), description: None }).collect();
    let mut add = |found: Vec<Style>| {
        for style in found {
            match styles.iter_mut().find(|s| s.name == style.name) {
                Some(known) => *known = style,
                None => styles.push(style),
            }
        }
    };
    add(read_dir(&config_dir.join("output-styles")));
    let mut project: Vec<Vec<Style>> = Vec::new();
    let mut dir = cwd;
    while let Some(current) = dir {
        project.push(read_dir(&current.join(".claude/output-styles")));
        if current.join(".git").exists() {
            break;
        }
        dir = current.parent();
    }
    // The nearest folder's style wins: added last.
    for found in project.into_iter().rev() {
        add(found);
    }
    styles
}

/// The `--settings` value that starts a session with `name`.
pub fn settings(name: &str) -> String {
    serde_json::json!({ "outputStyle": name }).to_string()
}

fn read_dir(dir: &Path) -> Vec<Style> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut styles: Vec<Style> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| Some(parse(p.file_stem()?.to_string_lossy().as_ref(), &std::fs::read_to_string(&p).ok()?)))
        .collect();
    styles.sort_by_key(|s| s.name.to_lowercase());
    styles
}

/// `name` and `description` from the file's header; the name defaults to the file's.
fn parse(stem: &str, text: &str) -> Style {
    let mut style = Style { name: stem.to_string(), description: None };
    let header = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---")).map(|(header, _)| header).unwrap_or_default();
    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        match key.trim() {
            "name" if !value.is_empty() => style.name = value,
            "description" if !value.is_empty() => style.description = Some(value),
            _ => {}
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_then_the_accounts_then_the_nearest_projects() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("claude");
        let repo = root.path().join("repo");
        let app = repo.join("apps/web");
        for dir in [config.join("output-styles"), repo.join(".claude/output-styles"), app.join(".claude/output-styles")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(config.join("output-styles/kurz.md"), "---\nname: Kurz\ndescription: Nur das Nötigste\n---\nAntworte knapp.").unwrap();
        std::fs::write(repo.join(".claude/output-styles/review.md"), "---\ndescription: from the repo\n---\n").unwrap();
        std::fs::write(app.join(".claude/output-styles/review.md"), "---\ndescription: from the app\n---\n").unwrap();
        std::fs::write(config.join("output-styles/notes.txt"), "no style").unwrap();

        let styles = available(&config, Some(&app));
        let names: Vec<&str> = styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Default", "Proactive", "Concise", "Explanatory", "Learning", "Kurz", "review"]);
        assert_eq!(styles[5].description.as_deref(), Some("Nur das Nötigste"));
        assert_eq!(styles[6].description.as_deref(), Some("from the app"), "the nearest folder wins");
        assert_eq!(settings("Kurz"), r#"{"outputStyle":"Kurz"}"#);
    }
}
