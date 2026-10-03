//! tmux: processes in tmux are children of the tmux server. The pane is found
//! by the process's tty, the terminal showing it through the attached client.

use std::process::Command;
use std::time::Duration;

use super::{Key, Outcome};

const PANE_FORMAT: &str = "#{pane_tty}\t#{session_id}:#{window_id}.#{pane_id}\t#{session_name}";
const CLIENT_FORMAT: &str = "#{client_tty}\t#{client_pid}\t#{client_activity}\t#{client_session}";

/// The pane a process runs in, on the server it belongs to.
pub struct Pane {
    /// `-L name` / `-S path` the server was started with, passed to every command.
    socket: Vec<String>,
    /// `$session:@window.%pane`, valid as target-window and target-pane.
    pub target: String,
    pub session: String,
    pub client: Option<Client>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub tty: String,
    pub pid: u32,
    activity: u64,
    session: String,
}

/// Finds the pane with `tty` on the server `server_pid`.
pub fn locate(server_pid: u32, tty: &str) -> Option<Pane> {
    let socket = socket_args(&command_line(server_pid)?);
    let panes = run(&socket, &["list-panes", "-a", "-F", PANE_FORMAT]).ok()?;
    let (target, session) = find_pane(&panes, tty)?;
    let clients = run(&socket, &["list-clients", "-F", CLIENT_FORMAT]).unwrap_or_default();
    let client = active_client(&parse_clients(&clients), &session);
    Some(Pane { socket, target, session, client })
}

impl Pane {
    /// Makes the pane the current one in its session.
    pub fn select(&self) -> Result<(), String> {
        run(&self.socket, &["select-window", "-t", &self.target])?;
        run(&self.socket, &["select-pane", "-t", &self.target]).map(|_| ())
    }

    /// Literal text, then Enter as a separate key so it is not part of a paste.
    pub fn type_text(&self, text: &str) -> Outcome {
        if let Err(err) = run(&self.socket, &["send-keys", "-t", &self.target, "-l", "--", text]) {
            return Outcome::Failed(err);
        }
        std::thread::sleep(Duration::from_millis(150));
        self.send_key(Key::Return)
    }

    pub fn send_key(&self, key: Key) -> Outcome {
        let name = match key {
            Key::Return => "Enter",
            Key::Escape => "Escape",
        };
        match run(&self.socket, &["send-keys", "-t", &self.target, name]) {
            Ok(_) => Outcome::Done,
            Err(err) => Outcome::Failed(err),
        }
    }
}

fn command_line(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-o", "args=", "-p", &pid.to_string()]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|line| !line.is_empty())
}

fn run(socket: &[String], args: &[&str]) -> Result<String, String> {
    let out = Command::new("tmux").args(socket).args(args).output().map_err(|err| format!("tmux: {err}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("tmux: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The server process keeps the command line of the client that started it,
/// e.g. `tmux -L work new-session -d`; the socket options are among the
/// global flags before the command.
pub fn socket_args(command_line: &str) -> Vec<String> {
    let mut words = command_line.split_whitespace().skip(1);
    let mut socket = Vec::new();
    while let Some(word) = words.next() {
        match word {
            "-L" | "-S" => {
                socket.push(word.to_string());
                socket.extend(words.next().map(str::to_string));
            }
            "-c" | "-f" | "-T" => {
                words.next();
            }
            _ if word.starts_with("-L") || word.starts_with("-S") => socket.push(word.to_string()),
            _ if word.starts_with('-') => {}
            _ => break,
        }
    }
    socket
}

/// `(target, session)` of the pane whose tty is `tty`, from `list-panes -a -F PANE_FORMAT`.
fn find_pane(list_panes: &str, tty: &str) -> Option<(String, String)> {
    list_panes.lines().find_map(|line| {
        let mut fields = line.splitn(3, '\t');
        (fields.next()? == tty).then_some(())?;
        Some((fields.next()?.to_string(), fields.next()?.to_string()))
    })
}

fn parse_clients(list_clients: &str) -> Vec<Client> {
    list_clients
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\t');
            Some(Client {
                tty: fields.next()?.to_string(),
                pid: fields.next()?.parse().ok()?,
                activity: fields.next()?.parse().ok()?,
                session: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// The most recently active client attached to `session`.
fn active_client(clients: &[Client], session: &str) -> Option<Client> {
    clients.iter().filter(|c| c.session == session).max_by_key(|c| c.activity).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_pane_and_the_most_recent_client_of_its_session() {
        let panes = "/dev/ttys014\t$0:@0.%0\twork\n/dev/ttys022\t$1:@3.%5\tmy session\n";
        assert_eq!(find_pane(panes, "/dev/ttys022"), Some(("$1:@3.%5".into(), "my session".into())));
        assert_eq!(find_pane(panes, "/dev/ttys099"), None);

        let clients = parse_clients(
            "/dev/ttys030\t111\t1759480000\tmy session\n/dev/ttys031\t222\t1759489999\twork\n/dev/ttys032\t333\t1759485000\tmy session\n",
        );
        assert_eq!(active_client(&clients, "my session").map(|c| (c.tty, c.pid)), Some(("/dev/ttys032".into(), 333)));
        assert_eq!(active_client(&clients, "other"), None);
    }

    #[test]
    fn reads_the_socket_from_the_server_command_line() {
        assert_eq!(socket_args("tmux -L brain-test new-session -d -s x sleep 600"), ["-L", "brain-test"]);
        assert_eq!(socket_args("/opt/homebrew/bin/tmux -2 -f ~/.tmux.conf -S /tmp/s attach"), ["-S", "/tmp/s"]);
        assert_eq!(socket_args("tmux -Lwork"), ["-Lwork"]);
        // Flags after the command belong to the command.
        assert!(socket_args("tmux new-session -L x").is_empty());
        assert!(socket_args("tmux").is_empty());
    }
}
