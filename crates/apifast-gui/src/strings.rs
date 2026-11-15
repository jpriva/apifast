//! Visible texts. Components receive them as parameters and never choose a
//! language; the screens pass the active [`Strings`].

/// Every text the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strings {
    /// Welcome screen heading.
    pub welcome_title: &'static str,
    /// Welcome screen explanation.
    pub welcome_hint: &'static str,
    /// Placeholder of the folder field.
    pub folder_placeholder: &'static str,
    /// Button that opens a collection.
    pub open: &'static str,
    /// Environment selector entry for "no environment".
    pub no_environment: &'static str,
    /// Accessible name of the environment selector.
    pub environment: &'static str,
    /// Button that sends the request.
    pub send: &'static str,
    /// Button that cancels the send.
    pub cancel: &'static str,
    /// Shown while waiting for a response.
    pub sending: &'static str,
    /// Shown before anything is sent.
    pub response_idle: &'static str,
    /// Shown after the user cancels a send.
    pub cancelled: &'static str,
    /// Shown when no request is open.
    pub no_request: &'static str,
    /// Response tab with the body.
    pub body: &'static str,
    /// Response tab with the headers.
    pub headers: &'static str,
    /// Shown under a body that is longer than what is displayed.
    pub truncated: &'static str,
    /// Hover text of a request kind this version cannot open.
    pub unsupported_kind: &'static str,
}

/// English.
pub const EN: Strings = Strings {
    welcome_title: "apifast",
    welcome_hint: "Open a collection folder to start.",
    folder_placeholder: "/path/to/collection",
    open: "Open",
    no_environment: "No environment",
    environment: "Environment",
    send: "Send",
    cancel: "Cancel",
    sending: "Sending…",
    response_idle: "Send the request to see its response.",
    cancelled: "Cancelled.",
    no_request: "Select a request in the sidebar.",
    body: "Body",
    headers: "Headers",
    truncated: "Only the start of the body is shown.",
    unsupported_kind: "This kind of request cannot be opened yet",
};
