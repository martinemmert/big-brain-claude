//! Quoting for the shell.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_the_shell() {
        let command = shell_command("/w/it's here", Some("/Users/me/.claude two"), "claude --resume abc");
        assert_eq!(command, r"cd '/w/it'\''s here' && CLAUDE_CONFIG_DIR='/Users/me/.claude two' claude --resume abc");
        assert_eq!(shell_command("/w", None, "claude"), "cd '/w' && claude");
    }
}
