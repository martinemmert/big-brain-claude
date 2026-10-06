//! KDE Konsole over D-Bus: every Konsole process registers `org.kde.konsole-<pid>`, its tabs
//! are `/Sessions/<id>` and know the pid of their shell. Wayland lets no other app raise a
//! window, so focusing asks KWin through a short script. Typing needs Konsole's setting
//! "Enable the security sensitive parts of the DBus API"; focusing works without it.

use std::process::Command;
use std::time::Duration;

use zbus::blocking::Connection;

use super::{Key, Outcome};

const SESSION: &str = "org.kde.konsole.Session";
const WINDOW: &str = "org.kde.konsole.Window";

/// The tab whose shell is `shell_pid`, in the Konsole process `konsole_pid`.
pub struct Tab {
    connection: Connection,
    service: String,
    konsole_pid: u32,
    window: String,
    session: i32,
}

pub fn locate(konsole_pid: u32, shell_pid: u32) -> Option<Tab> {
    let connection = Connection::session().ok()?;
    let service = format!("org.kde.konsole-{konsole_pid}");
    let (window, session) = windows(&connection, &service).into_iter().find_map(|window| {
        let sessions: Vec<String> = call(&connection, &service, &window, WINDOW, "sessionList", &()).ok()?;
        let session = sessions.iter().filter_map(|id| id.parse::<i32>().ok()).find(|id| {
            call::<i32>(&connection, &service, &format!("/Sessions/{id}"), SESSION, "processId", &())
                .is_ok_and(|pid| pid as u32 == shell_pid)
        })?;
        Some((window, session))
    })?;
    Some(Tab { connection, service, konsole_pid, window, session })
}

impl Tab {
    /// Whether Konsole lets Brain type: sending nothing hits the same settings check.
    pub fn accepts_typing(&self) -> bool {
        self.send("").is_ok()
    }

    /// Selects the tab, then brings its window to the front.
    pub fn focus(&self) -> Outcome {
        if let Err(err) = self.call::<()>(&self.window, WINDOW, "setCurrentSession", &(self.session,)) {
            return Outcome::Failed(err);
        }
        let title: String = self.call(&self.session_path(), SESSION, "title", &(1,)).unwrap_or_default();
        activate_window(self.konsole_pid, &title)
    }

    /// Literal text, then Return as a separate write so it is not taken as a paste.
    pub fn type_text(&self, text: &str) -> Outcome {
        if let Err(err) = self.send(text) {
            return Outcome::Failed(err);
        }
        std::thread::sleep(Duration::from_millis(150));
        self.send_key(Key::Return)
    }

    pub fn send_key(&self, key: Key) -> Outcome {
        let text = match key {
            Key::Return => "\r",
            Key::Escape => "\x1b",
        };
        match self.send(text) {
            Ok(()) => Outcome::Done,
            Err(err) => Outcome::Failed(err),
        }
    }

    fn send(&self, text: &str) -> Result<(), String> {
        self.call(&self.session_path(), SESSION, "sendText", &(text,)).map_err(|err| {
            if err.contains("AccessDenied") {
                "Konsole blocks typing over D-Bus: turn on \"Enable the security sensitive parts of the DBus API\" \
                 in Konsole's settings (General), then restart Konsole windows opened before"
                    .into()
            } else {
                err
            }
        })
    }

    fn session_path(&self) -> String {
        format!("/Sessions/{}", self.session)
    }

    fn call<R: serde::de::DeserializeOwned + zbus::zvariant::Type>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        args: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> Result<R, String> {
        call(&self.connection, &self.service, path, interface, method, args)
    }
}

/// Runs `shell_command` in a new Konsole tab (a new window if no Konsole runs). Starting
/// Konsole needs none of the D-Bus calls that its settings may block.
pub fn open_new(cwd: &str, shell_command: &str) -> Outcome {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let spawned = Command::new("konsole")
        .args(["--new-tab", "--workdir", cwd, "-e", &shell, "-lc", &format!("{shell_command}; exec {shell} -l")])
        .spawn();
    match spawned {
        Ok(_) => Outcome::Done,
        Err(err) => Outcome::Failed(format!("konsole: {err}")),
    }
}

/// Object paths of the Konsole windows, e.g. `/Windows/1`.
fn windows(connection: &Connection, service: &str) -> Vec<String> {
    let xml: String =
        call(connection, service, "/Windows", "org.freedesktop.DBus.Introspectable", "Introspect", &()).unwrap_or_default();
    child_nodes(&xml).into_iter().map(|node| format!("/Windows/{node}")).collect()
}

/// The `<node name="…"/>` children in D-Bus introspection XML.
fn child_nodes(xml: &str) -> Vec<String> {
    xml.split("<node name=\"").skip(1).filter_map(|rest| Some(rest.split_once('"')?.0.to_string())).collect()
}

fn call<R: serde::de::DeserializeOwned + zbus::zvariant::Type>(
    connection: &Connection,
    service: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> Result<R, String> {
    let reply = connection.call_method(Some(service), path, Some(interface), method, args).map_err(|err| err.to_string())?;
    reply.body().deserialize().map_err(|err| err.to_string())
}

/// Activates the window of `pid` whose caption starts with `title` (or its only one) via a
/// KWin script; Plasma 5 names the calls differently.
fn activate_window(pid: u32, title: &str) -> Outcome {
    let script = format!(
        r#"const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
const mine = list.filter(w => w.pid === {pid} && w.normalWindow);
const w = mine.find(w => {title}.length > 0 && w.caption.startsWith({title})) || mine[0];
if (w) {{
    w.minimized = false;
    if ("activeWindow" in workspace) workspace.activeWindow = w; else workspace.activeClient = w;
}}"#,
        title = js_string(title),
    );
    match run_kwin_script(&script) {
        Ok(()) => Outcome::Done,
        Err(err) => Outcome::Failed(format!("KWin: {err}")),
    }
}

fn run_kwin_script(script: &str) -> Result<(), String> {
    const PLUGIN: &str = "brain-focus";
    let connection = Connection::session().map_err(|err| err.to_string())?;
    let path = std::env::temp_dir().join(format!("brain-focus-{}.js", std::process::id()));
    std::fs::write(&path, script).map_err(|err| err.to_string())?;
    // A script left over from an interrupted run would make loadScript fail.
    let unload = || call::<bool>(&connection, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", &(PLUGIN,));
    let _ = unload();
    let loaded: Result<i32, String> = call(
        &connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
        "loadScript",
        &(path.to_string_lossy().as_ref(), PLUGIN),
    );
    let result = loaded.and_then(|id| {
        call::<()>(&connection, "org.kde.KWin", &format!("/Scripting/Script{id}"), "org.kde.kwin.Script", "run", &())
    });
    let _ = unload();
    let _ = std::fs::remove_file(&path);
    result
}

/// A JavaScript string literal.
fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_child_nodes_from_introspection() {
        let xml = r#"<node><interface name="x"/><node name="1"/><node name="12"/></node>"#;
        assert_eq!(child_nodes(xml), ["1", "12"]);
        assert!(child_nodes("<node></node>").is_empty());
    }

    #[test]
    fn quotes_javascript_strings() {
        assert_eq!(js_string(r#"a "b" \ c"#), r#""a \"b\" \\ c""#);
        assert_eq!(js_string("x\ny"), r#""x\u000ay""#);
    }
}
