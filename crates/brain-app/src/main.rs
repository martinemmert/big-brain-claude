mod app;
mod chat_font;
mod chrome;
mod config;
mod conversation;
mod demo;
mod format;
mod i18n;
mod links;
mod markdown;
mod menubar;
mod model;
mod notify;
mod prefs;
mod shell_env;
mod style;
mod trash;
mod ui;

fn main() -> iced::Result {
    shell_env::adopt_login_path();
    // The app menu ("About …", "Quit …") is named after the process.
    objc2_foundation::NSProcessInfo::processInfo().setProcessName(&objc2_foundation::NSString::from_str("Brain"));
    links::listen_for_urls();
    chat_font::init();
    iced::application(app::Brain::new, app::Brain::update, ui::view)
        .title(app::Brain::title)
        .subscription(app::Brain::subscription)
        .theme(|_: &app::Brain| style::theme())
        .default_font(style::UI)
        .window(iced::window::Settings {
            size: iced::Size::new(1180.0, 760.0),
            min_size: Some(iced::Size::new(900.0, 520.0)),
            platform_specific: iced::window::settings::PlatformSpecific {
                title_hidden: true,
                titlebar_transparent: true,
                fullsize_content_view: true,
            },
            ..iced::window::Settings::default()
        })
        .centered()
        .run()
}
