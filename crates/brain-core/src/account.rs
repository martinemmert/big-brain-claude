use std::path::{Path, PathBuf};

/// A Claude Code configuration directory, e.g. `~/.claude` or `~/.claude-second`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Short label shown in the UI: `main` for `~/.claude`, otherwise the
    /// directory name without the `.claude-` prefix (`second`).
    pub id: String,
    pub config_dir: PathBuf,
}

impl Account {
    pub fn from_config_dir(config_dir: PathBuf) -> Self {
        let id = account_id_for(&config_dir);
        Self { id, config_dir }
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.config_dir.join("sessions")
    }

    pub fn session_file(&self, pid: u32) -> PathBuf {
        self.sessions_dir().join(format!("{pid}.json"))
    }
}

fn account_id_for(config_dir: &Path) -> String {
    let name = config_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name == ".claude" {
        return "main".to_string();
    }
    if let Some(rest) = name.strip_prefix(".claude-") {
        return rest.to_string();
    }
    name.trim_start_matches('.').to_string()
}

/// Every `~/.claude` and `~/.claude-*` directory that has a `sessions` folder.
/// `main` comes first, the rest alphabetically.
pub fn discover_accounts(home: &Path) -> Vec<Account> {
    let mut accounts: Vec<Account> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            matches!(name, Some(n) if n == ".claude" || n.starts_with(".claude-"))
                && path.join("sessions").is_dir()
        })
        .map(Account::from_config_dir)
        .collect();
    accounts.sort_by(|a, b| (a.id != "main", &a.id).cmp(&(b.id != "main", &b.id)));
    accounts
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME is not set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_account_ids_from_directory_names() {
        assert_eq!(account_id_for(Path::new("/u/.claude")), "main");
        assert_eq!(account_id_for(Path::new("/u/.claude-second")), "second");
        assert_eq!(account_id_for(Path::new("/u/.work")), "work");
    }

    #[test]
    fn discovers_only_claude_dirs_with_sessions_main_first() {
        let home = tempfile::tempdir().unwrap();
        for dir in [".claude-second/sessions", ".claude/sessions", ".claude-empty", ".config/sessions"] {
            std::fs::create_dir_all(home.path().join(dir)).unwrap();
        }

        let ids: Vec<String> = discover_accounts(home.path()).into_iter().map(|a| a.id).collect();

        assert_eq!(ids, vec!["main", "second"]);
    }
}
