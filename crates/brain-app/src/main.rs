mod app;
mod chat_font;
#[cfg_attr(target_os = "linux", path = "clipboard_linux.rs")]
#[cfg_attr(target_os = "macos", path = "clipboard_macos.rs")]
mod clipboard;
mod companion;
#[cfg_attr(target_os = "linux", path = "chrome_linux.rs")]
#[cfg_attr(target_os = "macos", path = "chrome_macos.rs")]
mod chrome;
mod config;
mod conversation;
mod demo;
mod format;
#[cfg_attr(target_os = "linux", path = "hotkey_linux.rs")]
#[cfg_attr(target_os = "macos", path = "hotkey_macos.rs")]
mod hotkey;
mod i18n;
#[cfg_attr(target_os = "linux", path = "iterm_linux.rs")]
#[cfg_attr(target_os = "macos", path = "iterm_macos.rs")]
mod iterm;
mod konsil;
mod links;
mod markdown;
#[cfg_attr(target_os = "linux", path = "menubar_linux.rs")]
#[cfg_attr(target_os = "macos", path = "menubar_macos.rs")]
mod menubar;
mod model;
#[cfg_attr(target_os = "linux", path = "notify_linux.rs")]
#[cfg_attr(target_os = "macos", path = "notify_macos.rs")]
mod notify;
mod prefs;
mod shell_env;
mod style;
mod sticky;
#[cfg_attr(target_os = "linux", path = "trash_linux.rs")]
#[cfg_attr(target_os = "macos", path = "trash_macos.rs")]
mod trash;
mod ui;

fn main() -> iced::Result {
    shell_env::adopt_login_path();
    shell_env::forget_claude_session();
    // The app menu ("About …", "Quit …") is named after the process.
    #[cfg(target_os = "macos")]
    objc2_foundation::NSProcessInfo::processInfo().setProcessName(&objc2_foundation::NSString::from_str("Brain"));
    links::listen_for_urls();
    #[cfg(target_os = "linux")]
    desktop_fonts();
    chat_font::init();
    iced::application(app::Brain::new, app::Brain::update, ui::view)
        .title(app::Brain::title)
        .subscription(app::Brain::subscription)
        .theme(|_: &app::Brain| style::theme())
        .default_font(style::UI)
        .window(iced::window::Settings {
            size: iced::Size::new(1180.0, 760.0),
            min_size: Some(iced::Size::new(900.0, 520.0)),
            platform_specific: platform_window(),
            ..iced::window::Settings::default()
        })
        .centered()
        .run()
}

#[cfg(target_os = "macos")]
fn platform_window() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { title_hidden: true, titlebar_transparent: true, fullsize_content_view: true }
}

/// cosmic-text's fixed defaults ("Open Sans") are often missing; take fontconfig's.
#[cfg(target_os = "linux")]
fn desktop_fonts() {
    let family = |pattern: &str| {
        let out = std::process::Command::new("fc-match").args(["-f", "%{family[0]}", pattern]).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|f| out.status.success() && !f.is_empty())
    };
    let Ok(mut fonts) = iced::advanced::graphics::text::font_system().write() else { return };
    let db = fonts.raw().db_mut();
    if let Some(sans) = family("sans-serif") {
        db.set_sans_serif_family(sans);
    }
    if let Some(mono) = family("monospace") {
        db.set_monospace_family(mono);
    }
    if let Some(serif) = family("serif") {
        db.set_serif_family(serif);
    }
}

/// Matches brain.desktop.
#[cfg(target_os = "linux")]
fn platform_window() -> iced::window::settings::PlatformSpecific {
    iced::window::settings::PlatformSpecific { application_id: "brain".into(), ..Default::default() }
}
