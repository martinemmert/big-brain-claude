//! Sessions that get in each other's way: editing the same files, or the same checkout.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::state::SessionKey;

/// What one live session needs to know about the others.
pub struct Work<'a> {
    pub key: SessionKey,
    pub checkout: Option<PathBuf>,
    /// Files the session edited, from its transcript.
    pub edited: &'a [String],
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Other sessions that edited the same files, with those files.
    pub shared_files: Vec<(SessionKey, Vec<String>)>,
    /// Other sessions that edit files in the same checkout (but not the same files).
    pub same_checkout: Vec<SessionKey>,
}

impl Conflict {
    pub fn is_empty(&self) -> bool {
        self.shared_files.is_empty() && self.same_checkout.is_empty()
    }
}

pub fn conflicts(work: &[Work]) -> HashMap<SessionKey, Conflict> {
    let mut out: HashMap<SessionKey, Conflict> = HashMap::new();
    for (i, a) in work.iter().enumerate() {
        for b in &work[i + 1..] {
            let a_files: BTreeSet<&String> = a.edited.iter().collect();
            let shared: Vec<String> = b.edited.iter().filter(|f| a_files.contains(f)).cloned().collect();
            let same_checkout = a.checkout.is_some()
                && a.checkout == b.checkout
                && !a.edited.is_empty()
                && !b.edited.is_empty();
            if !shared.is_empty() {
                out.entry(a.key.clone()).or_default().shared_files.push((b.key.clone(), shared.clone()));
                out.entry(b.key.clone()).or_default().shared_files.push((a.key.clone(), shared));
            } else if same_checkout {
                out.entry(a.key.clone()).or_default().same_checkout.push(b.key.clone());
                out.entry(b.key.clone()).or_default().same_checkout.push(a.key.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pid: u32) -> SessionKey {
        SessionKey { account: "main".into(), pid }
    }

    #[test]
    fn shared_files_beat_a_shared_checkout_and_separate_worktrees_never_conflict() {
        let app = Some(PathBuf::from("/w/app"));
        let worktree = Some(PathBuf::from("/w/app/.worktrees/fix"));
        let (one, two, three, four) = (
            vec!["/w/app/src/a.rs".to_string(), "/w/app/src/b.rs".to_string()],
            vec!["/w/app/src/b.rs".to_string()],
            vec!["/w/app/README.md".to_string()],
            vec!["/w/app/.worktrees/fix/src/a.rs".to_string()],
        );
        let work = [
            Work { key: key(1), checkout: app.clone(), edited: &one },
            Work { key: key(2), checkout: app.clone(), edited: &two },
            Work { key: key(3), checkout: app.clone(), edited: &three },
            Work { key: key(4), checkout: worktree, edited: &four },
        ];

        let found = conflicts(&work);

        assert_eq!(found[&key(1)].shared_files, vec![(key(2), vec!["/w/app/src/b.rs".to_string()])]);
        assert_eq!(found[&key(1)].same_checkout, vec![key(3)]);
        assert_eq!(found[&key(3)].same_checkout, vec![key(1), key(2)]);
        assert!(!found.contains_key(&key(4)));
    }
}
