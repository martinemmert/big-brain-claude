//! Made-up sessions for screenshots (`BRAIN_DEMO=1`), so no real work shows up.

use std::collections::HashMap;
use std::path::PathBuf;

use brain_core::account::Account;
use brain_core::event::{Event, Kind, Source};
use brain_core::sessions::SessionFile;
use brain_core::state::{Board, SessionKey};
use brain_core::transcript::{Message, Role};
use chrono::{Duration, Utc};

pub fn enabled() -> bool {
    std::env::var_os("BRAIN_DEMO").is_some()
}

pub struct Demo {
    pub accounts: Vec<Account>,
    pub board: Board,
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
                (48, Source::Hook, Kind::Prompt, "Bau Rate-Limiting ins Gateway ein"),
                (44, Source::Report, Kind::Doing, "Rate-Limiter mit Token-Bucket"),
                (6, Source::Report, Kind::Waiting, "Sollen die Limits pro Mandant oder global gelten?"),
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
                (20, Source::Hook, Kind::Prompt, "Neue Zahlungsseite nach Figma umsetzen"),
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
                (35, Source::Hook, Kind::Prompt, "Die Suche soll auch Changelogs finden"),
                (12, Source::Report, Kind::Done, "Suche indexiert jetzt auch Changelogs, Build ist grün."),
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
                (9, Source::Hook, Kind::Prompt, "Rechnungs-IDs auf UUIDs umstellen"),
                (8, Source::Report, Kind::Doing, "Migriert die Rechnungstabellen auf UUIDs"),
            ],
        },
        Spec {
            pid: 4105,
            account: "second",
            name: "mobile-onboarding",
            status: "busy",
            minutes_ago: 2,
            events: vec![(15, Source::Report, Kind::Doing, "Schreibt Snapshot-Tests für den Onboarding-Flow")],
        },
        Spec {
            pid: 4106,
            account: "main",
            name: "infra-terraform",
            status: "busy",
            minutes_ago: 1,
            events: vec![(4, Source::Hook, Kind::Prompt, "Plan für das neue Staging-Cluster")],
        },
        Spec {
            pid: 4107,
            account: "second",
            name: "design-tokens",
            status: "idle",
            minutes_ago: 5 * 60,
            events: vec![(5 * 60, Source::Report, Kind::Done, "Farbskala als Tokens exportiert")],
        },
        Spec {
            pid: 4108,
            account: "main",
            name: "release-notes",
            status: "idle",
            minutes_ago: 26 * 60,
            events: vec![(26 * 60, Source::Report, Kind::Done, "Release Notes 2.4 entworfen")],
        },
    ];

    let now = Utc::now();
    let ms = |minutes: i64| (now - Duration::minutes(minutes)).timestamp_millis();
    let mut board = Board::default();
    for spec in &specs {
        for (minutes, source, kind, text) in &spec.events {
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
        msg(48, Role::User, None, "Bau Rate-Limiting ins Gateway ein. Kunden mit vielen Integrationen dürfen nicht die anderen ausbremsen."),
        msg(47, Role::Assistant, None, "Ich schaue mir zuerst an, wie Requests heute durch das Gateway laufen."),
        msg(47, Role::Tool, Some("Grep"), "fn handle_request"),
        msg(46, Role::Tool, Some("Read"), "~/Projects/api-gateway/src/router.rs"),
        msg(45, Role::Tool, Some("Read"), "~/Projects/api-gateway/src/middleware/mod.rs"),
        msg(44, Role::Assistant, None, "Die Middleware-Kette ist der richtige Ort. Ich setze einen **Token-Bucket** davor, der pro Schlüssel zählt."),
        msg(30, Role::Tool, Some("Write"), "~/Projects/api-gateway/src/middleware/rate_limit.rs"),
        msg(22, Role::Tool, Some("Edit"), "~/Projects/api-gateway/src/middleware/mod.rs"),
        msg(15, Role::Tool, Some("Bash"), "cargo test -p gateway rate_limit"),
        msg(
            6,
            Role::Assistant,
            None,
            "## Stand\n\nDer Limiter läuft und ist getestet:\n\n- Token-Bucket mit `burst` und `refill_per_sec`\n- Antwort `429` mit `Retry-After`-Header\n- 14 neue Tests, alle grün\n\n```toml\n[rate_limit]\nburst = 100\nrefill_per_sec = 20\n```\n\nOffen ist eine Entscheidung: Sollen die Limits **pro Mandant** gelten oder **global** für das ganze Gateway? Pro Mandant schützt große Kunden voreinander, global ist einfacher zu betreiben.",
        ),
    ];
    let mut messages = HashMap::new();
    messages.insert(SessionKey { account: "main".into(), pid: 4101 }, gateway);
    messages.insert(
        SessionKey { account: "second".into(), pid: 4102 },
        vec![
            msg(20, Role::User, None, "Neue Zahlungsseite nach Figma umsetzen"),
            msg(4, Role::Assistant, None, "Die Komponenten stehen. Ich will jetzt die Storybook-Snapshots neu erzeugen."),
            msg(3, Role::Tool, Some("Bash"), "pnpm storybook:snapshots --update"),
        ],
    );

    messages.insert(
        SessionKey { account: "main".into(), pid: 4104 },
        vec![
            msg(9, Role::User, None, "Rechnungs-IDs auf UUIDs umstellen, ohne dass alte Links kaputtgehen."),
            msg(8, Role::Assistant, None, "Ich lege eine neue Spalte `uuid` an, fülle sie für alle bestehenden Rechnungen und leite alte numerische URLs per **301** weiter."),
            msg(7, Role::Tool, Some("Write"), "~/Projects/billing-service/migrations/0042_invoice_uuid.sql"),
            msg(5, Role::Tool, Some("Bash"), "make db-migrate && cargo test -p billing invoices"),
            msg(2, Role::Tool, Some("Edit"), "~/Projects/billing-service/src/routes/invoices.rs"),
        ],
    );

    Demo { accounts, board, messages }
}
