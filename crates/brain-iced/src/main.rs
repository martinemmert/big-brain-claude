//! Prototype: Brain's session list, search and reply field in Iced, on top of brain-core.
//! Nothing is sent anywhere; it exists to try Iced's text fields against GPUI's.

use std::time::Duration;

use brain_core::account::{discover_accounts, home_dir, Account};
use brain_core::process::pid_alive;
use brain_core::sessions::read_session_files;
use brain_core::state::{Board, Phase, Session, SessionKey};
use brain_core::store::{Store, Tail};
use brain_core::transcript::{find_transcript, read_recent_messages, Role};
use chrono::{Days, Local};
use iced::widget::{button, column, container, markdown, operation, row, scrollable, text, text_editor, text_input, Space};
use iced::{border, color, Background, Border, Color, Element, Fill, Font, Length, Subscription, Task, Theme};

const INK: Color = color!(0x0f1322);
const CHROME: Color = color!(0x121728);
const SURFACE: Color = color!(0x161b2c);
const RAISED: Color = color!(0x1d2338);
const LINE: Color = color!(0x232a40);
const TEXT: Color = color!(0xd5d9e6);
const STRONG: Color = color!(0xf3f4f9);
const MUTED: Color = color!(0x8c93aa);
const FAINT: Color = color!(0x5f6782);
const CALLS: Color = color!(0xff5c6c);
const TURN: Color = color!(0xf2b84b);
const WORKING: Color = color!(0x5aa9ff);
const BACKGROUND: Color = color!(0x8f9cf5);
const ENDED: Color = color!(0x4a5272);

const SEARCH: &str = "search";

fn main() -> iced::Result {
    iced::application(Brain::new, Brain::update, Brain::view)
        .title("Brain · Iced prototype")
        .theme(|_: &Brain| brain_theme())
        .subscription(|_: &Brain| iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick))
        .window_size((1280.0, 820.0))
        .run()
}

fn brain_theme() -> Theme {
    Theme::custom(
        "Brain",
        iced::theme::Palette { background: INK, text: TEXT, primary: WORKING, success: color!(0x45d19a), warning: TURN, danger: CALLS },
    )
}

struct Brain {
    accounts: Vec<Account>,
    store: Store,
    board: Board,
    search: String,
    selected: Option<SessionKey>,
    messages: Vec<markdown::Item>,
    reply: text_editor::Content,
    status: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Search(String),
    Select(SessionKey),
    Reply(text_editor::Action),
    Send,
    Link(markdown::Uri),
}

impl Brain {
    fn new() -> (Self, Task<Message>) {
        let home = home_dir();
        let mut brain = Self {
            accounts: discover_accounts(&home),
            store: Store::new(Store::default_root(&home)),
            board: Board::default(),
            search: String::new(),
            selected: None,
            messages: Vec::new(),
            reply: text_editor::Content::new(),
            status: None,
        };
        brain.reload();
        brain.selected = brain.visible().first().map(|s| s.key.clone());
        brain.load_messages();
        (brain, operation::focus(SEARCH))
    }

    /// The last three days of events plus every account's status files.
    fn reload(&mut self) {
        let mut board = Board::default();
        let today = Local::now().date_naive();
        for back in (0..3).rev() {
            let Some(day) = today.checked_sub_days(Days::new(back)) else { continue };
            for event in Tail::new(self.store.file_for(day)).read_new() {
                board.apply_event(&event);
            }
        }
        for account in &self.accounts {
            for file in read_session_files(account) {
                board.apply_session_file(&account.id, &file, pid_alive(file.pid));
            }
        }
        self.board = board;
    }

    fn visible(&self) -> Vec<&Session> {
        self.board
            .sorted()
            .into_iter()
            .filter(|s| s.phase() != Phase::Ended || !self.search.is_empty())
            .filter(|s| s.matches(&self.search))
            .collect()
    }

    fn selected_session(&self) -> Option<&Session> {
        self.selected.as_ref().and_then(|k| self.board.get(k))
    }

    fn load_messages(&mut self) {
        let Some(session) = self.selected_session() else {
            self.messages.clear();
            return;
        };
        let account = self.accounts.iter().find(|a| a.id == session.key.account);
        let path = account.zip(session.session_id.as_deref()).and_then(|(a, id)| find_transcript(a, id));
        let markdown_text: String = path
            .map(|p| read_recent_messages(&p, 30))
            .unwrap_or_default()
            .iter()
            .map(|m| match m.role {
                Role::User => format!("**Du**\n\n{}\n\n", m.text),
                Role::Assistant => format!("**Claude**\n\n{}\n\n", m.text),
                Role::Tool => format!("`{} {}`\n\n", m.tool.as_deref().unwrap_or("Tool"), one_line(&m.text)),
                _ => String::new(),
            })
            .collect();
        self.messages = markdown::parse(&markdown_text).collect();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.reload(),
            Message::Search(query) => {
                self.search = query;
                if self.selected.as_ref().is_none_or(|k| !self.visible().iter().any(|s| s.key == *k)) {
                    self.selected = self.visible().first().map(|s| s.key.clone());
                    self.load_messages();
                }
            }
            Message::Select(key) => {
                self.selected = Some(key);
                self.load_messages();
            }
            Message::Reply(action) => self.reply.perform(action),
            Message::Send => {
                let chars = self.reply.text().trim_end().chars().count();
                self.status = Some(format!("Prototyp – nichts gesendet ({chars} Zeichen)."));
            }
            Message::Link(uri) => self.status = Some(format!("Link: {uri}")),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let list = column(self.visible().into_iter().map(|s| self.row(s))).spacing(2);
        let left = column![
            text_input("Sessions suchen …", &self.search).id(SEARCH).on_input(Message::Search).padding(10).size(14),
            scrollable(list).height(Fill),
        ]
        .spacing(12)
        .padding(16)
        .width(380);

        let detail: Element<'_, Message> = match self.selected_session() {
            None => container(text("Keine Session ausgewählt").color(MUTED)).center(Fill).into(),
            Some(s) => {
                let headline = s.headline().unwrap_or_else(|| "Noch keine Nachricht.".into());
                let header = column![
                    row![
                        text(s.display_name()).size(22).font(Font { weight: iced::font::Weight::Bold, ..Font::DEFAULT }).color(STRONG),
                        badge(&s.key.account),
                    ]
                    .spacing(10)
                    .align_y(iced::Center),
                    text(headline).size(14).color(TEXT),
                ]
                .spacing(8);
                let messages = scrollable(
                    container(markdown::view(&self.messages, markdown::Settings::with_text_size(14, brain_theme())).map(Message::Link))
                        .padding([0, 12]),
                )
                .height(Fill);
                let reply = text_editor(&self.reply)
                    .placeholder("Antwort … (mehrzeilig, Umlaute, ⌘Z, Markieren – probier’s aus)")
                    .on_action(Message::Reply)
                    .height(Length::Fixed(110.0))
                    .padding(10)
                    .size(14);
                column![
                    header,
                    messages,
                    reply,
                    row![
                        text(self.status.clone().unwrap_or_default()).size(12).color(FAINT),
                        Space::new().width(Fill),
                        button(text("Senden").size(13)).on_press(Message::Send).padding([6, 14]),
                    ]
                    .align_y(iced::Center),
                ]
                .spacing(14)
                .padding(24)
                .into()
            }
        };

        row![
            container(left).height(Fill).style(|_| panel(CHROME)),
            container(detail).width(Fill).height(Fill).style(|_| panel(INK)),
        ]
        .into()
    }

    fn row<'a>(&self, s: &'a Session) -> Element<'a, Message> {
        let selected = self.selected.as_ref() == Some(&s.key);
        let detail = s.headline().map(|h| one_line(&h)).unwrap_or_default();
        let content = row![
            text("●").size(10).color(phase_color(s.phase())),
            text(s.display_name()).size(13).color(STRONG),
            badge(&s.key.account),
            text(detail).size(12).color(MUTED).wrapping(text::Wrapping::None),
        ]
        .spacing(9)
        .align_y(iced::Center);
        button(content)
            .on_press(Message::Select(s.key.clone()))
            .width(Fill)
            .padding([8, 10])
            .style(move |_, status| button::Style {
                background: Some(Background::Color(match (selected, status) {
                    (true, _) => RAISED,
                    (false, button::Status::Hovered) => SURFACE,
                    _ => Color::TRANSPARENT,
                })),
                text_color: TEXT,
                border: border::rounded(8),
                ..button::Style::default()
            })
            .into()
    }
}

fn badge(account: &str) -> Element<'_, Message> {
    container(text(account.to_string()).size(11).color(if account == "main" { WORKING } else { BACKGROUND }))
        .padding([2, 7])
        .style(|_| container::Style { background: Some(Background::Color(RAISED)), border: border::rounded(5), ..container::Style::default() })
        .into()
}

fn panel(background: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(background)),
        border: Border { color: LINE, width: 0.0, radius: 0.0.into() },
        ..container::Style::default()
    }
}

fn phase_color(phase: Phase) -> Color {
    match phase {
        Phase::NeedsYou => CALLS,
        Phase::YourTurn => TURN,
        Phase::Working => WORKING,
        Phase::Background => BACKGROUND,
        Phase::Ended => ENDED,
    }
}

fn one_line(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 90 {
        format!("{}…", line.chars().take(90).collect::<String>())
    } else {
        line
    }
}
