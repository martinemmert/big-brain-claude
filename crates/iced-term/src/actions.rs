#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    Shutdown,
    ChangeTitle(String),
    /// ⌘-click on a URL or a file path in the output (Brain: the app decides how to open it).
    OpenLink(String),
    #[default]
    Ignore,
}
