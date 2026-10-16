//! Brain Link: the companion app on the phone talks to Brain over HTTPS in the network both are
//! in. It is off until the user pairs a phone (⌘K → "Pair a phone").
//!
//! Pairing hands the phone a `brainlink://pair` URL (as a QR code the iPhone camera opens): the
//! addresses Brain listens on, a random token every request must carry, and the SHA-256 of
//! Brain's self-signed certificate, which the phone pins instead of trusting any CA. So nobody
//! on the same Wi-Fi can read along or pose as Brain, and without the token nobody gets in.
//!
//! The server reads a snapshot of the session list that Brain publishes on every refresh,
//! transcripts, and the files a session touched or changed in its folder. The one thing it
//! writes is what the phone attaches (a photo, a PDF), into `inbox/` next to the token, for a
//! reply to name. Replies and permission answers are handed to Brain's UI thread as
//! [`Command`]s, which does with them what it does for a reply typed in Brain.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use base64::Engine;
use brain_core::account::{home_dir, Account};
use brain_core::changes::{changes_of, diff_of};
use brain_core::files::{session_files, Touch};
use brain_core::state::SessionKey;
use brain_core::transcript::{cwd_of, find_transcript, read_all_messages, read_recent_messages, Role};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use serde::Serialize;
use tokio_rustls::rustls;

/// The port Brain Link listens on, on every interface.
pub const PORT: u16 = 48620;
/// Messages the phone gets per session, newest last.
const MESSAGE_LIMIT: usize = 80;
/// The most it gets when it asks for older ones.
const MESSAGE_LIMIT_MAX: usize = 5000;
/// Search hits per search, and how much of each.
const SEARCH_HITS: usize = 100;
const SEARCH_CHARS: usize = 400;
/// A message asked for in full: practically all of it.
const FULL_CHARS: usize = 200_000;
/// The longest message text sent; the phone shows the start of longer ones.
const MESSAGE_CHARS: usize = 6000;
/// The largest request body: a reply.
const BODY_LIMIT: usize = 64 * 1024;
/// The largest attachment the phone sends: a photo, a PDF.
const ATTACHMENT_LIMIT: usize = 15 * 1024 * 1024;
/// The largest file the phone fetches from a session.
const FILE_LIMIT: u64 = 10 * 1024 * 1024;
/// The longest diff sent; the phone shows the start of longer ones.
const DIFF_LIMIT: usize = 512 * 1024;
/// The files a session touched, most recent first, at most this many.
const TOUCHED_LIMIT: usize = 200;

/// One session as the phone lists it.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub account: String,
    pub id: String,
    pub name: String,
    /// `needs_you`, `your_turn`, `working`, `background` or `ended`.
    pub phase: &'static str,
    /// The list section, as Brain's: `pinned`, `attention`, `working`, `resting` or `ended`.
    pub group: &'static str,
    pub headline: Option<String>,
    /// When the current phase began (epoch ms).
    pub since_ms: i64,
    pub folder: Option<String>,
    /// A permission dialog is open: the phone offers Allow and Deny.
    pub permission_open: bool,
    /// When that dialog opened (epoch ms): the phone sends it with its answer, so the answer
    /// only ever reaches the dialog the user saw, never one that opened since.
    pub permission_since_ms: Option<i64>,
    /// An AskUserQuestion Claude waits on: the phone offers its options. While one is open,
    /// `permission_open` is false: Allow would just pick the first option.
    pub question: Option<brain_core::qa::OpenQuestion>,
    /// Brain can deliver a reply (its terminal, an iTerm tab, or a background session).
    pub can_reply: bool,
    /// Questions of the session's team members to the person, not answered yet; the phone
    /// answers with a reply "Antwort an <from>: …", which the lead passes on.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub team_questions: Vec<brain_core::teams::TeamMessage>,
    /// How many of its agents still work or wait (none: no agents).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_active: Option<usize>,
    /// It runs as an agent team Brain started (in tmux): the phone offers to go on without.
    pub in_team: bool,
}

/// What the phone asked Brain to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Reply { key: SessionKey, text: String },
    /// Answers to an open question, one per question; `call` names the question the phone
    /// showed.
    Answer { key: SessionKey, call: String, answers: Vec<Picked> },
    /// A new background session in one of the offered folders, with an optional first prompt and
    /// output style.
    Start { account: String, folder: String, prompt: Option<String>, style: Option<String>, team: bool },
    /// `since_ms`: when the dialog the user answered opened ([`View::permission_since_ms`]).
    Permission { key: SessionKey, allow: bool, since_ms: i64 },
    /// Esc for a working session: Claude stops what it does.
    Interrupt { key: SessionKey },
    /// An ended session goes on in the background.
    Resume { key: SessionKey },
    /// The session goes on with these flags (`--model opus`, `--chrome`).
    Restart { key: SessionKey, flags: Vec<String> },
    /// A background session stops (its conversation stays), any other gets `/exit`.
    End { key: SessionKey },
    /// The session goes on as an agent team (true) or without one: a restart, as with flags.
    Team { key: SessionKey, on: bool },
}

/// The answer to one question: the options picked (one, or several where the question takes
/// several) and what the user wrote instead or besides.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub picks: Vec<usize>,
    pub other: Option<String>,
}

/// What this Brain Link offers beyond the first version, for the phone to show only what works.
const FEATURES: [&str; 12] = ["files", "attachments", "wait", "control", "answers", "history", "folders", "rename", "end", "styles", "commands", "team"];
/// How long a read with `wait` waits for something new before it answers anyway.
const WAIT: Duration = Duration::from_secs(25);
/// How often a waiting read looks again.
const WAIT_STEP: Duration = Duration::from_millis(250);

/// A plan limit as the phone shows it.
#[derive(Debug, Clone, Serialize)]
pub struct LimitView {
    pub used_percentage: f64,
    /// Unix seconds.
    pub resets_at: Option<i64>,
    /// When it runs out at the current pace, if before the reset (Unix seconds).
    pub runs_out_at: Option<i64>,
}

/// An account's plan limits.
#[derive(Debug, Clone, Serialize)]
pub struct UsageView {
    pub account: String,
    pub five_hour: Option<LimitView>,
    pub seven_day: Option<LimitView>,
}

/// The folders in one folder inside the home folder, for choosing where a session starts.
#[derive(Debug, Clone, Serialize)]
struct FolderList {
    path: String,
    /// The folder above, while that is still inside the home folder.
    parent: Option<String>,
    /// Full paths, by name.
    folders: Vec<String>,
}

/// Where the phone may start a session: the accounts, and the folders sessions ran in lately.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Places {
    pub accounts: Vec<String>,
    pub folders: Vec<String>,
    /// The output styles a session can start with: built-in and the accounts' own.
    pub styles: Vec<brain_core::styles::Style>,
}

/// What the pairing QR code carries.
#[derive(Debug, Clone)]
pub struct Pairing {
    pub url: String,
    pub hosts: Vec<String>,
    /// The first 16 hex digits of the certificate's fingerprint (`071f 6895 1f9e 01d6`: 64 bits,
    /// too many to grind a lookalike certificate for). The phone shows the same before it pairs:
    /// name and addresses in a link can be made up, this can't.
    pub code: String,
}

/// The running server: Brain publishes its session list to it and takes the phone's commands.
pub struct Link {
    state: Arc<State>,
    commands: Mutex<Receiver<Command>>,
    stop: Arc<AtomicBool>,
    /// Where the certificate, its key and the token live.
    dir: PathBuf,
    pub pairing: Pairing,
}

impl Link {
    /// Starts the server on its own thread. Creates the certificate and the token on first use.
    pub fn start(accounts: Vec<Account>) -> Result<Link, String> {
        Link::start_in(dir(), PORT, accounts)
    }

    fn start_in(dir: PathBuf, port: u16, accounts: Vec<Account>) -> Result<Link, String> {
        let identity = Identity::load_or_create(&dir)?;
        let config = identity.server_config()?;
        let hosts = hosts();
        let pairing = Pairing { url: pairing_url(&hosts, &identity), code: short_code(&identity.fingerprint()), hosts };
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();
        let state = Arc::new(State {
            token: RwLock::new(identity.token),
            inbox: dir.join("inbox"),
            accounts,
            sessions: RwLock::new(b"[]".to_vec()),
            version: AtomicU64::new(1),
            usage: RwLock::new(Vec::new()),
            places: RwLock::new(Places::default()),
            commands: Mutex::new(sender),
        });
        let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let (stopped, served) = (stop.clone(), state.clone());
        std::thread::Builder::new()
            .name("brain-link".into())
            .spawn(move || serve(listener, config, served, stopped))
            .map_err(|e| e.to_string())?;
        Ok(Link { state, commands: Mutex::new(receiver), stop, dir, pairing })
    }

    /// The list as the phone sees it. Only a list that differs from the last one counts as new:
    /// a phone waiting for news (`wait`) gets it within a moment.
    pub fn publish(&self, views: Vec<View>) {
        let json = serde_json::to_vec(&views).unwrap_or_default();
        if let Ok(mut sessions) = self.state.sessions.write() {
            if *sessions != json {
                *sessions = json;
                self.state.version.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// The plan limits and the places to start a session, for the phone.
    pub fn publish_more(&self, usage: Vec<UsageView>, places: Places) {
        if let Ok(mut current) = self.state.usage.write() {
            *current = usage;
        }
        if let Ok(mut current) = self.state.places.write() {
            *current = places;
        }
    }

    /// The Mac's addresses as they are now (after a change of network), for the pairing code.
    pub fn refresh_addresses(&mut self) -> Result<(), String> {
        let identity = Identity::load_or_create(&self.dir)?;
        self.pairing.hosts = hosts();
        self.pairing.url = pairing_url(&self.pairing.hosts, &identity);
        Ok(())
    }

    /// A new token: phones paired before are shut out and must scan the new code.
    pub fn forget_phones(&mut self) -> Result<(), String> {
        let _ = std::fs::remove_file(self.dir.join("token"));
        let identity = Identity::load_or_create(&self.dir)?;
        self.pairing.url = pairing_url(&self.pairing.hosts, &identity);
        if let Ok(mut token) = self.state.token.write() {
            *token = identity.token;
        }
        Ok(())
    }

    pub fn take_commands(&self) -> Vec<Command> {
        self.commands.lock().map(|r| r.try_iter().collect()).unwrap_or_default()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Whether the user turned Brain Link on (it then starts with Brain).
pub fn is_enabled() -> bool {
    dir().join("enabled").exists()
}

pub fn set_enabled(on: bool) {
    let marker = dir().join("enabled");
    if on {
        let _ = create_dir(&dir()).and_then(|_| std::fs::write(&marker, b""));
    } else {
        let _ = std::fs::remove_file(marker);
    }
}

/// The pairing URL as a QR code: its width and its modules, row by row (`true` is dark).
pub fn qr_modules(url: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M).ok()?;
    let modules = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((code.width(), modules))
}

// ---- identity -------------------------------------------------------------------------------

fn dir() -> PathBuf {
    home_dir().join(".claude-brain").join("link")
}

fn create_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

fn write_private(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(dir.join(name))
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

/// Brain's certificate, its key and the token, kept in `~/.claude-brain/link` (only the user
/// can open it).
struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
    token: String,
}

impl Identity {
    fn load_or_create(dir: &Path) -> Result<Identity, String> {
        create_dir(dir).map_err(|e| e.to_string())?;
        let read = |name: &str| std::fs::read(dir.join(name)).ok().filter(|b| !b.is_empty());
        let (cert, key) = match (read("cert.der"), read("key.der")) {
            (Some(cert), Some(key)) => (cert, key),
            _ => {
                let generated = rcgen::generate_simple_self_signed(vec!["brain.local".to_string()]).map_err(|e| e.to_string())?;
                let (cert, key) = (generated.cert.der().to_vec(), generated.signing_key.serialize_der());
                write_private(dir, "cert.der", &cert)?;
                write_private(dir, "key.der", &key)?;
                (cert, key)
            }
        };
        let token = match read("token").and_then(|b| String::from_utf8(b).ok()) {
            Some(token) => token.trim().to_string(),
            None => {
                let mut bytes = [0u8; 32];
                ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).map_err(|_| "no randomness".to_string())?;
                let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
                write_private(dir, "token", token.as_bytes())?;
                token
            }
        };
        Ok(Identity { cert, key, token })
    }

    /// The certificate's SHA-256, as the phone pins it.
    fn fingerprint(&self) -> String {
        ring::digest::digest(&ring::digest::SHA256, &self.cert).as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, String> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(self.cert.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())))
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(config))
    }
}

/// `brainlink://pair?…`: the Mac's name, the addresses to try, the port, the token and the
/// certificate's fingerprint.
fn pairing_url(hosts: &[String], identity: &Identity) -> String {
    let name = computer_name();
    format!(
        "brainlink://pair?v=1&n={}&h={}&p={PORT}&t={}&f={}",
        encode(&name),
        encode(&hosts.join(",")),
        identity.token,
        identity.fingerprint()
    )
}

fn short_code(fingerprint: &str) -> String {
    (0..4).map(|i| &fingerprint[i * 4..i * 4 + 4]).collect::<Vec<_>>().join(" ")
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The Mac's Bonjour name first (it survives a new IP address in the same network), then the
/// IPv4 addresses of its Wi-Fi and Ethernet. Virtual interfaces (a VM's bridge, a VPN's tunnel)
/// are left out: the phone can't reach them, and every dead address delays its reconnect.
fn hosts() -> Vec<String> {
    let mut hosts = Vec::new();
    if let Some(name) = local_host_name() {
        hosts.push(format!("{name}.local"));
    }
    hosts.extend(ipv4_addresses().into_iter().map(|a| a.to_string()));
    hosts
}

fn local_host_name() -> Option<String> {
    let out = std::process::Command::new("scutil").args(["--get", "LocalHostName"]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

fn computer_name() -> String {
    std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Brain".into())
}

fn ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut addresses = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list` with a linked list that freeifaddrs releases; entries are
    // only read while it lives.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return addresses;
        }
        let mut entry = list;
        while !entry.is_null() {
            let ifa = &*entry;
            let up = ifa.ifa_flags & (libc::IFF_UP as u32) != 0 && ifa.ifa_flags & (libc::IFF_LOOPBACK as u32) == 0;
            // macOS names Wi-Fi and Ethernet en0, en1, …
            let wired_or_wifi = !ifa.ifa_name.is_null() && std::ffi::CStr::from_ptr(ifa.ifa_name).to_bytes().starts_with(b"en");
            if up && wired_or_wifi && !ifa.ifa_addr.is_null() && i32::from((*ifa.ifa_addr).sa_family) == libc::AF_INET {
                let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
                if !ip.is_link_local() && !addresses.contains(&ip) {
                    addresses.push(ip);
                }
            }
            entry = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    addresses
}

// ---- server ---------------------------------------------------------------------------------

struct State {
    token: RwLock<String>,
    /// Where the phone's attachments go, a folder per session.
    inbox: PathBuf,
    accounts: Vec<Account>,
    /// The published list, as JSON.
    sessions: RwLock<Vec<u8>>,
    /// Counts up with each list that differs from the one before.
    version: AtomicU64,
    usage: RwLock<Vec<UsageView>>,
    places: RwLock<Places>,
    commands: Mutex<Sender<Command>>,
}

fn serve(listener: std::net::TcpListener, config: Arc<rustls::ServerConfig>, state: Arc<State>, stop: Arc<AtomicBool>) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() else { return };
    runtime.block_on(async move {
        let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
        let acceptor = tokio_rustls::TlsAcceptor::from(config);
        while !stop.load(Ordering::Relaxed) {
            // Wakes up every second to notice a stop.
            let Ok(Ok((tcp, _))) = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await else { continue };
            let (acceptor, state) = (acceptor.clone(), state.clone());
            tokio::spawn(async move {
                let Ok(Ok(tls)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await else { return };
                let service = hyper::service::service_fn(move |req| handle(req, state.clone()));
                let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(tls), service).await;
            });
        }
    });
}

async fn handle(req: Request<Incoming>, state: Arc<State>) -> Result<Response<Full<Bytes>>, Infallible> {
    let token = state.token.read().map(|t| t.clone()).unwrap_or_default();
    if token.is_empty() || !authorized(&req, &token) {
        // Guessing gets slow.
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Ok(reply(StatusCode::UNAUTHORIZED, &serde_json::json!({ "error": "not paired" })));
    }
    let path: Vec<String> = req.uri().path().trim_matches('/').split('/').map(str::to_string).collect();
    let parts: Vec<&str> = path.iter().map(String::as_str).collect();
    let method = req.method().clone();
    Ok(match (method, parts.as_slice()) {
        // Where Brain is reachable now: the phone keeps the list current, so a new address of
        // the Mac needs no new pairing.
        (Method::GET, ["v1", "hello"]) => {
            reply(
                StatusCode::OK,
                &serde_json::json!({ "name": computer_name(), "version": env!("CARGO_PKG_VERSION"), "hosts": hosts(), "features": FEATURES }),
            )
        }
        // `wait=<version>`: answers once the list is newer than that version (or after a while
        // anyway), so the phone sees a change within a moment instead of at its next look.
        (Method::GET, ["v1", "sessions"]) => {
            if let Some(seen) = query_param(req.uri().query(), "wait").and_then(|v| v.parse::<u64>().ok()) {
                let until = tokio::time::Instant::now() + WAIT;
                while state.version.load(Ordering::Relaxed) == seen && tokio::time::Instant::now() < until {
                    tokio::time::sleep(WAIT_STEP).await;
                }
            }
            // The version is read before the list: a list published in between comes again next time.
            let version = state.version.load(Ordering::Relaxed);
            let sessions = state.sessions.read().map(|s| s.clone()).unwrap_or_default();
            with_version(raw(StatusCode::OK, "application/json", sessions), version)
        }
        (Method::GET, ["v1", "usage"]) => reply(StatusCode::OK, &state.usage.read().map(|u| u.clone()).unwrap_or_default()),
        (Method::GET, ["v1", "places"]) => reply(StatusCode::OK, &state.places.read().map(|p| p.clone()).unwrap_or_default()),
        // `path=<folder>`, the home folder without one; nothing outside it.
        (Method::GET, ["v1", "folders"]) => {
            let asked = query_param(req.uri().query(), "path");
            off_thread(state.clone(), move |_| match folder_list(&home_dir(), asked.as_deref()) {
                Some(list) => reply(StatusCode::OK, &list),
                None => not_found(),
            })
            .await
        }
        (Method::POST, ["v1", "sessions"]) => {
            let Some(body) = read_json(req).await else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })));
            };
            let field = |name: &str| body.get(name).and_then(|v| v.as_str()).map(str::to_string);
            let (Some(account), Some(folder)) = (field("account"), field("folder")) else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "account and folder" })));
            };
            // Only what Brain offered: a known account, a folder sessions ran in or one inside the
            // home folder.
            let places = state.places.read().map(|p| p.clone()).unwrap_or_default();
            let folder = match places.folders.contains(&folder) {
                true => Some(folder),
                false => brain_core::folders::inside(&home_dir(), Path::new(&folder)).map(|p| p.display().to_string()),
            };
            let Some(folder) = folder.filter(|_| places.accounts.contains(&account)) else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "unknown account or folder" })));
            };
            let prompt = field("prompt").filter(|p| !p.trim().is_empty());
            // Brain checks the style against the account's when it starts the session.
            let style = field("output_style").filter(|s| !s.trim().is_empty());
            // As an agent team (`claude-team`), led by the configured agent.
            let team = body.get("team").and_then(|t| t.as_bool()).unwrap_or(false);
            let sent = state.commands.lock().map(|s| s.send(Command::Start { account, folder, prompt, style, team }).is_ok()).unwrap_or(false);
            if sent {
                reply(StatusCode::ACCEPTED, &serde_json::json!({ "ok": true }))
            } else {
                reply(StatusCode::SERVICE_UNAVAILABLE, &serde_json::json!({ "error": "Brain is not listening" }))
            }
        }
        // `wait=<version>`: answers once the transcript changed (its size is the version).
        (Method::GET, ["v1", "sessions", account, id, "messages"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let transcript = transcript_of(&state, &key);
            let size = || transcript.as_ref().and_then(|t| std::fs::metadata(t).ok()).map_or(0, |m| m.len());
            if let Some(seen) = query_param(req.uri().query(), "wait").and_then(|v| v.parse::<u64>().ok()) {
                let until = tokio::time::Instant::now() + WAIT;
                while size() == seen && tokio::time::Instant::now() < until {
                    tokio::time::sleep(WAIT_STEP).await;
                }
            }
            let version = size();
            // `limit`: older messages on request, up to a whole conversation.
            let limit = query_param(req.uri().query(), "limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(MESSAGE_LIMIT).clamp(1, MESSAGE_LIMIT_MAX);
            off_thread(state.clone(), move |state| {
                let (list, more) = messages(state, &key, limit);
                let mut response = with_version(reply(StatusCode::OK, &list), version);
                response.headers_mut().insert("brain-more", hyper::header::HeaderValue::from_static(if more { "1" } else { "0" }));
                response
            })
            .await
        }
        // The whole conversation searched; each hit says how far back it is (0: the newest).
        (Method::GET, ["v1", "sessions", account, id, "search"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let Some(text) = query_param(req.uri().query(), "q").filter(|q| q.trim().chars().count() >= 2) else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "q" })));
            };
            off_thread(state.clone(), move |state| reply(StatusCode::OK, &search(state, &key, &text))).await
        }
        // One message in full, which the list clips: by its time, role and place among those
        // with the same time.
        (Method::GET, ["v1", "sessions", account, id, "message"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let query = req.uri().query();
            let (Some(ts), Some(role)) = (query_param(query, "ts").and_then(|t| t.parse::<i64>().ok()), query_param(query, "role")) else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "ts and role" })));
            };
            let nth = query_param(query, "nth").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
            off_thread(state.clone(), move |state| match full_message(state, &key, ts, &role, nth) {
                Some(message) => reply(StatusCode::OK, &message),
                None => not_found(),
            })
            .await
        }
        (Method::POST, ["v1", "sessions", account, id, "control"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let Some(body) = read_json(req).await else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })));
            };
            let command = match body.get("action").and_then(|a| a.as_str()) {
                Some("interrupt") => Command::Interrupt { key },
                // What Brain types for it: the session's own command.
                Some("compact") => Command::Reply { key, text: "/compact".into() },
                Some("resume") => Command::Resume { key },
                Some("rename") => match body.get("name").and_then(|n| n.as_str()).and_then(session_name) {
                    Some(name) => Command::Reply { key, text: format!("/rename {name}") },
                    None => return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "name" }))),
                },
                Some("end") => Command::End { key },
                Some("team") => Command::Team { key, on: body.get("on").and_then(|o| o.as_bool()).unwrap_or(true) },
                Some("style") => match body.get("name").and_then(|n| n.as_str()).and_then(session_name) {
                    Some(name) => Command::Reply { key, text: format!("/output-style {name}") },
                    None => return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "name" }))),
                },
                Some("restart") => match crate::app::restart::parse_flags(body.get("flags").and_then(|f| f.as_str()).unwrap_or_default()) {
                    Ok(flags) => Command::Restart { key, flags },
                    Err(error) => return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": error }))),
                },
                _ => return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "action" }))),
            };
            send_command(&state, command)
        }
        // The session's agents and their messages, as its mod reported them.
        (Method::GET, ["v1", "sessions", account, id, "team"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            off_thread(state.clone(), move |state| {
                let team = brain_core::teams::load(&home_dir(), &key.id).unwrap_or_default();
                // Tokens per agent, from their transcripts beside the session's.
                let usage = transcript_of(state, &key).map(|t| brain_core::teams::usage(&t)).unwrap_or_default();
                reply(StatusCode::OK, &serde_json::json!({ "members": team.members, "messages": team.messages, "usage": usage }))
            })
            .await
        }
        // What `/…` completes to (as the session's mod reported it) and the styles it can switch to.
        (Method::GET, ["v1", "sessions", account, id, "commands"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            off_thread(state.clone(), move |state| {
                let config_dir = state.accounts.iter().find(|a| a.id == key.account).map(|a| a.config_dir.clone()).unwrap_or_default();
                reply(
                    StatusCode::OK,
                    &serde_json::json!({
                        "commands": brain_core::commands::load(&home_dir(), &key.id),
                        "styles": brain_core::styles::available(&config_dir, None),
                    }),
                )
            })
            .await
        }
        // Git and the files themselves are read off the server's only thread.
        (Method::GET, ["v1", "sessions", account, id, "files"]) => match session_key(&state, account, id) {
            Some(key) => off_thread(state.clone(), move |state| reply(StatusCode::OK, &files_view(state, &key))).await,
            None => not_found(),
        },
        (Method::GET, ["v1", "sessions", account, id, what @ ("diff" | "file")]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let Some(path) = query_param(req.uri().query(), "path") else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "path" })));
            };
            if *what == "diff" {
                off_thread(state.clone(), move |state| diff_reply(state, &key, &path)).await
            } else {
                off_thread(state.clone(), move |state| file_reply(state, &key, &path)).await
            }
        }
        (Method::POST, ["v1", "sessions", account, id, "attachments"]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let content_type = req.headers().get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
            let Some(extension) = attachment_extension(content_type) else {
                return Ok(reply(StatusCode::UNSUPPORTED_MEDIA_TYPE, &serde_json::json!({ "error": "type" })));
            };
            let name = query_param(req.uri().query(), "name").map(|n| file_stem(&n)).unwrap_or_default();
            let Ok(body) = Limited::new(req.into_body(), ATTACHMENT_LIMIT).collect().await.map(|b| b.to_bytes()) else {
                return Ok(reply(StatusCode::PAYLOAD_TOO_LARGE, &serde_json::json!({ "error": "too large" })));
            };
            if body.is_empty() {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "empty" })));
            }
            match save_attachment(&state.inbox, &key, &name, extension, &body) {
                Ok(path) => reply(StatusCode::CREATED, &serde_json::json!({ "path": path })),
                Err(_) => reply(StatusCode::INTERNAL_SERVER_ERROR, &serde_json::json!({ "error": "not saved" })),
            }
        }
        (Method::POST, ["v1", "sessions", account, id, action @ ("reply" | "permission" | "answer")]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let action = action.to_string();
            let Some(body) = read_json(req).await else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })));
            };
            let command = match action.as_str() {
                "reply" => body.get("text").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()).map(|text| Command::Reply { key, text: text.to_string() }),
                "answer" => {
                    let call = body.get("call").and_then(|c| c.as_str()).map(str::to_string);
                    match (call, answers_of(&body)) {
                        (Some(call), Some(answers)) => Some(Command::Answer { key, call, answers }),
                        _ => None,
                    }
                }
                _ => match (body.get("allow").and_then(|a| a.as_bool()), body.get("since_ms").and_then(|s| s.as_i64())) {
                    (Some(allow), Some(since_ms)) => Some(Command::Permission { key, allow, since_ms }),
                    _ => None,
                },
            };
            match command {
                Some(command) => send_command(&state, command),
                None => reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })),
            }
        }
        _ => not_found(),
    })
}

/// Hands a command to Brain's UI thread.
fn send_command(state: &State, command: Command) -> Response<Full<Bytes>> {
    if state.commands.lock().map(|s| s.send(command).is_ok()).unwrap_or(false) {
        reply(StatusCode::ACCEPTED, &serde_json::json!({ "ok": true }))
    } else {
        reply(StatusCode::SERVICE_UNAVAILABLE, &serde_json::json!({ "error": "Brain is not listening" }))
    }
}

/// An answer's body: `answers: [{picks: [..], other: ".."}]`, one per question, or as the first
/// phone app sends it, `picks: [..]` with one option per question.
fn answers_of(body: &serde_json::Value) -> Option<Vec<Picked>> {
    let indices = |value: Option<&serde_json::Value>| -> Option<Vec<usize>> {
        value?.as_array()?.iter().map(|v| v.as_u64().map(|n| n as usize)).collect()
    };
    let answers: Vec<Picked> = match body.get("answers").and_then(|a| a.as_array()) {
        Some(list) => list
            .iter()
            .map(|a| {
                let other = a.get("other").and_then(|o| o.as_str()).map(str::trim).filter(|o| !o.is_empty()).map(|o| clip(o, 2000));
                Some(Picked { picks: indices(a.get("picks")).unwrap_or_default(), other })
            })
            .collect::<Option<_>>()?,
        None => indices(body.get("picks"))?.into_iter().map(|pick| Picked { picks: vec![pick], other: None }).collect(),
    };
    let fits = !answers.is_empty() && answers.len() <= 8 && answers.iter().all(|a| (!a.picks.is_empty() || a.other.is_some()) && a.picks.len() <= 16);
    fits.then_some(answers)
}

fn with_version(mut response: Response<Full<Bytes>>, version: u64) -> Response<Full<Bytes>> {
    if let Ok(value) = hyper::header::HeaderValue::from_str(&version.to_string()) {
        response.headers_mut().insert("brain-version", value);
    }
    response
}

fn authorized(req: &Request<Incoming>, token: &str) -> bool {
    let given = req.headers().get(hyper::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    // Compares in constant time, so the answer's timing tells nothing about the token.
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

/// Only sessions of a known account, and ids that are plain session ids (no path tricks).
fn session_key(state: &State, account: &str, id: &str) -> Option<SessionKey> {
    let known = state.accounts.iter().any(|a| a.id == account);
    let plain = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    (known && plain).then(|| SessionKey { account: account.to_string(), id: id.to_string() })
}

#[derive(Serialize)]
struct MessageView {
    role: &'static str,
    tool: Option<String>,
    text: String,
    ts_ms: Option<i64>,
    /// The text was cut at [`MESSAGE_CHARS`]: `…/message` has it whole.
    clipped: bool,
    /// For a search hit: how many messages came after it (0: the newest).
    #[serde(skip_serializing_if = "Option::is_none")]
    back: Option<usize>,
}

impl MessageView {
    fn of(message: brain_core::transcript::Message, max: usize) -> MessageView {
        let clipped = message.text.chars().nth(max).is_some();
        MessageView {
            role: role_name(message.role),
            tool: message.tool,
            text: clip(&message.text, max),
            ts_ms: message.ts.map(|t| t.timestamp_millis()),
            clipped,
            back: None,
        }
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
        Role::System => "system",
    }
}

fn transcript_of(state: &State, key: &SessionKey) -> Option<PathBuf> {
    let account = state.accounts.iter().find(|a| a.id == key.account)?;
    find_transcript(account, &key.id)
}

/// The last `limit` messages, and whether older ones exist. Up to the usual window the end of
/// the transcript is read; further back the whole of it.
fn messages(state: &State, key: &SessionKey, limit: usize) -> (Vec<MessageView>, bool) {
    let Some(path) = transcript_of(state, key) else { return (Vec::new(), false) };
    let (list, more) = if limit <= MESSAGE_LIMIT {
        // One more than asked tells whether there are older ones.
        let mut list = read_recent_messages(&path, limit + 1);
        let more = list.len() > limit;
        list.drain(..list.len().saturating_sub(limit));
        (list, more)
    } else {
        let mut list = read_all_messages(&path);
        let more = list.len() > limit;
        list.drain(..list.len().saturating_sub(limit));
        (list, more)
    };
    (list.into_iter().map(|m| MessageView::of(m, MESSAGE_CHARS)).collect(), more)
}

/// Messages whose text contains `text` (any case), newest first, at most [`SEARCH_HITS`].
fn search(state: &State, key: &SessionKey, text: &str) -> Vec<MessageView> {
    let Some(path) = transcript_of(state, key) else { return Vec::new() };
    let needle = text.trim().to_lowercase();
    let all = read_all_messages(&path);
    let total = all.len();
    all.into_iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.text.to_lowercase().contains(&needle))
        .take(SEARCH_HITS)
        .map(|(at, m)| MessageView { back: Some(total - 1 - at), ..MessageView::of(m, SEARCH_CHARS) })
        .collect()
}

/// The `nth` message with this time and role, unclipped (up to [`FULL_CHARS`]).
fn full_message(state: &State, key: &SessionKey, ts_ms: i64, role: &str, nth: usize) -> Option<MessageView> {
    let path = transcript_of(state, key)?;
    read_all_messages(&path)
        .into_iter()
        .filter(|m| role_name(m.role) == role && m.ts.map(|t| t.timestamp_millis()) == Some(ts_ms))
        .nth(nth)
        .map(|m| MessageView::of(m, FULL_CHARS))
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// What a session changed in its folder (git) and the files it touched (its transcript).
#[derive(Serialize)]
struct FilesView {
    cwd: Option<String>,
    branch: Option<String>,
    changes: Vec<ChangeView>,
    touched: Vec<TouchedView>,
}

#[derive(Serialize)]
struct ChangeView {
    /// Relative to the session's folder.
    path: String,
    /// `git status --porcelain` code, e.g. ` M`, `A `, `??`.
    status: String,
    added: Option<u64>,
    removed: Option<u64>,
}

#[derive(Serialize)]
struct TouchedView {
    path: String,
    /// `written`, `edited`, `read`, `attached` or `shared`.
    touches: Vec<&'static str>,
    last_ms: i64,
}

fn files_view(state: &State, key: &SessionKey) -> FilesView {
    let transcript = transcript_of(state, key);
    let cwd = transcript.as_deref().and_then(cwd_of);
    let changes = cwd.as_deref().and_then(|c| changes_of(Path::new(c)));
    let touched = transcript.map(|t| session_files(&t)).unwrap_or_default();
    FilesView {
        branch: changes.as_ref().and_then(|c| c.branch.clone()),
        changes: changes
            .map(|c| c.files.into_iter().map(|f| ChangeView { path: f.path, status: f.status, added: f.added, removed: f.removed }).collect())
            .unwrap_or_default(),
        touched: touched
            .into_iter()
            .take(TOUCHED_LIMIT)
            .map(|f| TouchedView { touches: f.touches.iter().map(touch_name).collect(), path: f.path, last_ms: f.last_ms })
            .collect(),
        cwd,
    }
}

fn touch_name(touch: &Touch) -> &'static str {
    match touch {
        Touch::Written => "written",
        Touch::Edited => "edited",
        Touch::Read => "read",
        Touch::Attached => "attached",
        Touch::Shared => "shared",
    }
}

/// The diff of one file the session's folder lists as changed; nothing else.
fn diff_reply(state: &State, key: &SessionKey, path: &str) -> Response<Full<Bytes>> {
    let Some(cwd) = transcript_of(state, key).and_then(|t| cwd_of(&t)).map(PathBuf::from) else { return not_found() };
    let Some(change) = changes_of(&cwd).and_then(|c| c.files.into_iter().find(|f| f.path == path)) else { return not_found() };
    match diff_of(&cwd, &change) {
        Some(diff) => raw(StatusCode::OK, "text/plain; charset=utf-8", clip_bytes(&diff, DIFF_LIMIT).into_bytes()),
        None => not_found(),
    }
}

/// A file the session touched (by its transcript) or changed in its folder, nothing else: the
/// token opens the sessions, not the whole disk.
fn file_reply(state: &State, key: &SessionKey, path: &str) -> Response<Full<Bytes>> {
    if !Path::new(path).is_absolute() {
        return reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "absolute path" }));
    }
    let Some(transcript) = transcript_of(state, key) else { return not_found() };
    let touched = session_files(&transcript).iter().any(|f| f.path == path);
    let changed = || {
        cwd_of(&transcript).is_some_and(|cwd| {
            let cwd = Path::new(&cwd);
            changes_of(cwd).is_some_and(|c| c.files.iter().any(|f| cwd.join(&f.path) == Path::new(path)))
        })
    };
    if !touched && !changed() {
        return not_found();
    }
    let Ok(meta) = std::fs::metadata(path) else { return not_found() };
    if !meta.is_file() {
        return not_found();
    }
    if meta.len() > FILE_LIMIT {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, &serde_json::json!({ "error": "too large" }));
    }
    match std::fs::read(path) {
        Ok(bytes) => raw(StatusCode::OK, content_type_of(path, &bytes), bytes),
        Err(_) => not_found(),
    }
}

fn content_type_of(path: &str, bytes: &[u8]) -> &'static str {
    let extension = Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        _ if std::str::from_utf8(bytes).is_ok() => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// What the phone may attach, by its content type: what Claude reads.
fn attachment_extension(content_type: &str) -> Option<&'static str> {
    let essence = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    Some(match essence.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/markdown" => "md",
        _ => return None,
    })
}

/// The phone's file name as a plain stem (letters, digits, `-`, `_`; 40 at most): it ends up
/// in a path Claude reads, so no separators, dots or spaces.
fn file_stem(name: &str) -> String {
    let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let mut out = String::new();
    for c in stem.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
        if out.len() >= 40 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// `inbox/<session id>/<when>-<random>[-<name>].<ext>`, only the user can read it. The path
/// goes back to the phone, which names it in the reply.
fn save_attachment(inbox: &Path, key: &SessionKey, name: &str, extension: &str, bytes: &[u8]) -> Result<String, String> {
    create_dir(inbox).map_err(|e| e.to_string())?;
    let folder = inbox.join(&key.id);
    create_dir(&folder).map_err(|e| e.to_string())?;
    let mut random = [0u8; 4];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut random).map_err(|_| "no randomness".to_string())?;
    let tag: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file = if name.is_empty() { format!("{stamp}-{tag}.{extension}") } else { format!("{stamp}-{tag}-{name}.{extension}") };
    write_private(&folder, &file, bytes)?;
    Ok(folder.join(file).to_string_lossy().into_owned())
}

/// At most `max` bytes, cut at a character boundary.
fn clip_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…", &text[..cut])
}

/// A query parameter, percent-decoded (`+` stays a plus).
/// A session name from the phone as `/rename` takes it: one line without control characters,
/// at most 80 characters. `None` when nothing is left.
fn session_name(name: &str) -> Option<String> {
    let line: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let name: String = line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    (!name.is_empty()).then_some(name)
}

/// The folders in `path` (the home folder without one), or `None` when it is no folder inside
/// the home folder.
fn folder_list(home: &Path, path: Option<&str>) -> Option<FolderList> {
    let dir = brain_core::folders::inside(home, path.map_or(home, Path::new))?;
    let parent = dir.parent().and_then(|p| brain_core::folders::inside(home, p)).filter(|_| Some(dir.as_path()) != home.canonicalize().ok().as_deref());
    Some(FolderList {
        path: dir.display().to_string(),
        parent: parent.map(|p| p.display().to_string()),
        folders: brain_core::folders::subfolders(&dir).iter().map(|p| p.display().to_string()).collect(),
    })
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    query?
        .split('&')
        .find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == name).then(|| decode(value)).flatten()
        })
        .filter(|v| !v.is_empty())
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

async fn off_thread(state: Arc<State>, work: impl FnOnce(&State) -> Response<Full<Bytes>> + Send + 'static) -> Response<Full<Bytes>> {
    tokio::task::spawn_blocking(move || work(&state))
        .await
        .unwrap_or_else(|_| reply(StatusCode::INTERNAL_SERVER_ERROR, &serde_json::json!({ "error": "failed" })))
}

async fn read_json(req: Request<Incoming>) -> Option<serde_json::Value> {
    let body = Limited::new(req.into_body(), BODY_LIMIT).collect().await.ok()?.to_bytes();
    serde_json::from_slice(&body).ok()
}

fn reply<T: Serialize>(status: StatusCode, value: &T) -> Response<Full<Bytes>> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

fn raw(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, content_type)
        .header(hyper::header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

fn not_found() -> Response<Full<Bytes>> {
    reply(StatusCode::NOT_FOUND, &serde_json::json!({ "error": "not found" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_pairing_url_parts() {
        assert_eq!(encode("Martin's Mac"), "Martin%27s%20Mac");
        assert_eq!(encode("a.local,192.168.1.2"), "a.local%2C192.168.1.2");
    }

    #[test]
    fn the_pairing_code_is_the_fingerprints_first_64_bits() {
        assert_eq!(short_code("071f68951f9e01d62362164a55c07d800e580a1f6e35ae51114555ae3f590238"), "071f 6895 1f9e 01d6");
    }

    #[test]
    fn clips_long_messages_at_a_character_boundary() {
        assert_eq!(clip("äöü", 2), "äö…");
        assert_eq!(clip("short", 10), "short");
    }

    /// The real server on a free port: TLS, the token, a listed session, a reply handed over.
    #[test]
    fn serves_sessions_to_the_token_holder_only() {
        let dir = std::env::temp_dir().join(format!("brain-link-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let account = Account { id: "main".into(), config_dir: dir.join("claude") };
        let link = Link::start_in(dir.clone(), port, vec![account]).unwrap();
        link.publish(vec![View {
            account: "main".into(),
            id: "0331-ab".into(),
            name: "fin".into(),
            phase: "your_turn",
            group: "attention",
            headline: Some("Done".into()),
            since_ms: 1,
            folder: Some("fin".into()),
            permission_open: false,
            permission_since_ms: None,
            question: None,
            can_reply: true,
            team_questions: Vec::new(),
            team_active: None,
            in_team: false,
        }]);
        let token = std::fs::read_to_string(dir.join("token")).unwrap();
        let curl = |args: &[&str]| {
            let url_base = format!("https://127.0.0.1:{port}");
            let mut command = std::process::Command::new("curl");
            command.args(["-sk", "-o", "-", "-w", "\n%{http_code}"]);
            for arg in args {
                command.arg(arg.replace("BASE", &url_base));
            }
            String::from_utf8(command.output().unwrap().stdout).unwrap()
        };
        let bearer = format!("Authorization: Bearer {token}");

        assert!(curl(&["BASE/v1/sessions"]).ends_with("401"));
        let hello = curl(&["-H", &bearer, "BASE/v1/hello"]);
        assert!(hello.ends_with("200") && hello.contains("\"hosts\":["), "{hello}");
        assert!(curl(&["-H", "Authorization: Bearer wrong", "BASE/v1/sessions"]).ends_with("401"));
        let listed = curl(&["-H", &bearer, "BASE/v1/sessions"]);
        assert!(listed.ends_with("200") && listed.contains("\"name\":\"fin\""), "{listed}");
        assert!(curl(&["-H", &bearer, "BASE/v1/sessions/other/0331-ab/messages"]).ends_with("404"));
        assert!(curl(&["-H", &bearer, "BASE/v1/sessions/main/..%2f..%2fetc/messages"]).ends_with("404"));
        let sent = curl(&["-H", &bearer, "-H", "Content-Type: application/json", "-d", "{\"text\":\"weiter\"}", "BASE/v1/sessions/main/0331-ab/reply"]);
        assert!(sent.ends_with("202"), "{sent}");
        // An answer that doesn't say which dialog it is for is refused.
        assert!(curl(&["-H", &bearer, "-d", "{\"allow\":true}", "BASE/v1/sessions/main/0331-ab/permission"]).ends_with("400"));
        assert!(curl(&["-H", &bearer, "-d", "{\"allow\":true,\"since_ms\":7}", "BASE/v1/sessions/main/0331-ab/permission"]).ends_with("202"));
        assert!(curl(&["-H", &bearer, "-d", "{\"call\":\"toolu_1\"}", "BASE/v1/sessions/main/0331-ab/answer"]).ends_with("400"));
        assert!(curl(&["-H", &bearer, "-d", "{\"call\":\"toolu_1\",\"picks\":[1,0]}", "BASE/v1/sessions/main/0331-ab/answer"]).ends_with("202"));
        // Several picks and own words; a question answered with nothing is refused.
        let answers = "{\"call\":\"toolu_2\",\"answers\":[{\"picks\":[0,2]},{\"picks\":[],\"other\":\" lieber so \"}]}";
        assert!(curl(&["-H", &bearer, "-d", answers, "BASE/v1/sessions/main/0331-ab/answer"]).ends_with("202"));
        assert!(curl(&["-H", &bearer, "-d", "{\"call\":\"t\",\"answers\":[{\"picks\":[]}]}", "BASE/v1/sessions/main/0331-ab/answer"]).ends_with("400"));
        link.publish_more(Vec::new(), Places { accounts: vec!["main".into()], folders: vec!["/w/fin".into()], styles: Vec::new() });
        assert!(curl(&["-H", &bearer, "-d", "{\"account\":\"main\",\"folder\":\"/etc\"}", "BASE/v1/sessions"]).ends_with("400"));
        assert!(curl(&["-H", &bearer, "-d", "{\"account\":\"main\",\"folder\":\"/w/fin\",\"prompt\":\"los\"}", "BASE/v1/sessions"]).ends_with("202"));
        // Steering: interrupt, compact and rename (typed as the session's commands), resume, restart with flags, end.
        let control = |body: &str| curl(&["-H", &bearer, "-d", body, "BASE/v1/sessions/main/0331-ab/control"]);
        for body in ["{\"action\":\"interrupt\"}", "{\"action\":\"compact\"}", "{\"action\":\"resume\"}", "{\"action\":\"restart\",\"flags\":\"--model opus\"}", "{\"action\":\"rename\",\"name\":\"Neue\\nAusgabe\"}", "{\"action\":\"end\"}", "{\"action\":\"style\",\"name\":\"Concise\"}", "{\"action\":\"team\",\"on\":true}"] {
            assert!(control(body).ends_with("202"), "{body}");
        }
        // Brain's own flags and unknown actions are refused.
        assert!(control("{\"action\":\"restart\",\"flags\":\"--resume x\"}").ends_with("400"));
        assert!(control("{\"action\":\"reboot\"}").ends_with("400"));
        assert!(control("{\"action\":\"rename\",\"name\":\" \"}").ends_with("400"), "a name is needed");
        // The session's commands (none reported here) and the styles it can switch to.
        let listed = curl(&["-H", &bearer, "BASE/v1/sessions/main/0331-ab/commands"]);
        assert!(listed.contains("\"commands\":[]") && listed.contains("Explanatory") && listed.ends_with("200"), "{listed}");
        let key = SessionKey { account: "main".into(), id: "0331-ab".into() };
        assert_eq!(
            link.take_commands(),
            vec![
                Command::Reply { key: key.clone(), text: "weiter".into() },
                Command::Permission { key: key.clone(), allow: true, since_ms: 7 },
                Command::Answer {
                    key: key.clone(),
                    call: "toolu_1".into(),
                    answers: vec![Picked { picks: vec![1], other: None }, Picked { picks: vec![0], other: None }],
                },
                Command::Answer {
                    key: key.clone(),
                    call: "toolu_2".into(),
                    answers: vec![Picked { picks: vec![0, 2], other: None }, Picked { picks: vec![], other: Some("lieber so".into()) }],
                },
                Command::Start { account: "main".into(), folder: "/w/fin".into(), prompt: Some("los".into()), style: None, team: false },
                Command::Interrupt { key: key.clone() },
                Command::Reply { key: key.clone(), text: "/compact".into() },
                Command::Resume { key: key.clone() },
                Command::Restart { key: key.clone(), flags: vec!["--model".into(), "opus".into()] },
                Command::Reply { key: key.clone(), text: "/rename Neue Ausgabe".into() },
                Command::End { key: key.clone() },
                Command::Reply { key: key.clone(), text: "/output-style Concise".into() },
                Command::Team { key, on: true },
            ]
        );

        // A read with `wait` answers once the list changed, with its new version.
        let version = |out: &str| out.lines().find_map(|l| l.strip_prefix("brain-version: ")).map(|v| v.trim().to_string());
        let first = curl(&["-i", "-H", &bearer, "BASE/v1/sessions"]);
        let seen = version(&first).expect("a version");
        let waiting = {
            let (bearer, seen) = (bearer.clone(), seen.clone());
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let out = std::process::Command::new("curl")
                    .args(["-sk", "-i", "-H", &bearer, &format!("https://127.0.0.1:{port}/v1/sessions?wait={seen}")])
                    .output()
                    .unwrap();
                (started.elapsed(), String::from_utf8(out.stdout).unwrap())
            })
        };
        std::thread::sleep(Duration::from_millis(600));
        link.publish(Vec::new());
        let (took, answer) = waiting.join().unwrap();
        assert!(took >= Duration::from_millis(500) && took < Duration::from_secs(5), "{took:?}");
        assert_ne!(version(&answer), Some(seen), "{answer}");
        assert!(answer.trim_end().ends_with("[]"), "{answer}");
        drop(link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attachment_names_become_plain_stems() {
        assert_eq!(file_stem("Screenshot 2026-10-14 at 15.01.png"), "Screenshot-2026-10-14-at-15-01");
        assert_eq!(file_stem("../../etc/passwd"), "passwd");
        assert_eq!(file_stem("ü.jpg"), "");
        assert_eq!(attachment_extension("image/PNG; charset=binary"), Some("png"));
        assert_eq!(attachment_extension("application/x-sh"), None);
    }

    #[test]
    fn reads_percent_encoded_query_parameters() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home/Work/app")).unwrap();
        let home = root.path().join("home").canonicalize().unwrap();
        let top = folder_list(&home, None).unwrap();
        assert_eq!((top.parent, top.folders.len()), (None, 1));
        let work = folder_list(&home, Some(&top.folders[0])).unwrap();
        assert_eq!(work.parent.as_deref(), Some(top.path.as_str()));
        assert!(work.folders[0].ends_with("/Work/app"));
        assert!(folder_list(&home, Some(root.path().to_str().unwrap())).is_none(), "nothing outside the home folder");

        assert_eq!(query_param(Some("path=%2Fw%2Fa%20b%2B.txt&x=1"), "path").as_deref(), Some("/w/a b+.txt"));
        assert_eq!(query_param(Some("path=%zz"), "path"), None);
        assert_eq!(query_param(Some("path="), "path"), None);
        assert_eq!(query_param(None, "path"), None);
    }

    #[test]
    fn clips_a_diff_at_a_character_boundary() {
        assert_eq!(clip_bytes("aä", 2), "a\n…");
        assert_eq!(clip_bytes("ab", 2), "ab");
    }

    /// Older messages on request, a search through the whole conversation with how far back each
    /// hit is, and a clipped message in full.
    #[test]
    fn pages_searches_and_unclips_a_conversation() {
        let dir = std::env::temp_dir().join(format!("brain-link-history-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("claude").join("projects").join("-w");
        std::fs::create_dir_all(&project).unwrap();
        let long = "x".repeat(MESSAGE_CHARS + 500);
        let mut lines: Vec<String> = (0..100)
            .map(|i| serde_json::json!({ "type": "user", "timestamp": format!("2026-10-14T09:{:02}:{:02}Z", i / 60, i % 60), "message": { "role": "user", "content": format!("Frage {i}") } }).to_string())
            .collect();
        lines.push(serde_json::json!({ "type": "assistant", "timestamp": "2026-10-14T10:00:00Z", "message": { "role": "assistant", "content": [{ "type": "text", "text": long }] } }).to_string());
        std::fs::write(project.join("0331-ab.jsonl"), lines.join("\n")).unwrap();

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let link = Link::start_in(dir.join("link"), port, vec![Account { id: "main".into(), config_dir: dir.join("claude") }]).unwrap();
        let bearer = format!("Authorization: Bearer {}", std::fs::read_to_string(dir.join("link").join("token")).unwrap());
        let get = |path: &str| {
            let out = std::process::Command::new("curl")
                .args(["-sk", "-i", "-H", &bearer, &format!("https://127.0.0.1:{port}/v1/sessions/main/0331-ab/{path}")])
                .output()
                .unwrap();
            let text = String::from_utf8(out.stdout).unwrap();
            let (head, body) = text.split_once("\r\n\r\n").unwrap();
            (head.to_lowercase(), serde_json::from_str::<serde_json::Value>(body).unwrap_or_default())
        };

        let (head, recent) = get("messages");
        assert_eq!(recent.as_array().unwrap().len(), MESSAGE_LIMIT);
        assert!(head.contains("brain-more: 1"), "{head}");
        let last = &recent[MESSAGE_LIMIT - 1];
        assert_eq!((last["clipped"].as_bool(), last["text"].as_str().unwrap().chars().count()), (Some(true), MESSAGE_CHARS + 1));

        let (head, all) = get("messages?limit=200");
        assert_eq!(all.as_array().unwrap().len(), 101);
        assert!(head.contains("brain-more: 0"), "{head}");
        assert_eq!(all[0]["text"], "Frage 0");

        let (_, hits) = get("search?q=frage%201");
        let hits = hits.as_array().unwrap();
        // Frage 1, 10–19: newest first, each with how far back it is.
        assert_eq!(hits.len(), 11);
        assert_eq!((hits[0]["text"].as_str(), hits[0]["back"].as_u64()), (Some("Frage 19"), Some(81)));
        // One letter matches nearly everything: refused.
        assert_eq!(get("search?q=x").1["error"], "q");

        let ts = last["ts_ms"].as_i64().unwrap();
        let (_, full) = get(&format!("message?ts={ts}&role=assistant&nth=0"));
        assert_eq!((full["text"].as_str().map(str::len), full["clipped"].as_bool()), (Some(long.len()), Some(false)));
        assert_eq!(get(&format!("message?ts={ts}&role=user")).1["error"], "not found");
        drop(link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session's files (its git changes and what its transcript touched), one file's diff and
    /// bytes, nothing outside them; an attachment saved for a reply to name.
    #[test]
    fn serves_a_sessions_files_and_takes_attachments() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("brain-link-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap().status.success());
        git(&["init", "-q"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "start"]);
        std::fs::write(repo.join("new.txt"), "hallo\n").unwrap();
        let written = repo.join("new.txt").to_string_lossy().into_owned();
        let project = dir.join("claude").join("projects").join("-repo");
        std::fs::create_dir_all(&project).unwrap();
        let line = serde_json::json!({
            "type": "assistant", "cwd": repo, "timestamp": "2026-10-14T09:00:00Z",
            "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": "t1", "name": "Write", "input": { "file_path": written } }] }
        });
        std::fs::write(project.join("0331-ab.jsonl"), line.to_string()).unwrap();

        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let account = Account { id: "main".into(), config_dir: dir.join("claude") };
        let link = Link::start_in(dir.join("link"), port, vec![account]).unwrap();
        let token = std::fs::read_to_string(dir.join("link").join("token")).unwrap();
        let bearer = format!("Authorization: Bearer {token}");
        let curl = |args: &[&str]| {
            let mut command = std::process::Command::new("curl");
            command.args(["-sk", "-o", "-", "-w", "\n%{http_code}", "-H", &bearer]);
            for arg in args {
                command.arg(arg.replace("BASE", &format!("https://127.0.0.1:{port}/v1/sessions/main/0331-ab")));
            }
            String::from_utf8(command.output().unwrap().stdout).unwrap()
        };

        let files = curl(&["BASE/files"]);
        assert!(files.ends_with("200"), "{files}");
        assert!(files.contains("\"path\":\"new.txt\",\"status\":\"??\"") && files.contains("\"touches\":[\"written\"]"), "{files}");
        let diff = curl(&["BASE/diff?path=new.txt"]);
        assert!(diff.ends_with("200") && diff.contains("+hallo"), "{diff}");
        assert!(curl(&["BASE/diff?path=other.txt"]).ends_with("404"));
        let encoded: String = written.bytes().map(|b| format!("%{b:02X}")).collect();
        let file = curl(&[&format!("BASE/file?path={encoded}")]);
        assert_eq!(file, "hallo\n\n200");
        assert!(curl(&["BASE/file?path=%2Fetc%2Fhosts"]).ends_with("404"));
        assert!(curl(&["BASE/file?path=new.txt"]).ends_with("400"));

        assert!(curl(&["-H", "Content-Type: application/x-sh", "--data-binary", "echo", "BASE/attachments"]).ends_with("415"));
        let saved = curl(&["-H", "Content-Type: image/png", "--data-binary", "PNG!", "BASE/attachments?name=Bild%201.png"]);
        assert!(saved.ends_with("201"), "{saved}");
        let body = saved.rsplit_once('\n').unwrap().0;
        let path = serde_json::from_str::<serde_json::Value>(body).unwrap()["path"].as_str().unwrap().to_string();
        assert!(path.starts_with(&dir.join("link/inbox/0331-ab").to_string_lossy().into_owned()) && path.ends_with("-Bild-1.png"), "{path}");
        assert_eq!(std::fs::read(&path).unwrap(), b"PNG!");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        drop(link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// For trying the phone app against a real server without Brain: example sessions in a
    /// temporary folder, the pairing data in `pairing.txt` there, every command printed.
    /// `BRAIN_LINK_DEMO=/tmp/x cargo test -p brain-app demo_server -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn demo_server() {
        let dir = PathBuf::from(std::env::var("BRAIN_LINK_DEMO").expect("BRAIN_LINK_DEMO"));
        let account = Account { id: "main".into(), config_dir: dir.join("claude") };
        let project = account.config_dir.join("projects").join("-w-fin");
        std::fs::create_dir_all(&project).unwrap();
        let lines = [
            r#"{"type":"user","timestamp":"2026-10-10T09:00:00Z","message":{"role":"user","content":"Bau mir bitte den CSV-Export für die Buchungen."}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-10T09:00:20Z","message":{"role":"assistant","content":[{"type":"text","text":"Mach ich. Ich schaue mir zuerst an, wie die Buchungen gespeichert sind."},{"type":"tool_use","id":"t1","name":"Grep","input":{"pattern":"Booking"}}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-10-10T09:04:00Z","message":{"role":"assistant","content":[{"type":"text","text":"Der Export ist fertig: Datum, Betrag, Konto und Kategorie, UTF-8 mit BOM, damit Excel die Umlaute richtig zeigt. Soll ich ihn auch ins Menü hängen?"}]}}"#,
        ];
        // Its folder: a git repository with a changed and a new file, for the phone's files view.
        let repo = dir.join("fin");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap();
        git(&["init", "-q"]);
        std::fs::write(repo.join("src/bookings.rs"), "pub struct Booking {\n    pub amount: i64,\n}\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=demo", "-c", "user.email=demo@demo", "commit", "-q", "-m", "start"]);
        std::fs::write(repo.join("src/bookings.rs"), "pub struct Booking {\n    pub amount: i64,\n    pub category: String,\n}\n").unwrap();
        std::fs::write(repo.join("src/export.rs"), "/// CSV, UTF-8 with BOM.\npub fn export() -> String {\n    String::from(\"\\u{feff}date;amount\\n\")\n}\n").unwrap();
        let tool = |name: &str, file: &str| {
            serde_json::json!({
                "type": "assistant", "cwd": repo, "timestamp": "2026-10-10T09:03:00Z",
                "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": file, "name": name, "input": { "file_path": repo.join(file) } }] }
            })
            .to_string()
        };
        // An earlier part of the conversation, for loading older messages and searching, and a
        // report longer than the list shows.
        let mut older: Vec<String> = (0..60)
            .flat_map(|i| {
                let at = format!("2026-10-09T{:02}:{:02}:00Z", 8 + i / 60, i % 60);
                [
                    serde_json::json!({ "type": "user", "timestamp": at, "message": { "role": "user", "content": format!("Schritt {i}: prüf die Buchungen von Konto {}", 1000 + i) } }).to_string(),
                    serde_json::json!({ "type": "assistant", "timestamp": at, "message": { "role": "assistant", "content": [{ "type": "text", "text": format!("Konto {} geprüft, alles stimmt.", 1000 + i) }] } }).to_string(),
                ]
            })
            .collect();
        let report: String = (1..=120).map(|i| format!("{i}. Buchung {i}: Betrag {},00 €, Kategorie Haushalt\n", i * 7)).collect();
        older.push(serde_json::json!({ "type": "assistant", "timestamp": "2026-10-09T09:30:00Z", "message": { "role": "assistant", "content": [{ "type": "text", "text": format!("## Bericht\n\n{report}") }] } }).to_string());
        older.extend([lines[0].to_string(), lines[1].to_string(), tool("Edit", "src/bookings.rs"), tool("Write", "src/export.rs"), lines[2].to_string()]);
        let lines = older;
        std::fs::write(project.join("0331aa00-1111-2222-3333-444455556666.jsonl"), lines.join("\n")).unwrap();
        // Next to Brain's own port, so it runs while Brain (with Brain Link on) does.
        let port = PORT + 1;
        let link = Link::start_in(dir.join("link"), port, vec![account]).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let view = |id: &str, name: &str, phase: &'static str, group: &'static str, headline: &str, minutes: i64| View {
            account: "main".into(),
            id: id.into(),
            name: name.into(),
            phase,
            group,
            headline: Some(headline.into()),
            since_ms: now - minutes * 60_000,
            folder: Some(name.to_lowercase()),
            permission_open: phase == "needs_you" && id != "be75a6ae-0000-0000-0000-000000000003",
            permission_since_ms: (phase == "needs_you").then_some(now - minutes * 60_000),
            question: None,
            can_reply: phase != "ended",
            team_questions: Vec::new(),
            team_active: None,
            in_team: false,
        };
        link.publish(vec![
            view("9a1527b8-0000-0000-0000-000000000001", "Phoenix", "needs_you", "attention", "Bash: composer test --filter BookingExport", 2),
            view("0331aa00-1111-2222-3333-444455556666", "Finanzen", "your_turn", "attention", "Der Export ist fertig. Soll ich ihn auch ins Menü hängen?", 6),
            View {
                question: Some(brain_core::qa::OpenQuestion {
                    call: "toolu_demo".into(),
                    items: vec![
                        brain_core::qa::QuestionItem { question: "Welche Variante soll der Datepicker zuerst bekommen?".into(), options: vec!["Bereich".into(), "Monat".into(), "Woche".into()], multi_select: false },
                        brain_core::qa::QuestionItem { question: "Welche Ansichten brauchst du?".into(), options: vec!["Tag".into(), "Woche".into(), "Monat".into()], multi_select: true },
                    ],
                    multi_select: true,
                }),
                ..view("be75a6ae-0000-0000-0000-000000000003", "FUX", "needs_you", "attention", "Claude fragt: Welche Variante zuerst?", 1)
            },
            view("5e5e0000-0000-0000-0000-000000000005", "Docs", "working", "working", "Schreibt die Anleitung für den Export", 3),
            view("4d4aa5f4-0000-0000-0000-000000000004", "Haushalt", "ended", "ended", "Bank-Import läuft wieder", 180),
        ]);
        let limit = |used: f64, resets_in_h: i64, runs_out_in_h: Option<i64>| LimitView {
            used_percentage: used,
            resets_at: Some(now / 1000 + resets_in_h * 3600),
            runs_out_at: runs_out_in_h.map(|h| now / 1000 + h * 3600),
        };
        link.publish_more(
            vec![
                UsageView { account: "main".into(), five_hour: Some(limit(42.0, 3, None)), seven_day: Some(limit(67.0, 96, Some(40))) },
                UsageView { account: "second".into(), five_hour: Some(limit(8.0, 4, None)), seven_day: Some(limit(23.0, 120, None)) },
            ],
            Places { accounts: vec!["main".into(), "second".into()], folders: vec!["/Users/demo/Work/fin".into(), "/Users/demo/Work/phoenix".into(), "/Users/demo/Work/fux".into()], styles: Vec::new() },
        );
        let fingerprint: String = ring::digest::digest(&ring::digest::SHA256, &std::fs::read(dir.join("link/cert.der")).unwrap())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let token = std::fs::read_to_string(dir.join("link/token")).unwrap();
        let url = format!("brainlink://pair?v=1&n=Demo-Mac&h=127.0.0.1&p={port}&t={token}&f={fingerprint}");
        std::fs::write(dir.join("pairing.txt"), &url).unwrap();
        std::fs::write(dir.join("pairing.bmp"), qr_bmp(&url)).unwrap();
        println!("{url}");
        for _ in 0..1800 {
            for command in link.take_commands() {
                println!("COMMAND {command:?}");
                // A reply to "Finanzen" shows up in its conversation, and Claude answers: what a
                // phone waiting for news sees within a moment.
                if let Command::Reply { key, text } = &command {
                    if key.id == "0331aa00-1111-2222-3333-444455556666" {
                        let now = chrono::Utc::now().to_rfc3339();
                        let user = serde_json::json!({ "type": "user", "timestamp": now, "message": { "role": "user", "content": text } });
                        let claude = serde_json::json!({ "type": "assistant", "timestamp": now, "message": { "role": "assistant", "content": [{ "type": "text", "text": "Alles klar, mach ich." }] } });
                        let path = project.join("0331aa00-1111-2222-3333-444455556666.jsonl");
                        let mut transcript = std::fs::read_to_string(&path).unwrap_or_default();
                        transcript.push_str(&format!("\n{user}\n{claude}"));
                        std::fs::write(&path, transcript).unwrap();
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// The pairing code as a 24-bit BMP (8 px per module, 4 modules of quiet zone), for a phone
    /// to scan off the screen.
    fn qr_bmp(url: &str) -> Vec<u8> {
        let (width, modules) = qr_modules(url).unwrap();
        let (scale, quiet) = (8, 4);
        let side = (width + 2 * quiet) * scale;
        let row_len = (side * 3 + 3) / 4 * 4;
        let mut out = Vec::new();
        let size = 54 + row_len * side;
        out.extend(b"BM");
        out.extend((size as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend((side as i32).to_le_bytes());
        out.extend((side as i32).to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(24u16.to_le_bytes());
        out.extend([0u8; 24]);
        // Rows bottom-up.
        for y in (0..side).rev() {
            let mut row = Vec::with_capacity(row_len);
            for x in 0..side {
                let (mx, my) = ((x / scale) as isize - quiet as isize, (y / scale) as isize - quiet as isize);
                let inside = mx >= 0 && my >= 0 && (mx as usize) < width && (my as usize) < width;
                let dark = inside && modules[my as usize * width + mx as usize];
                row.extend(if dark { [0, 0, 0] } else { [255, 255, 255] });
            }
            row.resize(row_len, 0);
            out.extend(row);
        }
        out
    }

    #[test]
    fn draws_a_square_qr_code() {
        let (width, modules) = qr_modules("brainlink://pair?v=1&t=abc").unwrap();
        assert_eq!(modules.len(), width * width);
        assert!(modules.iter().any(|&dark| dark));
    }
}
