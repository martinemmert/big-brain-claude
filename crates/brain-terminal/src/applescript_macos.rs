//! Quoting for AppleScript, and running it.

use std::process::Command;

use super::Outcome;

/// An AppleScript string literal.
pub fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
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
    fn quotes_for_applescript() {
        assert_eq!(applescript_string(r#"say "hi" \ bye"#), r#""say \"hi\" \\ bye""#);
    }
}
