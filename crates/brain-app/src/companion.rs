//! Brain Link: the companion app on the phone talks to Brain over HTTPS in the network both are
//! in. It is off until the user pairs a phone (⌘K → "Pair a phone").
//!
//! Pairing hands the phone a `brainlink://pair` URL (as a QR code the iPhone camera opens): the
//! addresses Brain listens on, a random token every request must carry, and the SHA-256 of
//! Brain's self-signed certificate, which the phone pins instead of trusting any CA. So nobody
//! on the same Wi-Fi can read along or pose as Brain, and without the token nobody gets in.
//!
//! The server only reads: a snapshot of the session list that Brain publishes on every refresh,
//! and transcripts. Replies and permission answers are handed to Brain's UI thread as
//! [`Command`]s, which does with them what it does for a reply typed in Brain.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use base64::Engine;
use brain_core::account::{home_dir, Account};
use brain_core::state::SessionKey;
use brain_core::transcript::{find_transcript, read_recent_messages, Role};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use serde::Serialize;
use tokio_rustls::rustls;

/// The port Brain Link listens on, on every interface.
pub const PORT: u16 = 48620;
/// Messages the phone gets per session, newest last.
const MESSAGE_LIMIT: usize = 80;
/// The longest message text sent; the phone shows the start of longer ones.
const MESSAGE_CHARS: usize = 6000;
/// The largest request body: a reply.
const BODY_LIMIT: usize = 64 * 1024;

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
    /// Brain can deliver a reply (its terminal, an iTerm tab, or a background session).
    pub can_reply: bool,
}

/// What the phone asked Brain to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Reply { key: SessionKey, text: String },
    /// `since_ms`: when the dialog the user answered opened ([`View::permission_since_ms`]).
    Permission { key: SessionKey, allow: bool, since_ms: i64 },
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
            accounts,
            sessions: RwLock::new(Vec::new()),
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

    pub fn publish(&self, views: Vec<View>) {
        if let Ok(mut sessions) = self.state.sessions.write() {
            *sessions = views;
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
    accounts: Vec<Account>,
    sessions: RwLock<Vec<View>>,
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
            reply(StatusCode::OK, &serde_json::json!({ "name": computer_name(), "version": env!("CARGO_PKG_VERSION"), "hosts": hosts() }))
        }
        (Method::GET, ["v1", "sessions"]) => {
            let sessions = state.sessions.read().map(|s| s.clone()).unwrap_or_default();
            reply(StatusCode::OK, &sessions)
        }
        (Method::GET, ["v1", "sessions", account, id, "messages"]) => match session_key(&state, account, id) {
            Some(key) => reply(StatusCode::OK, &messages(&state, &key)),
            None => not_found(),
        },
        (Method::POST, ["v1", "sessions", account, id, action @ ("reply" | "permission")]) => {
            let Some(key) = session_key(&state, account, id) else { return Ok(not_found()) };
            let action = action.to_string();
            let Some(body) = read_json(req).await else {
                return Ok(reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })));
            };
            let command = match action.as_str() {
                "reply" => body.get("text").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()).map(|text| Command::Reply { key, text: text.to_string() }),
                _ => match (body.get("allow").and_then(|a| a.as_bool()), body.get("since_ms").and_then(|s| s.as_i64())) {
                    (Some(allow), Some(since_ms)) => Some(Command::Permission { key, allow, since_ms }),
                    _ => None,
                },
            };
            match command {
                Some(command) => {
                    let sent = state.commands.lock().map(|s| s.send(command).is_ok()).unwrap_or(false);
                    if sent {
                        reply(StatusCode::ACCEPTED, &serde_json::json!({ "ok": true }))
                    } else {
                        reply(StatusCode::SERVICE_UNAVAILABLE, &serde_json::json!({ "error": "Brain is not listening" }))
                    }
                }
                None => reply(StatusCode::BAD_REQUEST, &serde_json::json!({ "error": "bad body" })),
            }
        }
        _ => not_found(),
    })
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
}

fn messages(state: &State, key: &SessionKey) -> Vec<MessageView> {
    let Some(account) = state.accounts.iter().find(|a| a.id == key.account) else { return Vec::new() };
    let Some(path) = find_transcript(account, &key.id) else { return Vec::new() };
    read_recent_messages(&path, MESSAGE_LIMIT)
        .into_iter()
        .map(|m| MessageView {
            role: match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                Role::System => "system",
            },
            tool: m.tool,
            text: clip(&m.text, MESSAGE_CHARS),
            ts_ms: m.ts.map(|t| t.timestamp_millis()),
        })
        .collect()
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
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
            can_reply: true,
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
        let key = SessionKey { account: "main".into(), id: "0331-ab".into() };
        assert_eq!(
            link.take_commands(),
            vec![Command::Reply { key: key.clone(), text: "weiter".into() }, Command::Permission { key, allow: true, since_ms: 7 }]
        );
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
            permission_open: phase == "needs_you",
            permission_since_ms: (phase == "needs_you").then_some(now - minutes * 60_000),
            can_reply: phase != "ended",
        };
        link.publish(vec![
            view("9a1527b8-0000-0000-0000-000000000001", "Phoenix", "needs_you", "attention", "Bash: composer test --filter BookingExport", 2),
            view("0331aa00-1111-2222-3333-444455556666", "Finanzen", "your_turn", "attention", "Der Export ist fertig. Soll ich ihn auch ins Menü hängen?", 6),
            view("be75a6ae-0000-0000-0000-000000000003", "FUX", "working", "working", "Baut die Datepicker-Varianten …", 0),
            view("4d4aa5f4-0000-0000-0000-000000000004", "Haushalt", "ended", "ended", "Bank-Import läuft wieder", 180),
        ]);
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
