//! "What happened today": done reports per project and session, as data and as Markdown.

use chrono::{DateTime, Local, NaiveDate};

use crate::event::{Kind, Source};
use crate::state::Session;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub at: DateTime<Local>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionDay {
    pub name: String,
    pub account: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDay {
    pub project: String,
    pub sessions: Vec<SessionDay>,
}

/// Done reports of `day`. A session that reported nothing that day contributes its last turn's
/// reply instead, so quiet sessions still show up.
pub fn day<'a>(sessions: impl IntoIterator<Item = (String, &'a Session)>, day: NaiveDate) -> Vec<ProjectDay> {
    let mut projects: Vec<ProjectDay> = Vec::new();
    for (project, session) in sessions {
        let on_day = |ts: &DateTime<chrono::Utc>| ts.with_timezone(&Local).date_naive() == day;
        let mut entries: Vec<Entry> = session
            .timeline
            .iter()
            .filter(|e| e.source == Source::Report && e.kind == Kind::Done && on_day(&e.ts))
            .filter_map(|e| Some(Entry { at: e.ts.with_timezone(&Local), text: e.text.clone()? }))
            .collect();
        if entries.is_empty() {
            let last_stop = session
                .timeline
                .iter()
                .rev()
                .find(|e| e.kind == Kind::Stop && on_day(&e.ts) && e.text.is_some());
            entries.extend(last_stop.map(|e| Entry {
                at: e.ts.with_timezone(&Local),
                text: crate::hook::one_line(e.text.as_deref().unwrap_or_default(), 200),
            }));
        }
        if entries.is_empty() {
            continue;
        }
        let day = SessionDay { name: session.display_name(), account: session.key.account.clone(), entries };
        match projects.iter_mut().find(|p| p.project == project) {
            Some(p) => p.sessions.push(day),
            None => projects.push(ProjectDay { project, sessions: vec![day] }),
        }
    }
    projects.sort_by(|a, b| a.project.cmp(&b.project));
    projects
}

pub fn markdown(title: &str, projects: &[ProjectDay]) -> String {
    let mut out = format!("# {title}\n");
    for project in projects {
        out.push_str(&format!("\n## {}\n", project.project));
        for session in &project.sessions {
            out.push_str(&format!("\n**{}** ({})\n", session.name, session.account));
            for entry in &session.entries {
                out.push_str(&format!("- {} {}\n", entry.at.format("%H:%M"), entry.text));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::state::{Board, SessionKey};
    use chrono::{TimeZone, Utc};

    fn event(pid: u32, at: DateTime<Utc>, source: Source, kind: Kind, text: &str) -> Event {
        Event {
            v: 1,
            ts: at,
            account: "main".into(),
            pid,
            session_id: None,
            cwd: Some(format!("/w/s{pid}")),
            source,
            kind,
            text: Some(text.into()),
            tasks: None,
        }
    }

    #[test]
    fn reports_of_the_day_grouped_by_project_with_the_last_reply_as_fallback() {
        let today = Local.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap().with_timezone(&Utc);
        let yesterday = today - chrono::Duration::days(1);
        let mut board = Board::default();
        board.apply_event(&event(1, yesterday, Source::Report, Kind::Done, "old work"));
        board.apply_event(&event(1, today, Source::Report, Kind::Done, "Export finished"));
        board.apply_event(&event(2, today, Source::Hook, Kind::Stop, "Tests are green"));
        board.apply_event(&event(3, yesterday, Source::Hook, Kind::Stop, "nothing today"));
        let sessions = [1, 2, 3].map(|pid| {
            let s = board.get(&SessionKey { account: "main".into(), pid }).unwrap();
            ("app".to_string(), s)
        });

        let projects = day(sessions, today.with_timezone(&Local).date_naive());

        assert_eq!(projects.len(), 1);
        let texts: Vec<&str> = projects[0].sessions.iter().flat_map(|s| s.entries.iter().map(|e| e.text.as_str())).collect();
        assert_eq!(texts, vec!["Export finished", "Tests are green"]);
        let md = markdown("Today", &projects);
        assert!(md.starts_with("# Today\n\n## app\n\n**s1** (main)\n- "));
        assert!(md.contains("Export finished\n"));
    }
}
