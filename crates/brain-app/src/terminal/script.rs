//! Quoting for AppleScript and the shell, and running AppleScript.

use std::process::Command;

use super::Outcome;

/// An AppleScript string literal.
pub fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A single-quoted POSIX shell word.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `cd <cwd> && CLAUDE_CONFIG_DIR=<dir> <command>`; `command` is used as is.
pub fn shell_command(cwd: &str, config_dir: Option<&str>, command: &str) -> String {
    match config_dir {
        Some(dir) => format!("cd {} && CLAUDE_CONFIG_DIR={} {command}", shell_quote(cwd), shell_quote(dir)),
        None => format!("cd {} && {command}", shell_quote(cwd)),
    }
}

/// Runs a script that returns "ok" on success and anything else when the
/// target was not found; `missing` describes that case.
pub fn run_osascript(script: &str, missing: impl FnOnce() -> String) -> Outcome {
    match Command::new("osascript").args(["-e", script]).output() {
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "ok" => Outcome::Done,
        Ok(out) if out.status.success() => Outcome::Failed(missing()),
        Ok(out) => Outcome::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(err) => Outcome::Failed(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_shell_and_applescript() {
        let command = shell_command("/w/it's here", Some("/Users/me/.claude two"), "claude --resume abc");
        assert_eq!(command, r"cd '/w/it'\''s here' && CLAUDE_CONFIG_DIR='/Users/me/.claude two' claude --resume abc");
        assert_eq!(shell_command("/w", None, "claude"), "cd '/w' && claude");
        assert_eq!(applescript_string(r#"say "hi" \ bye"#), r#""say \"hi\" \\ bye""#);
    }
}
