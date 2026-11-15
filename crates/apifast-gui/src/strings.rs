//! Visible texts. Components receive them as parameters and never choose a
//! language; the screens pass the active [`Strings`].

use apifast_domain::Origin;

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
    /// Sidebar tab with the collection's tree.
    pub sidebar_collection: &'static str,
    /// Sidebar tab with the history of sends.
    pub sidebar_history: &'static str,
    /// Hint of a filter field.
    pub filter: &'static str,
    /// Accessible name of the history filter.
    pub filter_history: &'static str,
    /// Shown when there is no history.
    pub history_empty: &'static str,
    /// A send that failed, in the history.
    pub failed: &'static str,
    /// After a number of seconds.
    pub seconds_ago: &'static str,
    /// After a number of minutes.
    pub minutes_ago: &'static str,
    /// After a number of hours.
    pub hours_ago: &'static str,
    /// After a number of days.
    pub days_ago: &'static str,
    /// Response tab with the events of the send.
    pub timeline: &'static str,
    /// Shown when a response has no timeline.
    pub timeline_empty: &'static str,
    /// Timeline event: a redirect was followed.
    pub event_redirect: &'static str,
    /// Timeline event: the status and headers arrived.
    pub event_headers: &'static str,
    /// Timeline event: the whole body arrived.
    pub event_body: &'static str,
    /// Body view: formatted and colored.
    pub pretty: &'static str,
    /// Body view: as received.
    pub raw: &'static str,
    /// Copy button.
    pub copy: &'static str,
    /// Hover of the button that copies the body.
    pub copy_body: &'static str,
    /// Shown when a body is too large to show pretty.
    pub pretty_off: &'static str,
    /// Shown over a pretty body that could not be formatted.
    pub not_formatted: &'static str,
    /// Shown, with its size, instead of a binary body.
    pub body_binary: &'static str,
    /// Shown with a percentage while a large body is being read.
    pub indexing: &'static str,
    /// Shown before the reason a body cannot be read.
    pub body_unreadable: &'static str,
    /// Hover text of a request kind this version cannot open.
    pub unsupported_kind: &'static str,
    /// Action and dialog title: new request.
    pub new_request: &'static str,
    /// Action and dialog title: new folder.
    pub new_folder: &'static str,
    /// Action and dialog title: rename.
    pub rename: &'static str,
    /// Action: delete.
    pub delete: &'static str,
    /// Action: move one place up.
    pub move_up: &'static str,
    /// Action: move one place down.
    pub move_down: &'static str,
    /// Placeholder of the name field.
    pub name_placeholder: &'static str,
    /// Button that confirms a new item.
    pub create: &'static str,
    /// Title of the delete confirmation.
    pub delete_title: &'static str,
    /// What deleting does, under the item's name.
    pub delete_warning: &'static str,
    /// Placeholder of the URL field.
    pub url_placeholder: &'static str,
    /// Accessible name of the URL field.
    pub url: &'static str,
    /// Switch between a table and its bulk text.
    pub bulk_edit: &'static str,
    /// How the bulk text is written.
    pub bulk_hint: &'static str,
    /// Accessible name of the bulk text editor.
    pub bulk_text: &'static str,
    /// Accessible name of the method selector.
    pub method: &'static str,
    /// Hover text of a tab's close button.
    pub close_tab: &'static str,
    /// Title of the dialog shown before closing a tab with unsaved changes.
    pub unsaved_title: &'static str,
    /// What closing without saving does, under the request's name.
    pub unsaved_warning: &'static str,
    /// Button that saves.
    pub save: &'static str,
    /// Button that closes without saving.
    pub discard: &'static str,
    /// Marks a tab with unsaved changes.
    pub unsaved: &'static str,
    /// Hover of a variable that is not defined.
    pub variable_missing: &'static str,
    /// Hover of a tag whose value is made when sending.
    pub variable_generated: &'static str,
    /// Shown instead of a secret's value.
    pub secret_hidden: &'static str,
    /// Origin: apifast's default.
    pub origin_default: &'static str,
    /// Origin: the global environment.
    pub origin_global: &'static str,
    /// Origin: the collection.
    pub origin_collection: &'static str,
    /// Origin: an environment, followed by its name.
    pub origin_environment: &'static str,
    /// Origin: a folder, followed by its name.
    pub origin_folder: &'static str,
    /// Origin: the request.
    pub origin_request: &'static str,
    /// Origin: captured from a response.
    pub origin_runtime: &'static str,
    /// Origin: typed in for this send.
    pub origin_prompt: &'static str,
    /// Request tab: query and path parameters.
    pub params: &'static str,
    /// Heading of the query parameters.
    pub query_params: &'static str,
    /// Heading of the path parameters.
    pub path_params: &'static str,
    /// How path parameters are added.
    pub path_params_hint: &'static str,
    /// Request tab: authentication.
    pub auth: &'static str,
    /// Request tab: send settings.
    pub settings: &'static str,
    /// Heading of what the request inherits.
    pub inherited: &'static str,
    /// Hover of an inherited header that the request replaces.
    pub overridden: &'static str,
    /// Placeholder of a name cell.
    pub name: &'static str,
    /// Placeholder of a value cell.
    pub value: &'static str,
    /// Hover of a row's remove button.
    pub remove_row: &'static str,
    /// Hover of a row's checkbox.
    pub enabled: &'static str,
    /// Accessible name of the body type selector.
    pub body_type: &'static str,
    /// Body type: none.
    pub body_none: &'static str,
    /// Body type: JSON.
    pub body_json: &'static str,
    /// Body type: text.
    pub body_text: &'static str,
    /// Body type: XML.
    pub body_xml: &'static str,
    /// Body type: SPARQL.
    pub body_sparql: &'static str,
    /// Body type: URL-encoded form.
    pub body_form: &'static str,
    /// Body type: multipart.
    pub body_multipart: &'static str,
    /// Body type: file.
    pub body_file: &'static str,
    /// Shown for body types this version cannot edit.
    pub body_not_editable: &'static str,
    /// Button that formats the body.
    pub format: &'static str,
    /// Hover of the format button, with its shortcut.
    pub format_hint: &'static str,
    /// Shown when the body cannot be formatted, before the line number.
    pub format_failed: &'static str,
    /// Shown over a body too large to highlight.
    pub highlight_off: &'static str,
    /// Accessible name of the auth type selector.
    pub auth_type: &'static str,
    /// Auth type: inherit.
    pub auth_inherit: &'static str,
    /// Auth type: none.
    pub auth_none: &'static str,
    /// Auth type: basic.
    pub auth_basic: &'static str,
    /// Auth type: bearer token.
    pub auth_bearer: &'static str,
    /// Auth type: API key.
    pub auth_api_key: &'static str,
    /// Shown for an auth type this version does not support.
    pub auth_unsupported: &'static str,
    /// Before the origin of an inherited value.
    pub inherited_from: &'static str,
    /// Basic auth field.
    pub username: &'static str,
    /// Basic auth field.
    pub password: &'static str,
    /// Bearer auth field.
    pub token: &'static str,
    /// API key field: the name.
    pub key: &'static str,
    /// API key field: where it goes.
    pub add_to: &'static str,
    /// API key placement: header.
    pub in_header: &'static str,
    /// API key placement: query.
    pub in_query: &'static str,
    /// Checkbox next to each setting.
    pub inherit: &'static str,
    /// Setting: timeout.
    pub timeout: &'static str,
    /// Setting: follow redirects.
    pub follow_redirects: &'static str,
    /// Setting: maximum redirects.
    pub max_redirects: &'static str,
    /// Setting: encode the URL.
    pub encode_url: &'static str,
    /// A setting that is on.
    pub yes: &'static str,
    /// A setting that is off.
    pub no: &'static str,
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
    sidebar_collection: "Collection",
    sidebar_history: "History",
    filter: "Filter",
    filter_history: "Filter the history",
    history_empty: "Nothing sent yet.",
    failed: "Failed",
    seconds_ago: "s ago",
    minutes_ago: "min ago",
    hours_ago: "h ago",
    days_ago: "d ago",
    timeline: "Timeline",
    timeline_empty: "This response has no timeline.",
    event_redirect: "Redirect",
    event_headers: "Headers received",
    event_body: "Body received",
    pretty: "Pretty",
    raw: "Raw",
    copy: "Copy",
    copy_body: "Copy the whole body",
    pretty_off: "Shown raw: bodies over 2 MB are not formatted.",
    not_formatted: "Could not be formatted; shown as received.",
    body_binary: "Binary body, not shown:",
    indexing: "Reading the body:",
    body_unreadable: "The body cannot be read:",
    unsupported_kind: "This kind of request cannot be opened yet",
    new_request: "New request",
    new_folder: "New folder",
    rename: "Rename",
    delete: "Delete",
    move_up: "Move up",
    move_down: "Move down",
    name_placeholder: "Name",
    create: "Create",
    delete_title: "Delete this?",
    delete_warning: "Its files are deleted from disk, with everything inside.",
    url_placeholder: "https://example.com or {{host}}/path",
    url: "URL",
    bulk_edit: "Bulk edit",
    bulk_text: "Rows as text",
    bulk_hint: "One per line, as `name: value`. Start a line with // to keep it but not send it.",
    method: "Method",
    close_tab: "Close tab",
    unsaved_title: "Save changes?",
    unsaved_warning: "This request has unsaved changes. Closing without saving discards them.",
    save: "Save",
    discard: "Don't save",
    unsaved: "Unsaved changes",
    variable_missing: "Not defined here. Pick an environment that defines it, or add it.",
    variable_generated: "Made when the request is sent.",
    secret_hidden: "secret (hidden)",
    origin_default: "Default",
    origin_global: "Global environment",
    origin_collection: "Collection",
    origin_environment: "Environment",
    origin_folder: "Folder",
    origin_request: "Request",
    origin_runtime: "Captured from a response",
    origin_prompt: "Typed in for this send",
    params: "Params",
    query_params: "Query",
    path_params: "Path",
    path_params_hint: "Add `:name` to the URL's path to get one here.",
    auth: "Auth",
    settings: "Settings",
    inherited: "Inherited",
    overridden: "Replaced by this request's own header",
    name: "name",
    value: "value",
    remove_row: "Remove",
    enabled: "Send this row",
    body_type: "Body type",
    body_none: "No body",
    body_json: "JSON",
    body_text: "Text",
    body_xml: "XML",
    body_sparql: "SPARQL",
    body_form: "Form (URL-encoded)",
    body_multipart: "Multipart form",
    body_file: "File",
    body_not_editable: "This body type cannot be edited here yet; it is sent and saved as it is.",
    format: "Format",
    format_hint: "Format (Alt+Shift+F)",
    format_failed: "Could not format: brackets or tags do not match, or something is not closed. Check line",
    highlight_off: "Highlighting is off: this body is over 200 KB.",
    auth_type: "Auth type",
    auth_inherit: "Inherit",
    auth_none: "No auth",
    auth_basic: "Basic",
    auth_bearer: "Bearer token",
    auth_api_key: "API key",
    auth_unsupported: "This auth type is not supported yet; it is kept as it is.",
    inherited_from: "Inherited from",
    username: "Username",
    password: "Password",
    token: "Token",
    key: "Key",
    add_to: "Add to",
    in_header: "Header",
    in_query: "Query",
    inherit: "Inherit",
    timeout: "Timeout",
    follow_redirects: "Follow redirects",
    max_redirects: "Maximum redirects",
    encode_url: "Encode URL",
    yes: "Yes",
    no: "No",
};

impl Strings {
    /// Where a value comes from, ready to show. `folders` names the
    /// request's folders, outermost first, for [`Origin::Folder`].
    pub fn origin(&self, origin: &Origin, folders: &[String]) -> String {
        match origin {
            Origin::Default => self.origin_default.to_owned(),
            Origin::Global => self.origin_global.to_owned(),
            Origin::Collection => self.origin_collection.to_owned(),
            Origin::Environment(env) => format!("{} {env}", self.origin_environment),
            Origin::Folder(index) => {
                let folder = folders.get(*index).map_or("", String::as_str);
                format!("{} {folder}", self.origin_folder)
            }
            Origin::Request => self.origin_request.to_owned(),
            Origin::Runtime => self.origin_runtime.to_owned(),
            Origin::Prompt => self.origin_prompt.to_owned(),
            // A scope added after this version: say nothing wrong.
            _ => String::new(),
        }
    }
}
