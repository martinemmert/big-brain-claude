//! Notifications over D-Bus with Open / Snooze; each waits for its answer on its own thread.

use std::sync::Mutex;

use brain_core::state::SessionKey;
use notify_rust::{Hint, Notification, Timeout};

use crate::i18n::t;

const ACTION_OPEN: &str = "open";
const ACTION_SNOOZE: &str = "snooze";
/// What the server reports when the notification itself is clicked.
const ACTION_DEFAULT: &str = "default";

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

pub struct Notifier;

impl Notifier {
    pub fn new() -> Self {
        Self
    }

    /// `sound` only for sessions that call you. The spec has no subtitle; it leads the body.
    pub fn post(&self, key: &SessionKey, title: &str, subtitle: &str, body: &str, sound: bool) {
        let mut notification = Notification::new();
        notification
            .appname("Brain")
            .icon("brain")
            .summary(title)
            .body(&[subtitle, body].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n"))
            .action(ACTION_DEFAULT, t("Öffnen", "Open"))
            .action(ACTION_OPEN, t("Öffnen", "Open"))
            .action(ACTION_SNOOZE, t("15 Min. pausieren", "Snooze 15 min"))
            .hint(Hint::DesktopEntry("brain".into()))
            .timeout(Timeout::Default);
        if sound {
            notification.hint(Hint::SoundName("message-new-instant".into()));
        }
        let key = key.clone();
        std::thread::spawn(move || {
            let Ok(handle) = notification.show() else { return };
            handle.wait_for_action(|action| {
                let response = match action {
                    ACTION_SNOOZE => Response::Snooze(key),
                    ACTION_OPEN | ACTION_DEFAULT => Response::Open(key),
                    _ => return,
                };
                RESPONSES.lock().unwrap().push(response);
            });
        });
    }
}
