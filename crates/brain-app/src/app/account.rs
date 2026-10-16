//! ⌘K "Set up a new account": a Claude account folder made from the existing ones' setup and
//! memory. The preview comes from `brain_core::account_setup`; creating it runs
//! `brain account new <name> --yes`, the same way as on the command line (Brain's hooks
//! included), so there is one way to make an account.

use brain_core::account::home_dir;
use brain_core::account_setup::{self, Plan};
use iced::widget::operation;
use iced::Task;

use super::{off_thread, Brain, Message, Mode};
use crate::i18n::t;
use crate::tr;

/// The name field of the dialog.
pub const ACCOUNT_NAME: &str = "account-name";

#[derive(Default)]
pub struct AccountSetup {
    pub name: String,
    /// The preview for `name`, or why the name doesn't work.
    pub plan: Option<Result<Plan, String>>,
    /// Counts previews, so a slow one for an older name doesn't replace a newer one.
    asked: u64,
    /// The account is being made.
    pub busy: bool,
}

impl Brain {
    pub(super) fn open_account_setup(&mut self) -> Task<Message> {
        if self.blocked_in_demo() {
            return Task::none();
        }
        self.account_setup = AccountSetup::default();
        self.mode = Mode::AccountSetup;
        operation::focus(ACCOUNT_NAME)
    }

    /// The name changed: check it and work out what the account would get, off the UI thread
    /// (it reads every account's skills, plugins and memory).
    pub(super) fn account_setup_name(&mut self, name: String) -> Task<Message> {
        self.account_setup.name = name.trim().to_lowercase();
        self.account_setup.asked += 1;
        let asked = self.account_setup.asked;
        let home = home_dir();
        let target = match account_setup::target_dir(&home, &self.account_setup.name) {
            Ok(target) => target,
            Err(err) => {
                self.account_setup.plan = (!self.account_setup.name.is_empty()).then_some(Err(err));
                return Task::none();
            }
        };
        let sources = self.model.accounts.clone();
        Task::perform(off_thread(move || account_setup::plan(&sources, &home, &target)), move |plan| Message::AccountSetupPlanned(asked, plan))
    }

    pub(super) fn account_setup_planned(&mut self, asked: u64, plan: Plan) {
        if asked == self.account_setup.asked {
            self.account_setup.plan = Some(Ok(plan));
        }
    }

    /// ⏎ or "Create": `brain account new <name> --yes`.
    pub(super) fn account_setup_create(&mut self) -> Task<Message> {
        let ready = matches!(self.account_setup.plan, Some(Ok(_)));
        if !ready || self.account_setup.busy || self.blocked_in_demo() {
            return Task::none();
        }
        self.account_setup.busy = true;
        let name = self.account_setup.name.clone();
        Task::perform(off_thread(move || run_brain(&["account", "new", &name, "--yes"])), Message::AccountSetupDone)
    }

    /// Made: Brain lists the account, and the new-session dialog opens on it — its first session
    /// asks for the login.
    pub(super) fn account_setup_done(&mut self, result: Result<String, String>) -> Task<Message> {
        self.account_setup.busy = false;
        match result {
            Ok(_) => {
                let name = std::mem::take(&mut self.account_setup.name);
                self.model.reload_accounts();
                self.set_status(tr!(
                    "Konto „{name}“ eingerichtet – starte eine Session darin und melde dich mit /login an.",
                    "Account “{name}” set up – start a session in it and log in with /login."
                ));
                let task = self.open_new_session_dialog();
                if let Some(index) = self.model.accounts.iter().position(|a| a.id == name) {
                    self.new_session.account = index;
                }
                task
            }
            Err(err) => {
                self.set_status(tr!("Konto nicht eingerichtet: {err}", "Account not set up: {err}"));
                Task::none()
            }
        }
    }
}

/// Runs the `brain` CLI: the one on the PATH first (Brain's hooks point at it), else the one in
/// the app bundle. Its output on success, its error otherwise.
fn run_brain(args: &[&str]) -> Result<String, String> {
    let bundled = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("brain")));
    let mut last = t("brain nicht gefunden", "brain not found").to_string();
    for program in std::iter::once(std::path::PathBuf::from("brain")).chain(bundled) {
        match std::process::Command::new(&program).args(args).output() {
            Ok(out) if out.status.success() => return Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
            Ok(out) => {
                let text = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().to_string();
                return Err(brain_core::hook::one_line(&text, 200));
            }
            Err(err) => last = err.to_string(),
        }
    }
    Err(last)
}
