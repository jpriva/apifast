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
    /// Title of the environment editor, and its tab of environments.
    pub environments: &'static str,
    /// Button that opens the environment editor, with its shortcut.
    pub edit_environments: &'static str,
    /// Tab with every variable the active request sees.
    pub all_variables: &'static str,
    /// Explains the all-variables view.
    pub all_variables_hint: &'static str,
    /// Shown when the collection has no environments.
    pub no_environments: &'static str,
    /// Shown when a request sees no variables.
    pub no_variables: &'static str,
    /// Field for the name of a new environment.
    pub new_environment: &'static str,
    /// Button that adds what was typed.
    pub add: &'static str,
    /// Accessible name of the environment's name field.
    pub environment_name: &'static str,
    /// Hover of the environment's name field.
    pub rename_hint: &'static str,
    /// Label of the environment's color.
    pub color: &'static str,
    /// Label of the environment it inherits from.
    pub extends: &'static str,
    /// No environment to inherit from.
    pub extends_none: &'static str,
    /// Button that drops changes not saved.
    pub revert: &'static str,
    /// Asks to confirm, before the environment's name.
    pub delete_environment: &'static str,
    /// Column of a variable's type.
    pub kind: &'static str,
    /// Type: text.
    pub kind_text: &'static str,
    /// Type: number.
    pub kind_number: &'static str,
    /// Type: boolean.
    pub kind_bool: &'static str,
    /// Type: JSON object.
    pub kind_object: &'static str,
    /// Column telling whether a variable is secret.
    pub secret: &'static str,
    /// Field that adds a variable.
    pub new_variable: &'static str,
    /// Column of where a variable comes from.
    pub from: &'static str,
    /// Column of the scopes a variable overrides.
    pub overrides: &'static str,
    /// Shown over a request changed on disk while it had a draft.
    pub changed_on_disk: &'static str,
    /// Keeps the draft over the file changed on disk.
    pub keep_mine: &'static str,
    /// Drops the draft for the file changed on disk.
    pub load_from_disk: &'static str,
    /// Action: send.
    pub action_send: &'static str,
    /// Action: cancel the send.
    pub action_cancel: &'static str,
    /// Action: save.
    pub action_save: &'static str,
    /// Action: save every tab.
    pub action_save_all: &'static str,
    /// Action: command palette.
    pub action_palette: &'static str,
    /// Action: open a request by name.
    pub action_quick_open: &'static str,
    /// Action: new request.
    pub action_new_request: &'static str,
    /// Action: close the tab.
    pub action_close_tab: &'static str,
    /// Action: reopen the tab closed last.
    pub action_reopen_tab: &'static str,
    /// Action: next tab.
    pub action_next_tab: &'static str,
    /// Action: previous tab.
    pub action_previous_tab: &'static str,
    /// Action: go to tab, before its number.
    pub action_go_to_tab: &'static str,
    /// Action: put the cursor in the URL.
    pub action_focus_url: &'static str,
    /// Action: environment editor.
    pub action_environments: &'static str,
    /// Action: show or hide the sidebar.
    pub action_toggle_sidebar: &'static str,
    /// Action: show the history.
    pub action_show_history: &'static str,
    /// Action: format the body.
    pub action_format_body: &'static str,
    /// Action: edit keyboard shortcuts.
    pub action_shortcuts: &'static str,
    /// Placeholder of the command palette.
    pub palette_hint: &'static str,
    /// Placeholder of the palette on requests only.
    pub palette_requests_hint: &'static str,
    /// Shown when nothing matches in the palette.
    pub palette_nothing: &'static str,
    /// Before an environment's name in the palette.
    pub palette_environment: &'static str,
    /// Title of the shortcut editor.
    pub shortcuts_title: &'static str,
    /// Button that waits for a new shortcut.
    pub shortcut_change: &'static str,
    /// Shown while waiting for the new shortcut.
    pub shortcut_press: &'static str,
    /// Button that puts back the default.
    pub shortcut_reset: &'static str,
    /// Shown for an action without a shortcut.
    pub shortcut_none: &'static str,
    /// Tells a shortcut was taken from an action, before its name.
    pub shortcut_moved: &'static str,
    /// Before the entries of keymap.yml that were not understood.
    pub keymap_problems: &'static str,
    /// Name of a request created from the keyboard.
    pub new_request_name: &'static str,
    /// Column header: names.
    pub column_name: &'static str,
    /// Column header: values.
    pub column_value: &'static str,
    /// Action: follow the system's theme.
    pub action_theme_system: &'static str,
    /// Action: light theme.
    pub action_theme_light: &'static str,
    /// Action: dark theme.
    pub action_theme_dark: &'static str,
    /// Action: open the settings.
    pub action_settings: &'static str,
    /// Title of the settings window.
    pub settings_title: &'static str,
    /// Settings section: appearance.
    pub settings_appearance: &'static str,
    /// Setting: theme.
    pub settings_theme: &'static str,
    /// Theme option: follow the system.
    pub theme_system: &'static str,
    /// Theme option: light.
    pub theme_light: &'static str,
    /// Theme option: dark.
    pub theme_dark: &'static str,
    /// Setting: interface scale.
    pub settings_scale: &'static str,
    /// Hover of the interface scale.
    pub settings_scale_hint: &'static str,
    /// Settings section: responses.
    pub settings_responses: &'static str,
    /// Setting: largest body shown pretty.
    pub settings_pretty_limit: &'static str,
    /// Settings section: history and storage.
    pub settings_storage: &'static str,
    /// Setting: history entries kept.
    pub settings_history_entries: &'static str,
    /// Setting: days kept.
    pub settings_keep_days: &'static str,
    /// Setting: megabytes of responses kept.
    pub settings_responses_mb: &'static str,
    /// Explains when storage limits apply.
    pub settings_storage_hint: &'static str,
    /// Button that puts back every default.
    pub settings_reset: &'static str,
    /// Before the entries of settings.yml that were not understood.
    pub settings_problems: &'static str,
    /// Unit after a number of days.
    pub days: &'static str,
    /// Button that opens the system's folder picker.
    pub browse: &'static str,
    /// Title of the folder picker.
    pub pick_collection: &'static str,
    /// Hover of the secret column.
    pub secret_hint: &'static str,
    /// Accessible name of a secret's value field.
    pub secret_value: &'static str,
    /// A secret's value is on this machine.
    pub secret_local: &'static str,
    /// A secret's value is shared.
    pub secret_shared: &'static str,
    /// A shared value this machine cannot open.
    pub secret_locked: &'static str,
    /// A secret without value.
    pub secret_missing: &'static str,
    /// Checkbox that shares a secret's value.
    pub shared: &'static str,
    /// Hover of the shared checkbox.
    pub shared_hint: &'static str,
    /// Button that shows a secret's value for a while.
    pub reveal: &'static str,
    /// Button that hides a revealed value.
    pub hide: &'static str,
    /// Heading of stored values no secret uses.
    pub orphans_title: &'static str,
    /// Explains orphan values.
    pub orphans_hint: &'static str,
    /// Button that removes an orphan value.
    pub forget: &'static str,
    /// Title of the collection key window, and its field.
    pub collection_key: &'static str,
    /// Action: open the collection key window.
    pub action_collection_key: &'static str,
    /// Button that takes a pasted key.
    pub use_key: &'static str,
    /// Shown when the collection shares no secret.
    pub no_collection_key: &'static str,
    /// Asks for the collection's key.
    pub key_needed: &'static str,
    /// How to pass the key.
    pub share_key_hint: &'static str,
    /// Button that shows the collection's key.
    pub show_key: &'static str,
    /// Button that rotates the collection's key.
    pub rotate_key: &'static str,
    /// What rotating does.
    pub rotate_hint: &'static str,
    /// Confirmation of a rotation.
    pub rotate_confirm: &'static str,
    /// Banner when the key of a collection is missing.
    pub vault_banner: &'static str,
    /// Top-bar note when the master key is in a file.
    pub master_in_file: &'static str,
    /// Explains the master key file.
    pub master_in_file_hint: &'static str,
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
    environments: "Environments",
    edit_environments: "Edit environments (Ctrl+E)",
    all_variables: "All variables",
    all_variables_hint: "What the active request sees, after every scope is applied.",
    no_environments: "No environments yet.",
    no_variables: "This request sees no variables.",
    new_environment: "New environment",
    add: "Add",
    environment_name: "Environment name",
    rename_hint: "Press Enter to rename",
    color: "Color",
    extends: "Extends",
    extends_none: "Nothing",
    revert: "Revert",
    delete_environment: "Delete environment",
    kind: "Type",
    kind_text: "Text",
    kind_number: "Number",
    kind_bool: "Boolean",
    kind_object: "Object",
    secret: "Secret",
    new_variable: "New variable",
    from: "From",
    overrides: "Overrides",
    changed_on_disk: "This request changed on disk while you were editing it.",
    keep_mine: "Keep my changes",
    load_from_disk: "Load from disk",
    action_send: "Send request",
    action_cancel: "Cancel send",
    action_save: "Save",
    action_save_all: "Save all",
    action_palette: "Command palette",
    action_quick_open: "Go to request",
    action_new_request: "New request",
    action_close_tab: "Close tab",
    action_reopen_tab: "Reopen closed tab",
    action_next_tab: "Next tab",
    action_previous_tab: "Previous tab",
    action_go_to_tab: "Go to tab",
    action_focus_url: "Focus URL",
    action_environments: "Edit environments",
    action_toggle_sidebar: "Toggle sidebar",
    action_show_history: "Show history",
    action_format_body: "Format body",
    action_shortcuts: "Keyboard shortcuts",
    palette_hint: "Type a command, request or environment",
    palette_requests_hint: "Type a request name or path",
    palette_nothing: "Nothing matches.",
    palette_environment: "Environment:",
    shortcuts_title: "Keyboard shortcuts",
    shortcut_change: "Change",
    shortcut_press: "Press the new shortcut, or Escape",
    shortcut_reset: "Reset",
    shortcut_none: "none",
    shortcut_moved: "Taken from",
    keymap_problems: "Some entries of keymap.yml were ignored:",
    new_request_name: "New request",
    column_name: "Name",
    column_value: "Value",
    action_theme_system: "Theme: follow the system",
    action_theme_light: "Theme: light",
    action_theme_dark: "Theme: dark",
    action_settings: "Settings",
    settings_title: "Settings",
    settings_appearance: "Appearance",
    settings_theme: "Theme",
    theme_system: "Follow the system",
    theme_light: "Light",
    theme_dark: "Dark",
    settings_scale: "Interface scale",
    settings_scale_hint: "Also Ctrl+= and Ctrl+-; Ctrl+0 resets it",
    settings_responses: "Responses",
    settings_pretty_limit: "Format and color bodies up to",
    settings_storage: "History and storage",
    settings_history_entries: "Sends kept in the history",
    settings_keep_days: "Keep history and responses for",
    settings_responses_mb: "Space for response bodies",
    settings_storage_hint: "Limits apply the next time a collection opens.",
    settings_reset: "Restore defaults",
    settings_problems: "Some entries of settings.yml were ignored:",
    days: "days",
    browse: "Browse...",
    pick_collection: "Open a collection folder",
    secret_hint: "Secret: the file keeps only its name; the value goes to the vault",
    secret_value: "Secret value",
    secret_local: "Stored on this machine",
    secret_shared: "Shared with the team",
    secret_locked: "Locked: this machine lacks the collection key",
    secret_missing: "No value yet: type one",
    shared: "Shared",
    shared_hint: "Share with the team: sealed with the collection key in .apifast/secrets, which goes to git",
    reveal: "Reveal",
    hide: "Hide",
    orphans_title: "Values no secret uses",
    orphans_hint: "Their variable is gone or no longer secret, as after editing with Bruno.",
    forget: "Forget",
    collection_key: "Collection key",
    action_collection_key: "Collection key",
    use_key: "Use key",
    no_collection_key: "This collection shares no secrets yet. Sharing the first one makes its key.",
    key_needed: "This collection shares secrets, and this machine does not have its key. Paste the key your team gave you:",
    share_key_hint: "Give this key to your team through a password manager or another private channel. apifast never sends it.",
    show_key: "Show key",
    rotate_key: "Rotate key",
    rotate_hint: "Rotating makes a new key and seals every shared secret again. Your team needs the new key.",
    rotate_confirm: "Make a new key and seal every shared secret again? Commit the changed files afterwards, and give the new key to your team. Old values stay in git's history: if the old key leaked, change the real credentials too.",
    vault_banner: "This collection shares secrets that this machine cannot open: paste the collection's key.",
    master_in_file: "Master key in a file",
    master_in_file_hint: "No system keyring was found, so the vault's master key is in a file only you can read. Anyone who copies your home folder gets it too.",
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
