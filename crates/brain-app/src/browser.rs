//! Links open in the Chrome profile of the session's account: the arguments for `open`.

use brain_core::browser::{chrome_profiles, profile_for, CHROME_APP};

/// The arguments for `open`, or none when the URL isn't http(s). Without a profile: the default
/// browser. With one: a new Chrome instance (`-n`) with `--profile-directory`; a running Chrome
/// takes the URL over into that profile as a tab and the new instance ends. Without `-n`, `open`
/// drops the arguments of an app that already runs.
pub fn open_args(url: &str, profile: Option<&str>) -> Option<Vec<String>> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return None;
    }
    Some(match profile {
        Some(dir) => ["-n", "-a", CHROME_APP, "--args"].map(String::from).into_iter().chain([format!("--profile-directory={dir}"), url.to_string()]).collect(),
        None => vec![url.to_string()],
    })
}

/// Opens a web link, in the Chrome profile of the account when it has one. False when the URL
/// isn't http(s) or `open` didn't start.
pub fn open_url(url: &str, account: Option<&str>) -> bool {
    let profile = account.and_then(|a| profile_for(&crate::config::path(), a, &chrome_profiles()));
    let Some(args) = open_args(url, profile.as_deref()) else { return false };
    std::process::Command::new("open").args(args).spawn().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_arguments() {
        assert_eq!(open_args("https://claude.ai/artifact/a", None).unwrap(), ["https://claude.ai/artifact/a"]);
        assert_eq!(
            open_args("https://claude.ai/artifact/a", Some("Profile 9")).unwrap(),
            ["-n", "-a", "Google Chrome", "--args", "--profile-directory=Profile 9", "https://claude.ai/artifact/a"]
        );
        let odd = r#"https://x.test/a b?q="1" 'x'"#;
        assert_eq!(open_args(odd, Some("Profile 9")).unwrap().last().unwrap(), odd);
        assert!(open_args("file:///etc/passwd", Some("Profile 9")).is_none());
        assert!(open_args("-a Terminal", None).is_none());
    }
}
