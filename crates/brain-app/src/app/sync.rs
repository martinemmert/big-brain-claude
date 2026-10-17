//! ⌘K "Sync accounts": what `brain_core::sync` would bring in step, where the accounts
//! conflict, and the user's choice for each. Memory notes are only counted: they never wait.

use brain_core::account::home_dir;
use brain_core::sync::{self, Decision, Item, Kind, Outcome, Plan};
use iced::keyboard::key::Named;
use iced::Task;

use super::{off_thread, Brain, Message, Mode};
use crate::tr;

#[derive(Default)]
pub struct SyncDialog {
    pub plan: Option<Plan>,
    /// Per row: whose version goes everywhere, and whether the accounts with another one keep
    /// theirs.
    pub choices: Vec<SyncChoice>,
    pub selected: usize,
    pub busy: bool,
    pub error: Option<String>,
    /// Conflicts the last sync left open (a session start doesn't decide them).
    pub open: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncChoice {
    pub take: String,
    pub keep: bool,
}

impl SyncDialog {
    /// The items the dialog lists one by one: conflicts first, then what is taken over.
    pub fn rows(&self) -> Vec<&Item> {
        self.plan.iter().flat_map(|p| p.items.iter().filter(|i| i.kind != Kind::Memory)).collect()
    }

    /// The memory notes and index lines taken over, and in how many projects.
    pub fn memory(&self) -> (usize, usize) {
        let items: Vec<&Item> = self.plan.iter().flat_map(|p| p.items.iter().filter(|i| i.kind == Kind::Memory)).collect();
        let projects: std::collections::BTreeSet<&str> = items.iter().map(|i| i.label.split(':').next().unwrap_or_default()).collect();
        (items.len(), projects.len())
    }

    fn decisions(&self) -> Vec<Decision> {
        self.rows()
            .into_iter()
            .zip(&self.choices)
            .filter(|(item, choice)| item.conflict || choice.take != item.take || choice.keep)
            .map(|(item, choice)| {
                let taken = item.versions.iter().find(|v| v.account == choice.take).map(|v| &v.value);
                let keep = if choice.keep { item.versions.iter().filter(|v| Some(&v.value) != taken).map(|v| v.account.clone()).collect() } else { Vec::new() };
                Decision { id: item.id.clone(), take: choice.take.clone(), keep }
            })
            .collect()
    }
}

impl Brain {
    pub(super) fn open_sync(&mut self) -> Task<Message> {
        if self.blocked_in_demo() {
            return Task::none();
        }
        self.sync = SyncDialog { open: self.sync.open, ..SyncDialog::default() };
        self.mode = Mode::Sync;
        let accounts = self.model.accounts.clone();
        Task::perform(
            off_thread(move || {
                let home = home_dir();
                sync::plan(&sync::default_dir(&home), &accounts, &home)
            }),
            Message::SyncPlanned,
        )
    }

    pub(super) fn sync_planned(&mut self, plan: Plan) {
        self.sync.plan = Some(plan);
        self.sync.choices = self.sync.rows().iter().map(|i| SyncChoice { take: i.take.clone(), keep: false }).collect();
    }

    /// A click on an account's version, or its digit: that version goes everywhere.
    pub(super) fn sync_pick(&mut self, row: usize, account: String) {
        if let Some(choice) = self.sync.choices.get_mut(row) {
            choice.take = account;
        }
        self.sync.selected = row;
    }

    /// K: the accounts with another version keep theirs from now on.
    pub(super) fn sync_keep(&mut self, row: usize) {
        if let Some(choice) = self.sync.choices.get_mut(row) {
            choice.keep = !choice.keep;
        }
        self.sync.selected = row;
    }

    pub(super) fn on_sync_key(&mut self, named: Option<Named>, character: Option<&str>) -> Task<Message> {
        let rows = self.sync.rows().len();
        match (named, character) {
            (Some(Named::ArrowDown), _) if self.sync.selected + 1 < rows => self.sync.selected += 1,
            (Some(Named::ArrowUp), _) => self.sync.selected = self.sync.selected.saturating_sub(1),
            (Some(Named::Enter), _) => return self.sync_apply(),
            (_, Some("k")) => self.sync_keep(self.sync.selected),
            (_, Some(c)) => {
                let row = self.sync.selected;
                let account = c.parse::<usize>().ok().filter(|d| *d >= 1).and_then(|d| Some(self.sync.rows().get(row)?.versions.get(d - 1)?.account.clone()));
                if let Some(account) = account {
                    self.sync_pick(row, account);
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// ⏎: everything as shown, conflicts with the version chosen (or suggested).
    pub(super) fn sync_apply(&mut self) -> Task<Message> {
        let Some(plan) = self.sync.plan.clone() else { return Task::none() };
        if self.sync.busy || self.blocked_in_demo() {
            return Task::none();
        }
        self.sync.busy = true;
        let decisions = self.sync.decisions();
        let accounts = self.model.accounts.clone();
        Task::perform(
            off_thread(move || {
                let home = home_dir();
                sync::apply(&sync::default_dir(&home), &accounts, &home, &plan, &decisions)
            }),
            Message::SyncDone,
        )
    }

    pub(super) fn sync_done(&mut self, result: Result<Outcome, String>) {
        self.sync.busy = false;
        match result {
            Ok(outcome) => {
                self.sync.open = outcome.open;
                self.mode = Mode::Normal;
                let synced = outcome.synced;
                self.set_status(tr!("Konten abgeglichen: {synced} Einträge.", "Accounts synced: {synced} items."));
            }
            Err(err) => self.sync.error = Some(err),
        }
    }

    /// On every refresh: conflicts a session start left open, for the title bar.
    pub(super) fn refresh_sync_conflicts(&mut self) {
        self.sync.open = sync::open_conflicts(&sync::default_dir(&home_dir()));
    }
}
