//! Notifications through `UNUserNotificationCenter`, so they come from Brain (its name, its icon)
//! and offer Open / Snooze. The framework only works inside an app bundle; when Brain runs as a
//! bare binary (`cargo run`) it falls back to `osascript`.

use std::sync::Mutex;

use block2::RcBlock;
use brain_core::state::SessionKey;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use crate::i18n::t;

const CATEGORY: &str = "brain.session";
const ACTION_OPEN: &str = "open";
const ACTION_SNOOZE: &str = "snooze";

/// What the user did with a notification; drained by the view on every tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Open(SessionKey),
    Snooze(SessionKey),
}

static RESPONSES: Mutex<Vec<Response>> = Mutex::new(Vec::new());

pub fn take_responses() -> Vec<Response> {
    std::mem::take(&mut *RESPONSES.lock().unwrap())
}

/// `brain|<account>|<pid>|<nonce>` — the session travels in the request identifier.
fn identifier(key: &SessionKey) -> String {
    format!("brain|{}|{}|{}", key.account, key.pid, chrono::Utc::now().timestamp_millis())
}

fn parse_identifier(id: &str) -> Option<SessionKey> {
    let mut parts = id.split('|');
    (parts.next()? == "brain").then_some(())?;
    let account = parts.next()?.to_string();
    let pid = parts.next()?.parse().ok()?;
    Some(SessionKey { account, pid })
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[name = "BrainNotificationDelegate"]
    #[ivars = ()]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Show banners even while Brain is the active app.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            let action = response.actionIdentifier().to_string();
            if let Some(key) = parse_identifier(&id) {
                let default = unsafe { UNNotificationDefaultActionIdentifier }.to_string();
                let event = if action == ACTION_SNOOZE {
                    Some(Response::Snooze(key))
                } else if action == ACTION_OPEN || action == default {
                    Some(Response::Open(key))
                } else {
                    None
                };
                if let Some(event) = event {
                    RESPONSES.lock().unwrap().push(event);
                }
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

pub struct Notifier {
    native: Option<(Retained<UNUserNotificationCenter>, Retained<Delegate>)>,
}

impl Notifier {
    pub fn new() -> Self {
        let in_bundle = NSBundle::mainBundle().bundleIdentifier().is_some();
        if !in_bundle {
            return Self { native: None };
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        let open = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(ACTION_OPEN),
            &NSString::from_str(t("Öffnen", "Open")),
            UNNotificationActionOptions::Foreground,
        );
        let snooze = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(ACTION_SNOOZE),
            &NSString::from_str(t("15 Min. pausieren", "Snooze 15 min")),
            UNNotificationActionOptions::empty(),
        );
        let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(CATEGORY),
            &NSArray::from_retained_slice(&[open, snooze]),
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));

        let handler = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &handler,
        );
        Self { native: Some((center, delegate)) }
    }

    /// `sound` only for sessions that call you (question or permission).
    pub fn post(&self, key: &SessionKey, title: &str, subtitle: &str, body: &str, sound: bool) {
        let Some((center, _)) = &self.native else {
            fallback(title, subtitle, body);
            return;
        };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setSubtitle(&NSString::from_str(subtitle));
        content.setBody(&NSString::from_str(body));
        content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
        content.setThreadIdentifier(&NSString::from_str(&format!("{}|{}", key.account, key.pid)));
        if sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier(key)),
            &content,
            None,
        );
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }
}

/// Outside an app bundle (development builds).
fn fallback(title: &str, subtitle: &str, body: &str) {
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!(
        "display notification {} with title {} subtitle {}",
        quote(body),
        quote(title),
        quote(subtitle)
    );
    let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_survives_the_round_trip_through_the_identifier() {
        let key = SessionKey { account: "second".into(), pid: 4711 };
        assert_eq!(parse_identifier(&identifier(&key)), Some(key));
        assert_eq!(parse_identifier("other|x|1|2"), None);
    }
}
