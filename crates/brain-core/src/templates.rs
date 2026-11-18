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

/// The file text of a template: the header (empty fields left out) and the prompt.
pub fn render(template: &Template) -> String {
    let clean = |s: &str| s.replace(['\n', '\r'], " ").trim().to_string();
    let mut header = format!("---\nname: {}\n", clean(&template.name));
    for (key, value) in [("folder", &template.folder), ("account", &template.account), ("model", &template.model)] {
        if let Some(value) = value.as_deref().map(clean).filter(|v| !v.is_empty()) {
            header.push_str(&format!("{key}: {value}\n"));
        }
    }
    format!("{header}---\n{}\n", template.prompt.trim())
}

/// A file name for a new template from its name (`Review MR` → `review-mr.md`), not taken yet.
pub fn new_path(dir: &Path, name: &str) -> PathBuf {
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let slug = if slug.is_empty() { "template".to_string() } else { slug };
    let mut path = dir.join(format!("{slug}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{slug}-{n}.md"));
        n += 1;
    }
    path
}

/// Writes the template to its path (a new one gets [`new_path`]) and returns where.
pub fn save(dir: &Path, template: &Template) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = if template.path.as_os_str().is_empty() { new_path(dir, &template.name) } else { template.path.clone() };
    std::fs::write(&path, render(template))?;
    Ok(path)
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
    fn a_saved_template_reads_back_the_same_and_gets_a_file_named_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let template = Template {
            path: PathBuf::new(),
            name: "Review MR".into(),
            folder: Some("~/Work/app".into()),
            account: None,
            model: Some("haiku".into()),
            prompt: "Review {branch}.\n\nThen list blockers.".into(),
        };
        let path = save(dir.path(), &template).unwrap();
        assert_eq!(path, dir.path().join("review-mr.md"));
        assert_eq!(parse(&path, &std::fs::read_to_string(&path).unwrap()), Template { path: path.clone(), ..template.clone() });
        assert_eq!(save(dir.path(), &template).unwrap(), dir.path().join("review-mr-2.md"), "a second new one doesn't overwrite the first");
        let changed = Template { path: path.clone(), prompt: "Anders.".into(), ..template };
        assert_eq!(save(dir.path(), &changed).unwrap(), path, "an existing one is written in place");
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
