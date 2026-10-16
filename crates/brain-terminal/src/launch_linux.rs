//! New terminal tabs or windows on Linux: Konsole first (Brain can control it), then the
//! GNOME terminals, then the system's default terminal.

use std::io::ErrorKind;
use std::process::Command;

use super::Outcome;

/// Runs `shell_command` in a login shell that stays open afterwards.
pub fn open_new(cwd: &str, shell_command: &str) -> Outcome {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let run = format!("{shell_command}; exec {shell} -l");
    let terminals: [(&str, &[&str]); 5] = [
        ("konsole", &["--new-tab", "--workdir", cwd, "-e"]),
        ("ptyxis", &["--tab", "--working-directory", cwd, "--"]),
        ("gnome-terminal", &["--tab", "--working-directory", cwd, "--"]),
        ("kgx", &["--tab", "--working-directory", cwd, "--"]),
        ("x-terminal-emulator", &["-e"]),
    ];
    for (program, args) in terminals {
        match Command::new(program).args(args).args([shell.as_str(), "-lc", &run]).current_dir(cwd).spawn() {
            Ok(_) => return Outcome::Done,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Outcome::Failed(format!("{program}: {err}")),
        }
    }
    Outcome::Unsupported("no terminal found (Konsole, Ptyxis, GNOME Terminal, GNOME Console, x-terminal-emulator)".into())
}
