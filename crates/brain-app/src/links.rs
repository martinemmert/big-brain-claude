//! `brain://session/<account>/<pid>` links (Alfred's ⌥⏎, `brain show`) and the update check.

use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `'GURL'`: the Apple Event class and id macOS sends for a URL the app is registered for.
const GET_URL: u32 = u32::from_be_bytes(*b"GURL");
/// `'----'`: the event's direct object, here the URL.
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[name = "BrainURLHandler"]
    #[ivars = ()]
    struct Handler;

    unsafe impl NSObjectProtocol for Handler {}

    impl Handler {
        #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
        fn handle(&self, event: &AnyObject, _reply: &AnyObject) {
            // SAFETY: `event` is an NSAppleEventDescriptor; both methods exist on it.
            let url: Option<Retained<NSString>> = unsafe {
                let descriptor: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
                match descriptor {
                    Some(descriptor) => msg_send![&*descriptor, stringValue],
                    None => None,
                }
            };
            if let Some(url) = url {
                OPENED.lock().unwrap().push(url.to_string());
            }
        }
    }
);

/// Asks macOS to hand `brain://` URLs (registered in Info.plist) to Brain. The handler object
/// lives for the whole run.
pub fn listen_for_urls() {
    let handler: Retained<Handler> = unsafe { msg_send![super(Handler::alloc().set_ivars(())), init] };
    // SAFETY: the shared manager exists in every process; the selector matches `handle`.
    unsafe {
        let manager: Retained<AnyObject> = msg_send![objc2::class!(NSAppleEventManager), sharedAppleEventManager];
        let _: () = msg_send![
            &*manager,
            setEventHandler: &*handler,
            andSelector: sel!(handleGetURLEvent:withReplyEvent:),
            forEventClass: GET_URL,
            andEventID: GET_URL
        ];
    }
    std::mem::forget(handler);
}

/// Sessions asked for since the last call, as account and pid of their process.
pub fn take_sessions() -> Vec<(String, u32)> {
    std::mem::take(&mut *OPENED.lock().unwrap()).iter().filter_map(|u| parse(u)).collect()
}

fn parse(url: &str) -> Option<(String, u32)> {
    let rest = url.strip_prefix("brain://session/")?;
    let (account, pid) = rest.trim_end_matches('/').split_once('/')?;
    Some((account.to_string(), pid.parse().ok()?))
}

/// A newer release on GitHub: its version and page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub url: String,
}

const LATEST: &str = "https://api.github.com/repos/martinemmert/big-brain-claude/releases/latest";

/// Asks GitHub for the latest release (blocking; run off the UI thread).
pub fn check_for_update() -> Option<Update> {
    let out = std::process::Command::new("/usr/bin/curl")
        .args(["-fsSL", "-m", "10", "-H", "Accept: application/vnd.github+json", LATEST])
        .output()
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let url = json.get("html_url")?.as_str()?.to_string();
    newer(tag, env!("CARGO_PKG_VERSION")).then(|| Update { version: tag.trim_start_matches('v').to_string(), url })
}

fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((version(candidate), version(current)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_links_and_versions_are_parsed() {
        assert_eq!(parse("brain://session/second/4711"), Some(("second".to_string(), 4711)));
        assert_eq!(parse("brain://other/x"), None);
        assert!(newer("v0.3.0", "0.2.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("nightly", "0.2.0"));
    }
}
