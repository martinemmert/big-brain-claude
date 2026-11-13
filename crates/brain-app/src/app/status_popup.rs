//! The popup under the status item: what it lists (who needs you, who finished, the accounts'
//! limits) and opening and closing it. It floats over the main window; `ui` draws it.

use brain_core::state::{Ask, Phase, SessionKey};
use iced::Task;

use super::{Brain, Message};
use crate::format::{clip, plain};
use crate::i18n::t;
use crate::tr;
use crate::ui::{runs_out_label, usage_now, UsageNow};

/// Rows each list shows; the rest is a line "… und n weitere".
pub const SHOWN: usize = 5;

// Heights of the pieces, in points; the view gives each piece exactly this height.
pub const PADDING: f32 = 8.0;
pub const HEAD_H: f32 = 40.0;
pub const SECTION_H: f32 = 26.0;
pub const ROW_H: f32 = 50.0;
pub const BUTTONS_H: f32 = 31.0;
pub const MORE_H: f32 = 22.0;
pub const CALM_H: f32 = 34.0;
pub const SEPARATOR_H: f32 = 11.0;
pub const ACCOUNT_H: f32 = 22.0;
pub const ACCOUNT_GAP: f32 = 6.0;
pub const NOTE_H: f32 = 18.0;
pub const USAGE_PADDING: f32 = 4.0;

/// A session that needs you.
#[derive(Debug, Clone, PartialEq)]
pub struct NeedRow {
    pub key: SessionKey,
    pub name: String,
    pub project: String,
    pub since_ms: i64,
    /// The question or "Freigabe für Bash: npm run e2e".
    pub line: String,
    /// Allow and Deny answer it.
    pub permission: bool,
}

/// A session that finished and wasn't looked at.
#[derive(Debug, Clone, PartialEq)]
pub struct DoneRow {
    pub key: SessionKey,
    pub name: String,
    pub project: String,
    pub since_ms: i64,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    pub account: String,
    /// Which hue the account has in the tree.
    pub index: usize,
    pub usage: UsageNow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PopupContent {
    pub needs: Vec<NeedRow>,
    pub needs_total: usize,
    pub finished: Vec<DoneRow>,
    pub finished_total: usize,
    pub working: usize,
    pub accounts: Vec<AccountRow>,
    /// "main voll ~Do 14:00 · neue Sessions besser in second".
    pub advice: Option<String>,
}

impl PopupContent {
    /// Cuts both lists to what the popup shows and works out the advice.
    pub fn new(needs: Vec<NeedRow>, finished: Vec<DoneRow>, working: usize, accounts: Vec<AccountRow>, now_ms: i64) -> Self {
        let (needs_total, finished_total) = (needs.len(), finished.len());
        let advice = advice(&accounts, now_ms);
        Self {
            needs: needs.into_iter().take(SHOWN).collect(),
            needs_total,
            finished: finished.into_iter().take(SHOWN).collect(),
            finished_total,
            working,
            accounts,
            advice,
        }
    }

    /// Nobody needs you and nothing finished unseen.
    pub fn is_calm(&self) -> bool {
        self.needs_total == 0 && self.finished_total == 0
    }

    /// The window's height: exactly what the view lays out.
    pub fn height(&self) -> f32 {
        let mut height = PADDING * 2.0 + HEAD_H;
        if self.is_calm() {
            height += CALM_H;
        }
        if self.needs_total > 0 {
            height += SECTION_H;
            height += self.needs.iter().map(|r| ROW_H + if r.permission { BUTTONS_H } else { 0.0 }).sum::<f32>();
            if self.needs_total > self.needs.len() {
                height += MORE_H;
            }
        }
        if self.finished_total > 0 {
            height += SECTION_H + ROW_H * self.finished.len() as f32;
            if self.finished_total > self.finished.len() {
                height += MORE_H;
            }
        }
        if !self.accounts.is_empty() {
            height += SEPARATOR_H + SECTION_H + USAGE_PADDING * 2.0;
            height += self.accounts.len() as f32 * ACCOUNT_H + (self.accounts.len() as f32 - 1.0) * ACCOUNT_GAP;
            if self.advice.is_some() {
                height += ACCOUNT_GAP + NOTE_H;
            }
        }
        height
    }

    /// "1 braucht dich · 1 fertig · 3 arbeiten"; a count of zero stays out.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = [
            (self.needs_total, t("braucht dich", "needs you")),
            (self.finished_total, t("fertig", "finished")),
            (self.working, t("arbeiten", "working")),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect();
        if parts.is_empty() { t("nichts offen", "nothing open").to_string() } else { parts.join(" · ") }
    }
}

/// When an account runs out and where a new session fits better: "main voll ~Do 14:00 · neue
/// Sessions besser in second". Only when some account runs out; the better account is the one
/// with room and the least of the five hours used.
fn advice(accounts: &[AccountRow], now_ms: i64) -> Option<String> {
    let full: Vec<String> = accounts
        .iter()
        .filter_map(|a| {
            let when = runs_out_label(a.usage.runs_out?, now_ms);
            Some(format!("{} {}", a.account, tr!("voll ~{when}", "full ~{when}")))
        })
        .collect();
    if full.is_empty() {
        return None;
    }
    let better = accounts
        .iter()
        .filter(|a| a.usage.has_room && a.usage.runs_out.is_none())
        .min_by(|a, b| a.usage.five_hour.unwrap_or(0.0).total_cmp(&b.usage.five_hour.unwrap_or(0.0)));
    let mut parts = full;
    if let Some(account) = better.map(|a| a.account.clone()) {
        parts.push(tr!("neue Sessions besser in {account}", "new sessions better in {account}"));
    }
    Some(parts.join(" · "))
}

impl Brain {
    /// What the popup lists now. The whole board counts: neither the search nor the account
    /// filter hides a decision.
    pub fn popup_content(&self, now: i64) -> PopupContent {
        let groups = self.groups_for(now, "", None);
        let needs = groups
            .decide
            .iter()
            .filter(|(_, ask)| *ask != Ask::Maybe)
            .map(|(s, ask)| {
                let what = if *ask == Ask::Permission {
                    let what = s.insight.tool.clone().or_else(|| s.headline()).map(|h| plain(&h)).unwrap_or_default();
                    tr!("Freigabe für {what}", "Permission for {what}")
                } else {
                    plain(&s.headline().unwrap_or_else(|| t("wartet auf deine Eingabe", "waiting for your input").into()))
                };
                NeedRow {
                    key: s.key.clone(),
                    name: s.display_name(),
                    project: self.project_name(s),
                    since_ms: s.phase_since_ms(),
                    line: clip(&what, 200),
                    permission: *ask == Ask::Permission,
                }
            })
            .collect();
        let finished = groups
            .read
            .iter()
            .map(|s| DoneRow {
                key: s.key.clone(),
                name: s.display_name(),
                project: self.project_name(s),
                since_ms: s.phase_since_ms(),
                line: clip(&plain(&s.closing_line().unwrap_or_else(|| t("Fertig, du bist dran", "Done, your turn").into())), 200),
            })
            .collect();
        let working = groups.quiet.iter().filter(|s| matches!(s.phase(), Phase::Working | Phase::Background)).count();
        let accounts = self
            .model
            .accounts
            .iter()
            .filter_map(|account| {
                Some(AccountRow { account: account.id.clone(), index: self.account_index(&account.id), usage: usage_now(self, &account.id, now)? })
            })
            .collect();
        PopupContent::new(needs, finished, working, accounts, now)
    }

    /// The button in the title bar and a click on the status item: the popup opens or closes.
    pub(super) fn toggle_status_popup(&mut self) -> Task<Message> {
        self.status_open = !self.status_open;
        self.changes_open = false;
        self.targets_open = false;
        self.keys_leave_terminal()
    }

    /// A click on the menu bar's item: Brain comes forward with the popup open.
    pub(super) fn open_status_popup_in_front(&mut self) -> Task<Message> {
        self.status_open = true;
        self.changes_open = false;
        self.targets_open = false;
        Task::batch([self.bring_forward(), self.keys_leave_terminal()])
    }

    /// An open popup (or the list of changes) has the keyboard: the terminal gives it up, so typing
    /// doesn't reach Claude.
    pub(super) fn keys_leave_terminal(&mut self) -> Task<Message> {
        if !self.status_open && !self.changes_open && !self.targets_open {
            return Task::none();
        }
        self.focused_terminal = None;
        self.focused_shell = None;
        super::unfocus()
    }

    /// "Ansehen": the popup closes and Brain comes forward with the session.
    pub(super) fn show_session_from_popup(&mut self, key: SessionKey) -> Task<Message> {
        self.status_open = false;
        self.run_menu(super::menus::MenuCommand::OpenSession(key))
    }

    /// Allow or Deny from the popup: the way of the tree's buttons. The popup stays, shorter.
    pub(super) fn answer_from_popup(&mut self, key: SessionKey, allow: bool, since_ms: i64) -> Task<Message> {
        self.update(Message::Steer(crate::companion::Command::Permission { key, allow, since_ms }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> SessionKey {
        SessionKey { account: "main".into(), id: id.into() }
    }

    fn need(id: &str, permission: bool) -> NeedRow {
        NeedRow { key: key(id), name: id.into(), project: "p".into(), since_ms: 0, line: "x".into(), permission }
    }

    fn done(id: &str) -> DoneRow {
        DoneRow { key: key(id), name: id.into(), project: "p".into(), since_ms: 0, line: "x".into() }
    }

    fn account(name: &str, five_hour: f64, runs_out: Option<i64>) -> AccountRow {
        AccountRow {
            account: name.into(),
            index: 0,
            usage: UsageNow { five_hour: Some(five_hour), week: Some(10.0), runs_out, has_room: five_hour < 50.0 && runs_out.is_none() },
        }
    }

    #[test]
    fn nothing_open_is_calm_and_a_decision_is_not() {
        assert!(PopupContent::new(vec![], vec![], 3, vec![], 0).is_calm());
        assert!(!PopupContent::new(vec![need("a", true)], vec![], 0, vec![], 0).is_calm());
        assert!(!PopupContent::new(vec![], vec![done("a")], 0, vec![], 0).is_calm());
    }

    #[test]
    fn only_what_exists_takes_room_in_the_window() {
        let calm = PopupContent::new(vec![], vec![], 0, vec![], 0).height();
        let asking = PopupContent::new(vec![need("a", false)], vec![], 0, vec![], 0).height();
        let permission = PopupContent::new(vec![need("a", true)], vec![], 0, vec![], 0).height();
        assert_eq!(asking, PADDING * 2.0 + HEAD_H + SECTION_H + ROW_H);
        assert_eq!(permission - asking, BUTTONS_H, "Allow and Deny add their row");
        assert!(calm < asking + 30.0 && calm == PADDING * 2.0 + HEAD_H + CALM_H, "the calm line stands alone: {calm}");
        let finished = PopupContent::new(vec![], vec![done("a")], 0, vec![], 0).height();
        assert_eq!(finished, PADDING * 2.0 + HEAD_H + SECTION_H + ROW_H, "no calm line next to a finished session");
    }

    #[test]
    fn a_long_list_is_cut_and_says_how_many_more() {
        let needs: Vec<NeedRow> = (0..8).map(|i| need(&i.to_string(), false)).collect();
        let content = PopupContent::new(needs, vec![], 0, vec![], 0);
        assert_eq!((content.needs.len(), content.needs_total), (SHOWN, 8));
        assert_eq!(content.height(), PADDING * 2.0 + HEAD_H + SECTION_H + ROW_H * SHOWN as f32 + MORE_H);
        assert_eq!(content.summary().split(' ').next(), Some("8"), "the header counts all of them");
    }

    #[test]
    fn the_header_leaves_out_zero_counts() {
        let content = PopupContent::new(vec![need("a", true)], vec![done("b")], 3, vec![], 0);
        assert_eq!(content.summary().matches('·').count(), 2);
        let quiet = PopupContent::new(vec![], vec![], 2, vec![], 0);
        assert!(quiet.summary().starts_with("2 "), "{}", quiet.summary());
        assert!(!quiet.summary().contains('·'));
    }

    #[test]
    fn the_advice_names_the_account_that_runs_out_and_the_one_with_room() {
        let now_ms = 1_790_000_000_000;
        let accounts = vec![account("main", 80.0, Some(now_ms / 1000 + 3600)), account("second", 12.0, None), account("third", 30.0, None)];
        let advice = advice(&accounts, now_ms).expect("main runs out");
        assert!(advice.starts_with("main ") && advice.contains('~'), "{advice}");
        assert!(advice.ends_with("second"), "the least used account with room: {advice}");
    }

    #[test]
    fn no_advice_while_no_account_runs_out() {
        let accounts = vec![account("main", 20.0, None), account("second", 12.0, None)];
        assert_eq!(advice(&accounts, 0), None);
    }

    #[test]
    fn running_out_everywhere_recommends_nobody() {
        let now_ms = 1_790_000_000_000;
        let accounts = vec![account("main", 80.0, Some(now_ms / 1000 + 3600)), account("second", 90.0, Some(now_ms / 1000 + 7200))];
        let advice = advice(&accounts, now_ms).expect("both run out");
        assert!(!advice.contains("besser") && !advice.contains("better"), "{advice}");
    }
}
