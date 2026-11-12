mod app;
mod appmenu;
mod chat_font;
mod clipboard;
mod companion;
mod chrome;
mod config;
mod conversation;
mod demo;
mod folder_picker;
mod format;
mod hotkey;
mod i18n;
mod iterm;
mod links;
mod markdown;
mod menubar;
mod model;
mod notify;
mod popup;
mod prefs;
mod shell_env;
mod style;
mod trash;
mod tree;
mod ui;

fn main() -> iced::Result {
    shell_env::adopt_login_path();
    shell_env::forget_claude_session();
    // The app menu ("About …", "Quit …") is named after the process.
    objc2_foundation::NSProcessInfo::processInfo().setProcessName(&objc2_foundation::NSString::from_str("Brain"));
    links::listen_for_urls();
    chat_font::init();
    iced::daemon(app::Brain::new, app::Brain::update, ui::window_view)
        .title(app::Brain::title)
        .subscription(app::Brain::subscription)
        .theme(|_: &app::Brain, _| style::theme())
        .scale_factor(app::Brain::scale_of)
        .default_font(style::UI)
        .run()
}
