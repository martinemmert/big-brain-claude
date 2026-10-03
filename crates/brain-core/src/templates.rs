//! Session templates: Markdown files in `~/.claude-brain/templates/` with a small header and the
//! first prompt as body. Placeholders like `{ticket}` are asked for when the template starts.
//!
//! ```text
//! ---
//! name: Review a merge request
//! folder: ~/Work/app
//! account: main
//! model: haiku
//! ---
//! Review the merge request for {branch} and list blocking issues first.
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub path: PathBuf,
    pub name: String,
    pub folder: Option<String>,
    pub account: Option<String>,
    pub model: Option<String>,
    pub prompt: String,
}

impl Template {
    /// Placeholder names in order of first appearance, e.g. `["ticket", "branch"]`.
    pub fn placeholders(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut rest = self.prompt.as_str();
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) if is_name(&after[..end]) => {
                    let name = after[..end].to_string();
                    if !out.contains(&name) {
                        out.push(name);
                    }
                    rest = &after[end + 1..];
                }
                _ => rest = after,
            }
        }
        out
    }

    /// The prompt with every `{name}` replaced by its value.
    pub fn fill(&self, values: &[(String, String)]) -> String {
        values.iter().fold(self.prompt.clone(), |prompt, (name, value)| prompt.replace(&format!("{{{name}}}"), value))
    }
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn templates_dir(home: &Path) -> PathBuf {
    home.join(".claude-brain/templates")
}

pub fn parse(path: &Path, text: &str) -> Template {
    let fallback = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut template = Template { path: path.to_path_buf(), name: fallback, folder: None, account: None, model: None, prompt: String::new() };
    let body = match text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---")) {
        Some((header, body)) => {
            for line in header.lines() {
                let Some((key, value)) = line.split_once(':') else { continue };
                let value = value.trim().trim_matches('"').to_string();
                if value.is_empty() {
                    continue;
                }
                match key.trim() {
                    "name" => template.name = value,
                    "folder" => template.folder = Some(value),
                    "account" => template.account = Some(value),
                    "model" => template.model = Some(value),
                    _ => {}
                }
            }
            body.strip_prefix('\n').unwrap_or(body)
        }
        None => text,
    };
    template.prompt = body.trim().to_string();
    template
}

/// All templates, sorted by name.
pub fn load_all(dir: &Path) -> Vec<Template> {
    let mut out: Vec<Template> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| Some(parse(&p, &std::fs::read_to_string(&p).ok()?)))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Writes a new template skeleton and returns its path (`new-template.md`, `-2`, …).
pub fn create(dir: &Path, name: &str, prompt: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join("new-template.md");
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("new-template-{n}.md"));
        n += 1;
    }
    let text = format!("---\nname: {name}\nfolder:\naccount:\nmodel:\n---\n{prompt}\n");
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_and_fills_placeholders() {
        let text = "---\nname: Review MR\nfolder: ~/Work/app\naccount: second\nmodel:\n---\nReview {branch} for {ticket}. Then {branch} again. Keep JSON like {\"a\": 1} and {not a name}.\n";
        let t = parse(Path::new("/t/review.md"), text);

        assert_eq!((t.name.as_str(), t.folder.as_deref(), t.account.as_deref(), t.model.as_deref()), ("Review MR", Some("~/Work/app"), Some("second"), None));
        assert_eq!(t.placeholders(), vec!["branch", "ticket"]);
        assert_eq!(
            t.fill(&[("branch".into(), "fix-42".into()), ("ticket".into(), "FUX-7".into())]),
            "Review fix-42 for FUX-7. Then fix-42 again. Keep JSON like {\"a\": 1} and {not a name}."
        );
    }

    #[test]
    fn a_file_without_header_is_all_prompt_and_new_files_do_not_collide() {
        assert_eq!(parse(Path::new("/t/quick.md"), "Just do it.").name, "quick");
        let dir = tempfile::tempdir().unwrap();
        let first = create(dir.path(), "One", "p").unwrap();
        let second = create(dir.path(), "Two", "p").unwrap();
        assert_ne!(first, second);
        let names: Vec<String> = load_all(dir.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["One", "Two"]);
    }
}
