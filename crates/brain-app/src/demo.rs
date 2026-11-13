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

/// The projects the demo's tree lists; "legacy-shop" has no session, "release-notes" and
/// "design-tokens" are not listed on purpose (they show under "Weitere").
const LISTED: [&str; 7] = ["api-gateway", "checkout-redesign", "docs-site", "billing-service", "infra-terraform", "mobile-onboarding", "legacy-shop"];

pub fn list_projects(prefs: &mut crate::prefs::Prefs) -> bool {
    for name in LISTED {
        prefs.list_project(name, Some(&format!("~/Projects/{name}")));
    }
    false
}

/// Notes for later: two in docs-site, one in the resting legacy-shop.
pub fn later() -> Vec<(brain_core::later::Item, String)> {
    let note = |id: &str, hours: i64, text: &str, project: &str| {
        let item = brain_core::later::Item {
            id: id.into(),
            ts: Utc::now() - Duration::hours(hours),
            text: text.into(),
            cwd: Some(format!("~/Projects/{project}")),
            account: Some("main".into()),
            session_id: None,
            session_name: None,
        };
        (item, project.to_string())
    };
    vec![
        note("demo-1", 30, "Algolia-Index nachts neu bauen", "docs-site"),
        note("demo-2", 5, "Kaputte Links im Archiv prüfen", "docs-site"),
        note("demo-3", 70, "Altes Warenkorb-Modul entfernen", "legacy-shop"),
    ]
}

pub struct Demo {
    pub accounts: Vec<Account>,
    pub board: Board,
    pub usage: Vec<Snapshot>,
    pub messages: HashMap<SessionKey, Vec<Message>>,
    /// The files each session changed, newest first.
    pub changes: HashMap<SessionKey, brain_core::changes::Changeset>,
    /// What the first of them (Rate-Limits) changes against each target the select offers.
    pub targets: HashMap<String, brain_core::changes::Changeset>,
    /// Their diffs, by path.
    pub diffs: HashMap<String, String>,
}

struct Spec {
    pid: u32,
    account: &'static str,
    /// The folder's name, which is the project.
    project: &'static str,
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
            project: "api-gateway",
            name: "Rate-Limits",
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
            project: "checkout-redesign",
            name: "Stripe Elements",
            status: "waiting",
            minutes_ago: 3,
            events: vec![
                (20, Source::Hook, Kind::Prompt, "Build the new payment page from the Figma file"),
                (3, Source::Hook, Kind::Permission, "Claude needs your permission to use Bash"),
            ],
        },
        Spec {
            pid: 4109,
            account: "second",
            project: "checkout-redesign",
            name: "Warenkorb-Tests",
            status: "busy",
            minutes_ago: 21,
            events: vec![(30, Source::Report, Kind::Doing, "Writing cart tests")],
        },
        Spec {
            pid: 4103,
            account: "main",
            project: "docs-site",
            name: "Changelog-Suche",
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
            project: "billing-service",
            name: "UUID-Migration",
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
            project: "mobile-onboarding",
            name: "Onboarding-Flow",
            status: "busy",
            minutes_ago: 2,
            events: vec![(15, Source::Report, Kind::Doing, "Writing snapshot tests for the onboarding flow")],
        },
        Spec {
            pid: 4106,
            account: "main",
            project: "infra-terraform",
            name: "Staging-Cluster",
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
            project: "design-tokens",
            name: "Farbskala exportieren",
            status: "idle",
            minutes_ago: 5 * 60,
            events: vec![(5 * 60, Source::Report, Kind::Done, "Exported the colour scale as tokens")],
        },
        Spec {
            pid: 4108,
            account: "main",
            project: "release-notes",
            name: "Release Notes 2.4",
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
                cwd: Some(format!("~/Projects/{}", spec.project)),
                source: *source,
                kind: *kind,
                text: (!text.is_empty()).then(|| text.to_string()),
                tasks,
                name: None,
                ask: None,
            });
        }
        board.apply_session_file(
            spec.account,
            &SessionFile {
                pid: spec.pid,
                session_id: Some(format!("demo-{}", spec.pid)),
                cwd: Some(format!("~/Projects/{}", spec.project)),
                name: Some(spec.name.into()),
                status: Some(spec.status.into()),
                started_at: Some(ms(spec.minutes_ago + 50)),
                updated_at: Some(ms(spec.minutes_ago)),
                status_updated_at: Some(ms(spec.minutes_ago)),
                spare: false,
                parked_job_id: None,
                messaging_socket_path: None,
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
    messages.insert(SessionKey { account: "main".into(), id: "demo-4101".into() }, gateway);
    messages.insert(
        SessionKey { account: "second".into(), id: "demo-4102".into() },
        vec![
            msg(20, Role::User, None, "Build the new payment page from the Figma file"),
            msg(4, Role::Assistant, None, "The components are done. Now I want to regenerate the Storybook snapshots."),
            msg(3, Role::Tool, Some("Bash"), "pnpm storybook:snapshots --update"),
        ],
    );

    messages.insert(
        SessionKey { account: "main".into(), id: "demo-4104".into() },
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
        let tool = (pid == 4102).then(|| "Bash(npm run e2e)".to_string());
        let key = SessionKey { account: account.into(), id: format!("demo-{pid}") };
        board.apply_insight(
            &key,
            Insight {
                asks: false,
                closing: None,
                tool,
                model: Some(model.into()),
                context_tokens: Some(tokens),
                permission_mode: Some(mode.into()),
                cost_usd: Some(cost),
                title: None,
                edited: Vec::new(),
                entrypoint: Some("cli".into()),
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
        snapshot("main", 4101, 84.0, 21.0, 71.0, 3.84),
        snapshot("main", 4103, 84.0, 21.0, 19.0, 0.41),
        snapshot("main", 4104, 84.0, 21.0, 48.0, 2.17),
        snapshot("second", 4102, 12.0, 34.0, 30.0, 0.92),
    ];

    let (targets, diffs) = rate_limit_changes(now.timestamp_millis());
    let changes = HashMap::from([(SessionKey { account: "main".into(), id: "demo-4101".into() }, targets["main"].clone())]);

    Demo { accounts, board, usage, messages, changes, targets, diffs }
}

/// "Rate-Limits" has two drafts of a review: one on `burst` in the configuration, one on the
/// limiter's clock.
pub fn reviews() -> Vec<brain_core::review::Review> {
    use brain_core::review::{Place, Review};
    let root = "~/Projects/api-gateway";
    let mut review = Review::new(&SessionKey { account: "main".into(), id: "demo-4101".into() });
    review.add(
        &format!("{root}/src/config.rs"),
        "src/config.rs",
        Place::Line { new: Some(22), old: None },
        "    pub burst: u32,",
        "Bitte als Option, Default 100.",
    );
    review.add(
        &format!("{root}/src/middleware/rate_limit.rs"),
        "src/middleware/rate_limit.rs",
        Place::Line { new: Some(18), old: None },
        "        let now = Instant::now();",
        "Hier lieber eine Uhr reinreichen statt Instant::now(), dann braucht der zweite Test kein sleep.",
    );
    vec![review]
}

/// The merge requests of "Rate-Limits", `feature/rate-limit → <target>`, one for each target the
/// select offers: `main` has four files, `develop` two (the rest is there already), `origin/main`
/// and `feature/billing` more (a README, a dependency). The files by path with their made-up
/// diffs; the counts are those of the diffs. A file without a sentence is not the session's (a
/// colleague's commit on the branch), it has no mark.
fn rate_limit_changes(now_ms: i64) -> (HashMap<String, brain_core::changes::Changeset>, HashMap<String, String>) {
    let root = "~/Projects/api-gateway";
    let files = [
        (
            "src/config.rs",
            7,
            Some("Neuer Abschnitt „rate_limit“ in der Konfiguration: wie viele Anfragen pro Sekunde, wie viele auf Vorrat und ob das Limit global oder pro Mandant gilt."),
            "M ",
            CONFIG_DIFF.to_string(),
            &["main", "origin/main", "feature/billing"][..],
        ),
        ("src/middleware/mod.rs", 24, None, "M ", MOD_DIFF.to_string(), &["main", "origin/main", "feature/billing"][..]),
        (
            "src/middleware/rate_limit.rs",
            19,
            Some("Der Limiter selbst, ein Token-Bucket: Jede Anfrage kostet eine Marke, der Eimer füllt sich mit der Zeit wieder."),
            "A ",
            new_file("src/middleware/rate_limit.rs", LIMITER_BODY),
            &["main", "develop", "origin/main", "feature/billing"][..],
        ),
        (
            "tests/rate_limit.rs",
            11,
            Some("Zwei Tests für den Limiter: Ein voller Eimer lässt drei Anfragen durch und lehnt die vierte ab, nach 30 ms kommt wieder eine durch."),
            "A ",
            new_file("tests/rate_limit.rs", TEST_BODY),
            &["main", "develop", "origin/main", "feature/billing"][..],
        ),
        (
            "README.md",
            3,
            Some("Ein Abschnitt „Rate limiting“ in der README: wie man das Limit einstellt und was Clients bei 429 tun sollen."),
            "M ",
            README_DIFF.to_string(),
            &["origin/main", "feature/billing"][..],
        ),
        ("Cargo.toml", 31, None, "M ", CARGO_DIFF.to_string(), &["feature/billing"][..]),
    ];
    let mut changes: Vec<(brain_core::changes::SessionChange, &[&str])> = Vec::new();
    let mut diffs = HashMap::new();
    for (relative, minutes, said, status, diff, targets) in files {
        let count = |sign: char, header: &str| diff.lines().filter(|l| l.starts_with(sign) && !l.starts_with(header)).count() as u64;
        let path = format!("{root}/{relative}");
        let (added, removed) = (Some(count('+', "+++")), Some(count('-', "---")));
        changes.push((brain_core::changes::SessionChange {
            path: path.clone(),
            relative: relative.to_string(),
            last_ms: now_ms - minutes * 60_000,
            added,
            removed,
            said: said.map(str::to_string),
            root: PathBuf::from(root),
            git: Some(brain_core::changes::FileChange { path: relative.to_string(), status: status.to_string(), added, removed }),
            from_session: said.is_some(),
            base: None,
            original: None,
        }, targets));
        diffs.insert(path, diff);
    }
    let sets = target_choices()
        .into_iter()
        .map(|target| {
            let merge = brain_core::changes::MergeInfo {
                root: PathBuf::from(root),
                branch: "feature/rate-limit".into(),
                detached: false,
                target: Some(target.clone()),
                base: "demo".into(),
            };
            let files = changes.iter().filter(|(_, targets)| targets.contains(&target.as_str())).map(|(change, _)| change.clone()).collect();
            (target, brain_core::changes::Changeset { files, merge: Some(merge) })
        })
        .collect();
    (sets, diffs)
}

/// The branches the demo offers as targets.
pub fn target_choices() -> Vec<String> {
    ["main", "develop", "origin/main", "feature/billing"].map(String::from).to_vec()
}

/// A new file's diff: every line added.
fn new_file(relative: &str, body: &str) -> String {
    let count = body.lines().count();
    let added: String = body.lines().map(|l| format!("+{l}\n")).collect();
    format!("diff --git a/{relative} b/{relative}\nnew file mode 100644\n--- /dev/null\n+++ b/{relative}\n@@ -0,0 +1,{count} @@\n{added}")
}

const CONFIG_DIFF: &str = r#"diff --git a/src/config.rs b/src/config.rs
index 3f2a1c4..9b7e0d2 100644
--- a/src/config.rs
+++ b/src/config.rs
@@ -12,7 +12,23 @@ pub struct Config {
     pub listen: SocketAddr,
     pub upstream: Url,
-    pub timeout_secs: u64,
+    pub timeout: Duration,
+    pub rate_limit: RateLimitConfig,
 }

+/// How many requests a client may make per second, and how many it may save up.
+#[derive(Debug, Clone, Deserialize)]
+pub struct RateLimitConfig {
+    pub per_second: u32,
+    pub burst: u32,
+    pub scope: Scope,
+}
+
+#[derive(Debug, Clone, Copy, Deserialize)]
+#[serde(rename_all = "snake_case")]
+pub enum Scope {
+    Global,
+    Tenant,
+}
@@ -41,6 +57,10 @@ impl Config {
     pub fn load(path: &Path) -> Result<Self> {
         let text = std::fs::read_to_string(path)?;
-        Ok(toml::from_str(&text)?)
+        let config: Self = toml::from_str(&text)?;
+        anyhow::ensure!(config.rate_limit.burst >= config.rate_limit.per_second, "burst must not be below per_second");
+        Ok(config)
     }
 }
"#;

const README_DIFF: &str = r#"diff --git a/README.md b/README.md
index 8d41c02..e7a9f35 100644
--- a/README.md
+++ b/README.md
@@ -18,6 +18,16 @@ listen = "0.0.0.0:8080"
 upstream = "http://localhost:9000"
 ```

+## Rate limiting
+
+Every client may make `per_second` requests per second and save up `burst` of them. A request
+over the limit gets `429 Too Many Requests` with a `Retry-After` header.
+
+```toml
+[rate_limit]
+per_second = 50
+burst = 100
+scope = "tenant"
+```
+
 ## Development
"#;

const CARGO_DIFF: &str = r#"diff --git a/Cargo.toml b/Cargo.toml
index 6b2d9e1..f40c8a7 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -9,6 +9,7 @@ edition = "2021"
 anyhow = "1"
 serde = { version = "1", features = ["derive"] }
 tokio = { version = "1", features = ["full"] }
+toml = "0.8"
 tower = "0.5"
-tracing = "0.1"
+tracing = "0.1.40"
"#;

const MOD_DIFF: &str = r#"diff --git a/src/middleware/mod.rs b/src/middleware/mod.rs
index 51d0a77..c3e8f19 100644
--- a/src/middleware/mod.rs
+++ b/src/middleware/mod.rs
@@ -1,6 +1,8 @@
 mod auth;
 mod logging;
-mod timeout;
+mod rate_limit;
+mod timeout;

 pub use auth::Auth;
 pub use logging::Logging;
+pub use rate_limit::RateLimit;
"#;

const LIMITER_BODY: &str = r#"use std::time::Instant;

/// A token bucket: it fills at `per_second` and holds at most `burst`.
pub struct Bucket {
    tokens: f64,
    per_second: f64,
    burst: f64,
    updated: Instant,
}

impl Bucket {
    pub fn new(per_second: u32, burst: u32) -> Self {
        Self { tokens: burst as f64, per_second: per_second as f64, burst: burst as f64, updated: Instant::now() }
    }

    /// Takes one token if there is one.
    pub fn take(&mut self) -> bool {
        let now = Instant::now();
        let gained = now.duration_since(self.updated).as_secs_f64() * self.per_second;
        self.tokens = (self.tokens + gained).min(self.burst);
        self.updated = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}"#;

const TEST_BODY: &str = r#"use gateway::middleware::rate_limit::Bucket;

#[test]
fn a_full_bucket_lets_a_burst_through_and_then_says_no() {
    let mut bucket = Bucket::new(1, 3);
    assert!(bucket.take() && bucket.take() && bucket.take());
    assert!(!bucket.take());
}

#[test]
fn a_bucket_fills_up_again_with_time() {
    let mut bucket = Bucket::new(100, 1);
    assert!(bucket.take());
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(bucket.take());
}"#;
