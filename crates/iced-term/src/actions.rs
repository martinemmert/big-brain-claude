#[derive(Debug, Clone, PartialEq, Default)]
pub enum Action {
    Shutdown,
    ChangeTitle(String),
    /// ⌘-click on a URL or a file path in the output (Brain: the app decides how to open it).
    OpenLink(String),
    /// Brain: the program rang the bell (BEL).
    Bell,
    /// Brain: the program asked to put text on the clipboard (OSC 52). Reading the clipboard
    /// back is never allowed.
    CopyToClipboard(String),
    #[default]
    Ignore,
}
