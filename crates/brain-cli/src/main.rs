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
use chrono::Utc;
use clap::{ArgGroup, Parser, Subcommand};

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
    /// Print the current board.
    Status,
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
        Command::Status => run_status(&store, &accounts),
    }
}

fn run_hook(store: &Store, accounts: &[Account]) -> Option<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok()?;
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
            Ok(patched) => report_change("settings.json", install::rewrite_file(&settings, |_| patched)),
            Err(err) => {
                failed = true;
                println!("  settings.json  ✗ not valid JSON, left untouched: {err}");
            }
        }

        let claude_md = account.config_dir.join("CLAUDE.md");
        report_change("CLAUDE.md", install::rewrite_file(&claude_md, install::patch_claude_md));
    }

    if accounts.is_empty() {
        eprintln!("brain install: no ~/.claude or ~/.claude-* directories found");
        return ExitCode::FAILURE;
    }
    println!("\nHooks: {}\nNew Claude sessions pick this up on start.", install::HOOK_EVENTS.join(", "));
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn report_change(label: &str, result: std::io::Result<Change>) {
    match result {
        Ok(Change::Unchanged) => println!("  {label:<14} ✓ already up to date"),
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

fn run_status(store: &Store, accounts: &[Account]) -> ExitCode {
    let mut board = Board::default();
    for event in Tail::new(store.file_for(chrono::Local::now().date_naive())).read_new() {
        board.apply_event(&event);
    }
    for account in accounts {
        for file in read_session_files(account) {
            board.apply_session_file(&account.id, &file, pid_alive(file.pid));
        }
    }
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
