//! The desktop's trash: `gio trash`, else `kioclient` (KDE).

use std::path::Path;
use std::process::Command;

pub fn move_to_trash(path: &Path) -> Result<(), String> {
    let attempts: [(&str, &[&str]); 3] = [("gio", &["trash"]), ("kioclient", &["move"]), ("kioclient5", &["move"])];
    let mut last = String::from("no trash tool found (gio, kioclient)");
    for (program, args) in attempts {
        let mut command = Command::new(program);
        command.args(args).arg(path);
        if program != "gio" {
            command.arg("trash:/");
        }
        match command.output() {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => last = format!("{program}: {}", String::from_utf8_lossy(&out.stderr).trim()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => last = format!("{program}: {err}"),
        }
    }
    Err(last)
}
