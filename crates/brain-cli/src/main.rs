mod sessions;

use std::io::Read;
use std::process::ExitCode;

use brain_core::account::{discover_accounts, home_dir, Account};
use brain_core::event::{Event, Kind, Source, PROTOCOL_VERSION};
use brain_core::hook::{one_line, HookPayload};
use brain_core::install::{self, Change};
use brain_core::process::{find_claude_session, pid_alive, ProcessTable};
use brain_core::sessions::read_session_files;
use brain_core::state::{Board, Phase};
use brain_core::store::{Store, Tail};
use brain_terminal::Outcome;
use chrono::Utc;
use clap::{ArgGroup, Parser, Subcommand};
use sessions::Target;

#[derive(Parser)]
#[command(name = "brain", about = "Status protocol for the Claude Brain dashboard")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Called by Claude Code hooks; reads the hook JSON from stdin.
    Hook,
    /// Report what this Claude session is doing or waiting for.
    #[command(group(ArgGroup::new("kind").required(true).args(["doing", "waiting", "done"])))]
    Report {
        #[arg(long)]
        doing: bool,
        #[arg(long)]
        waiting: bool,
        #[arg(long)]
        done: bool,
        /// One line of text.
        text: Vec<String>,
    },
    /// Install hooks and the protocol section into every Claude account.
    Install,
    /// Remove Brain's hooks and protocol section from every Claude account.
    Uninstall,
    /// Print the current board.
    Status,
    /// List the sessions that have not ended, in triage order.
    #[command(group(ArgGroup::new("format").required(true).args(["alfred", "json"])))]
    Sessions {
        /// As Alfred Script Filter JSON.
        #[arg(long)]
        alfred: bool,
        /// As a JSON array (account, pid, name, phase, headline, cwd).
        #[arg(long)]
        json: bool,
    },
    /// Bring the terminal of a session to the front.
    Open {
        /// The session as <account>:<pid>, e.g. main:4242.
        session: String,
    },
    /// Show a session in Brain.app.
    Show {
        /// The session as <account>:<pid>, e.g. main:4242.
        session: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let home = home_dir();
    let store = Store::new(Store::default_root(&home));
    let accounts = discover_accounts(&home);

    match cli.command {
        Command::Hook => {
            // Hooks must never disturb Claude: no output, always success.
            let _ = run_hook(&store, &accounts);
            ExitCode::SUCCESS
        }
        Command::Report { doing, waiting, done, text } => {
            let kind = if waiting {
                Kind::Waiting
            } else if done {
                Kind::Done
            } else {
                debug_assert!(doing);
                Kind::Doing
            };
            run_report(&store, &accounts, kind, &text.join(" "))
        }
        Command::Install => run_install(&accounts),
        Command::Uninstall => run_uninstall(&accounts),
        Command::Status => run_status(&store, &accounts),
        Command::Sessions { alfred, json: _ } => run_sessions(&store, &accounts, alfred),
        Command::Open { session } => run_open(&accounts, &session),
        Command::Show { session } => run_show(&session),
    }
}

fn run_hook(store: &Store, accounts: &[Account]) -> Option<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok()?;
    capture_raw_hook(&input);
    let payload: HookPayload = serde_json::from_str(&input).ok()?;
    let kind = payload.kind()?;

    let session_id = payload.session_id.as_deref();
    let table = ProcessTable::snapshot();
    // At `SessionEnd` the session file may already be gone; the earlier events
    // of the same session still know its pid.
    let (account, pid) = find_claude_session(&table, std::process::id(), accounts, session_id)
        .or_else(|| find_by_session_id(accounts, session_id?))
        .or_else(|| find_in_events(store, accounts, session_id?))?;

    store
        .append(&Event {
            v: PROTOCOL_VERSION,
            ts: Utc::now(),
            account: account.id,
            pid,
            session_id: payload.session_id.clone(),
            cwd: payload.cwd.clone(),
            source: Source::Hook,
            kind,
            text: payload.text(),
        })
        .ok()
}

/// Diagnostics: when `~/.claude-brain/capture-hooks/` exists, every raw hook payload is appended
/// there (one JSON line per payload, a file per day). Delete the folder to stop.
fn capture_raw_hook(input: &str) {
    let dir = home_dir().join(".claude-brain/capture-hooks");
    if !dir.is_dir() {
        return;
    }
    let file = dir.join(format!("{}.jsonl", chrono::Local::now().format("%Y-%m-%d")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        use std::io::Write;
        let _ = writeln!(f, "{}", input.trim());
    }
}

fn find_by_session_id(accounts: &[Account], session_id: &str) -> Option<(Account, u32)> {
    accounts.iter().find_map(|account| {
        read_session_files(account)
            .into_iter()
            .find(|f| f.session_id.as_deref() == Some(session_id))
            .map(|f| (account.clone(), f.pid))
    })
}

fn find_in_events(store: &Store, accounts: &[Account], session_id: &str) -> Option<(Account, u32)> {
    let today = chrono::Local::now().date_naive();
    let last = Tail::new(store.file_for(today))
        .read_new()
        .into_iter()
        .rev()
        .find(|e| e.session_id.as_deref() == Some(session_id))?;
    let account = accounts.iter().find(|a| a.id == last.account)?.clone();
    Some((account, last.pid))
}

fn run_report(store: &Store, accounts: &[Account], kind: Kind, text: &str) -> ExitCode {
    let text = one_line(text, 400);
    if text.is_empty() {
        eprintln!("brain report: text is empty");
        return ExitCode::FAILURE;
    }
    let table = ProcessTable::snapshot();
    let Some((account, pid)) = find_claude_session(&table, std::process::id(), accounts, None) else {
        eprintln!("brain report: not running inside a Claude Code session");
        return ExitCode::FAILURE;
    };
    let session = read_session_files(&account).into_iter().find(|f| f.pid == pid);
    let event = Event {
        v: PROTOCOL_VERSION,
        ts: Utc::now(),
        account: account.id.clone(),
        pid,
        session_id: session.as_ref().and_then(|s| s.session_id.clone()),
        cwd: std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()),
        source: Source::Report,
        kind,
        text: Some(text),
    };
    match store.append(&event) {
        Ok(()) => {
            println!("brain: reported to {} (pid {pid})", account.id);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("brain report: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_install(accounts: &[Account]) -> ExitCode {
    let exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(exe) => exe,
        Err(err) => {
            eprintln!("brain install: cannot resolve own path: {err}");
            return ExitCode::FAILURE;
        }
    };
    let hook_command = format!("{} hook", shell_quote(&exe.to_string_lossy()));
    let mut failed = false;

    for account in accounts {
        println!("{} ({})", account.id, account.config_dir.display());

        let settings = account.config_dir.join("settings.json");
        let settings_text = std::fs::read_to_string(&settings).unwrap_or_default();
        match install::patch_settings_text(&settings_text, &hook_command) {
            Ok(patched) => report_change("settings.json", install::rewrite_file(&settings, |_| patched), UP_TO_DATE),
            Err(err) => {
                failed = true;
                println!("  settings.json  ✗ not valid JSON, left untouched: {err}");
            }
        }

        let claude_md = account.config_dir.join("CLAUDE.md");
        report_change("CLAUDE.md", install::rewrite_file(&claude_md, install::patch_claude_md), UP_TO_DATE);
    }

    if accounts.is_empty() {
        eprintln!("brain install: no ~/.claude or ~/.claude-* directories found");
        return ExitCode::FAILURE;
    }
    println!("\nHooks: {}\nNew Claude sessions pick this up on start.", install::HOOK_EVENTS.join(", "));
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn run_uninstall(accounts: &[Account]) -> ExitCode {
    let mut failed = false;

    for account in accounts {
        println!("{} ({})", account.id, account.config_dir.display());

        let settings = account.config_dir.join("settings.json");
        match std::fs::read_to_string(&settings) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => println!("  {:<14} ✓ {NOTHING_TO_REMOVE}", "settings.json"),
            Err(err) => {
                failed = true;
                println!("  settings.json  ✗ {err}");
            }
            Ok(text) => match install::unpatch_settings_text(&text) {
                Ok(patched) => {
                    report_change("settings.json", install::rewrite_file(&settings, |_| patched), NOTHING_TO_REMOVE)
                }
                Err(err) => {
                    failed = true;
                    println!("  settings.json  ✗ not valid JSON, left untouched: {err}");
                }
            },
        }

        let claude_md = account.config_dir.join("CLAUDE.md");
        if claude_md.exists() {
            report_change("CLAUDE.md", install::rewrite_file(&claude_md, install::unpatch_claude_md), NOTHING_TO_REMOVE);
        } else {
            println!("  {:<14} ✓ {NOTHING_TO_REMOVE}", "CLAUDE.md");
        }
    }

    if accounts.is_empty() {
        eprintln!("brain uninstall: no ~/.claude or ~/.claude-* directories found");
        return ExitCode::FAILURE;
    }
    println!("\nBrain.app, the brain binary and ~/.claude-brain are left in place.");
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

const UP_TO_DATE: &str = "already up to date";
const NOTHING_TO_REMOVE: &str = "nothing to remove";

fn report_change(label: &str, result: std::io::Result<Change>, unchanged: &str) {
    match result {
        Ok(Change::Unchanged) => println!("  {label:<14} ✓ {unchanged}"),
        Ok(Change::Updated { backup: Some(b) }) => println!("  {label:<14} ✓ updated (backup: {})", b.display()),
        Ok(Change::Updated { backup: None }) => println!("  {label:<14} ✓ created"),
        Err(err) => println!("  {label:<14} ✗ {err}"),
    }
}

fn shell_quote(path: &str) -> String {
    if path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// The board from today's events and the session files of every account.
fn load_board(store: &Store, accounts: &[Account]) -> Board {
    let mut board = Board::default();
    for event in Tail::new(store.file_for(chrono::Local::now().date_naive())).read_new() {
        board.apply_event(&event);
    }
    for account in accounts {
        for file in read_session_files(account) {
            board.apply_session_file(&account.id, &file, pid_alive(file.pid));
        }
    }
    board
}

fn run_status(store: &Store, accounts: &[Account]) -> ExitCode {
    let board = load_board(store, accounts);
    for session in board.sorted().into_iter().filter(|s| s.phase() != Phase::Ended) {
        let marker = match session.phase() {
            Phase::NeedsYou => "🔴",
            Phase::YourTurn => "🟡",
            Phase::Working => "🔵",
            Phase::Ended => "⚫",
        };
        println!(
            "{marker} {:<28} {:<7} {}",
            session.display_name(),
            session.key.account,
            session.headline().unwrap_or_default()
        );
    }
    ExitCode::SUCCESS
}

fn run_sessions(store: &Store, accounts: &[Account], alfred: bool) -> ExitCode {
    let board = load_board(store, accounts);
    let open: Vec<_> = board.sorted().into_iter().filter(|s| s.phase() != Phase::Ended).collect();
    let output = if alfred { sessions::alfred_items(&open) } else { sessions::json_list(&open) };
    println!("{output}");
    ExitCode::SUCCESS
}

fn run_open(accounts: &[Account], session: &str) -> ExitCode {
    let target = match Target::parse(session) {
        Ok(target) => target,
        Err(err) => return fail("brain open", &err),
    };
    let Some(account) = accounts.iter().find(|a| a.id == target.account) else {
        return fail("brain open", &format!("no Claude account {:?}", target.account));
    };
    let Some(file) = read_session_files(account).into_iter().find(|f| f.pid == target.pid) else {
        return fail("brain open", &format!("no session {target}"));
    };
    if !pid_alive(file.pid) {
        return fail("brain open", &format!("session {target} has ended"));
    }
    match brain_terminal::focus(file.pid, file.cwd.as_deref()) {
        Outcome::Done => ExitCode::SUCCESS,
        Outcome::NoTerminal => fail("brain open", &format!("session {target} has no terminal")),
        Outcome::Unsupported(msg) | Outcome::Failed(msg) => fail("brain open", &format!("session {target}: {msg}")),
    }
}

/// Hands `brain://session/<account>/<pid>` to Brain.app.
fn run_show(session: &str) -> ExitCode {
    let target = match Target::parse(session) {
        Ok(target) => target,
        Err(err) => return fail("brain show", &err),
    };
    let url = format!("brain://session/{}/{}", target.account, target.pid);
    match std::process::Command::new("open").arg(&url).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => fail("brain show", &format!("could not open {url} (is Brain.app installed?)")),
        Err(err) => fail("brain show", &format!("could not run open: {err}")),
    }
}

fn fail(command: &str, message: &str) -> ExitCode {
    eprintln!("{command}: {message}");
    ExitCode::FAILURE
}
