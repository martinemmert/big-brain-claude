//! Made-up sessions for screenshots (`BRAIN_DEMO=1`), so no real work shows up.

use std::collections::HashMap;
use std::path::PathBuf;

use brain_core::account::Account;
use brain_core::event::{BackgroundTask, Event, Kind, Source};
use brain_core::sessions::SessionFile;
use brain_core::state::{Board, SessionKey};
use brain_core::transcript::{Insight, Message, Role};
use brain_core::usage::{Limit, Snapshot};
use chrono::{Duration, Utc};

pub fn enabled() -> bool {
    std::env::var_os("BRAIN_DEMO").is_some()
}

pub struct Demo {
    pub accounts: Vec<Account>,
    pub board: Board,
    pub usage: Vec<Snapshot>,
    pub messages: HashMap<SessionKey, Vec<Message>>,
}

struct Spec {
    pid: u32,
    account: &'static str,
    name: &'static str,
    status: &'static str,
    minutes_ago: i64,
    events: Vec<(i64, Source, Kind, &'static str)>,
}

pub fn build() -> Demo {
    let accounts = vec![
        Account::from_config_dir(PathBuf::from("/demo/.claude")),
        Account::from_config_dir(PathBuf::from("/demo/.claude-second")),
    ];
    let specs = vec![
        Spec {
            pid: 4101,
            account: "main",
            name: "api-gateway",
            status: "idle",
            minutes_ago: 6,
            events: vec![
                (48, Source::Hook, Kind::Prompt, "Add rate limiting to the gateway"),
                (44, Source::Report, Kind::Doing, "Rate limiter with a token bucket"),
                (6, Source::Report, Kind::Waiting, "Should the limits apply per tenant or globally?"),
                (6, Source::Hook, Kind::Stop, ""),
            ],
        },
        Spec {
            pid: 4102,
            account: "second",
            name: "checkout-redesign",
            status: "waiting",
            minutes_ago: 3,
            events: vec![
                (20, Source::Hook, Kind::Prompt, "Build the new payment page from the Figma file"),
                (3, Source::Hook, Kind::Permission, "Claude needs your permission to use Bash"),
            ],
        },
        Spec {
            pid: 4103,
            account: "main",
            name: "docs-site",
            status: "idle",
            minutes_ago: 12,
            events: vec![
                (35, Source::Hook, Kind::Prompt, "Search should find changelogs too"),
                (12, Source::Report, Kind::Done, "Search now indexes changelogs too, build is green."),
                (12, Source::Hook, Kind::Stop, ""),
            ],
        },
        Spec {
            pid: 4104,
            account: "main",
            name: "billing-service",
            status: "busy",
            minutes_ago: 1,
            events: vec![
                (9, Source::Hook, Kind::Prompt, "Switch invoice ids to UUIDs"),
                (8, Source::Report, Kind::Doing, "Migrating the invoice tables to UUIDs"),
            ],
        },
        Spec {
            pid: 4105,
            account: "second",
            name: "mobile-onboarding",
            status: "busy",
            minutes_ago: 2,
            events: vec![(15, Source::Report, Kind::Doing, "Writing snapshot tests for the onboarding flow")],
        },
        Spec {
            pid: 4106,
            account: "main",
            name: "infra-terraform",
            status: "idle",
            minutes_ago: 2,
            events: vec![
                (9, Source::Hook, Kind::Prompt, "Plan the new staging cluster"),
                (2, Source::Hook, Kind::Stop, "Two agents are checking the network and the cost estimate; I'll summarise when they are back."),
            ],
        },
        Spec {
            pid: 4107,
            account: "second",
            name: "design-tokens",
            status: "idle",
            minutes_ago: 5 * 60,
            events: vec![(5 * 60, Source::Report, Kind::Done, "Exported the colour scale as tokens")],
        },
        Spec {
            pid: 4108,
            account: "main",
            name: "release-notes",
            status: "idle",
            minutes_ago: 26 * 60,
            events: vec![(26 * 60, Source::Report, Kind::Done, "Drafted the 2.4 release notes")],
        },
    ];

    let now = Utc::now();
    let ms = |minutes: i64| (now - Duration::minutes(minutes)).timestamp_millis();
    let agent = |id: &str, description: &str| BackgroundTask {
        id: id.into(),
        kind: "subagent".into(),
        status: Some("running".into()),
        description: Some(description.into()),
        agent_type: Some("general-purpose".into()),
    };
    let background = vec![agent("a1", "Check the VPC and subnet layout"), agent("a2", "Estimate the monthly cost")];
    let mut board = Board::default();
    for spec in &specs {
        for (minutes, source, kind, text) in &spec.events {
            let tasks = (spec.pid == 4106 && *kind == Kind::Stop).then(|| background.clone());
            board.apply_event(&Event {
                v: 1,
                ts: now - Duration::minutes(*minutes),
                account: spec.account.into(),
                pid: spec.pid,
                session_id: Some(format!("demo-{}", spec.pid)),
                cwd: Some(format!("~/Projects/{}", spec.name)),
                source: *source,
                kind: *kind,
                text: (!text.is_empty()).then(|| text.to_string()),
                tasks,
            });
        }
        board.apply_session_file(
            spec.account,
            &SessionFile {
                pid: spec.pid,
                session_id: Some(format!("demo-{}", spec.pid)),
                cwd: Some(format!("~/Projects/{}", spec.name)),
                name: Some(spec.name.into()),
                status: Some(spec.status.into()),
                started_at: Some(ms(spec.minutes_ago + 50)),
                updated_at: Some(ms(spec.minutes_ago)),
                status_updated_at: Some(ms(spec.minutes_ago)),
            },
            true,
        );
    }

    let msg = |minutes: i64, role: Role, tool: Option<&str>, text: &str| Message {
        role,
        tool: tool.map(str::to_string),
        text: text.to_string(),
        ts: Some(now - Duration::minutes(minutes)),
    };
    let gateway = vec![
        msg(48, Role::User, None, "Add rate limiting to the gateway. Customers with many integrations must not slow down everyone else."),
        msg(47, Role::Assistant, None, "First I'll look at how requests flow through the gateway today."),
        msg(47, Role::Tool, Some("Grep"), "fn handle_request"),
        msg(46, Role::Tool, Some("Read"), "~/Projects/api-gateway/src/router.rs"),
        msg(45, Role::Tool, Some("Read"), "~/Projects/api-gateway/src/middleware/mod.rs"),
        msg(44, Role::Assistant, None, "The middleware chain is the right place. I'll put a **token bucket** in front of it that counts per key."),
        msg(30, Role::Tool, Some("Write"), "~/Projects/api-gateway/src/middleware/rate_limit.rs"),
        msg(22, Role::Tool, Some("Edit"), "~/Projects/api-gateway/src/middleware/mod.rs"),
        msg(15, Role::Tool, Some("Bash"), "cargo test -p gateway rate_limit"),
        msg(
            6,
            Role::Assistant,
            None,
            "## Status\n\nThe limiter works and is tested:\n\n- Token bucket with `burst` and `refill_per_sec`\n- Responds `429` with a `Retry-After` header\n- 14 new tests, all green\n\n```toml\n[rate_limit]\nburst = 100\nrefill_per_sec = 20\n```\n\nOne decision is open: should the limits apply **per tenant** or **globally** for the whole gateway? Per tenant keeps big customers from hurting each other, global is simpler to run.",
        ),
    ];
    let mut messages = HashMap::new();
    messages.insert(SessionKey { account: "main".into(), pid: 4101 }, gateway);
    messages.insert(
        SessionKey { account: "second".into(), pid: 4102 },
        vec![
            msg(20, Role::User, None, "Build the new payment page from the Figma file"),
            msg(4, Role::Assistant, None, "The components are done. Now I want to regenerate the Storybook snapshots."),
            msg(3, Role::Tool, Some("Bash"), "pnpm storybook:snapshots --update"),
        ],
    );

    messages.insert(
        SessionKey { account: "main".into(), pid: 4104 },
        vec![
            msg(9, Role::User, None, "Switch invoice ids to UUIDs without breaking old links."),
            msg(8, Role::Assistant, None, "I'll add a `uuid` column, backfill it for all existing invoices and redirect old numeric URLs with a **301**."),
            msg(7, Role::Tool, Some("Write"), "~/Projects/billing-service/migrations/0042_invoice_uuid.sql"),
            msg(5, Role::Tool, Some("Bash"), "make db-migrate && cargo test -p billing invoices"),
            msg(2, Role::Tool, Some("Edit"), "~/Projects/billing-service/src/routes/invoices.rs"),
        ],
    );

    let insights = [
        (4101, "main", "claude-opus-5-5", 142_000, 3.84, "default"),
        (4102, "second", "claude-sonnet-5-5", 61_000, 0.92, "default"),
        (4103, "main", "claude-sonnet-5-5", 38_000, 0.41, "acceptEdits"),
        (4104, "main", "claude-opus-5-5", 97_000, 2.17, "default"),
    ];
    for (pid, account, model, tokens, cost, mode) in insights {
        let key = SessionKey { account: account.into(), pid };
        board.apply_insight(
            &key,
            Insight {
                // No turn signal: the demo's state comes from its events and status files.
                turn: None,
                model: Some(model.into()),
                context_tokens: Some(tokens),
                permission_mode: Some(mode.into()),
                cost_usd: Some(cost),
                title: None,
            },
        );
    }

    let resets = |hours: i64| Some((now + Duration::hours(hours)).timestamp());
    let snapshot = |account: &str, pid: u32, five: f64, seven: f64, context: f64, cost: f64| Snapshot {
        account: account.into(),
        pid,
        session_id: Some(format!("demo-{pid}")),
        ts: now.timestamp_millis(),
        context_percent: Some(context),
        context_size: Some(200_000),
        cost_usd: Some(cost),
        five_hour: Some(Limit { used_percentage: five, resets_at: resets(2) }),
        seven_day: Some(Limit { used_percentage: seven, resets_at: resets(80) }),
    };
    let usage = vec![
        snapshot("main", 4101, 46.0, 21.0, 71.0, 3.84),
        snapshot("main", 4103, 46.0, 21.0, 19.0, 0.41),
        snapshot("main", 4104, 46.0, 21.0, 48.0, 2.17),
        snapshot("second", 4102, 12.0, 34.0, 30.0, 0.92),
    ];

    Demo { accounts, board, usage, messages }
}
