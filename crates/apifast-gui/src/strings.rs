//! Visible texts. Components receive them as parameters and never choose a
//! language; the screens pass the active [`Strings`].

use apifast_domain::Origin;

/// Every text the interface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strings {
    /// Shown in the sidebar while no collection is open.
    pub no_collection: &'static str,
    /// Button and action that create a collection.
    pub new_collection: &'static str,
    /// Button and action that open a collection.
    pub open_collection: &'static str,
    /// Heading of the "New collection" dialog.
    pub new_collection_title: &'static str,
    /// Label of the collection's name.
    pub collection_name: &'static str,
    /// Placeholder of the collection's name.
    pub new_collection_hint: &'static str,
    /// Label of the folder a new collection goes in.
    pub collection_parent: &'static str,
    /// Before the folder a new collection will have.
    pub collection_goes_in: &'static str,
    /// Title of the picker for that folder.
    pub pick_parent: &'static str,
    /// Offers to create a collection in a folder that has none.
    pub create_here: &'static str,
    /// Root menu: unload the collection.
    pub close_collection: &'static str,
    /// Root menu: remove the collection from the workspace.
    pub remove_collection: &'static str,
    /// Root menu: copy the collection's folder.
    pub copy_folder_path: &'static str,
    /// Accessible name of the workspace selector.
    pub workspace: &'static str,
    /// Creates a workspace.
    pub new_workspace: &'static str,
    /// Opens a workspace folder.
    pub open_workspace: &'static str,
    /// Renames the active workspace.
    pub rename_workspace: &'static str,
    /// Title of the picker of a workspace folder.
    pub pick_workspace: &'static str,
    /// Before the name of the collection whose history is shown.
    pub history_of: &'static str,
    /// Button that opens something.
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
    /// Button and action: import a collection.
    pub import_collection_action: &'static str,
    /// Title of the import window.
    pub import_title: &'static str,
    /// Field: the collection's export.
    pub import_collection_file: &'static str,
    /// Field: environments' exports.
    pub import_environments: &'static str,
    /// Button: add environment exports.
    pub import_add_environments: &'static str,
    /// Button: make the plan.
    pub import_review: &'static str,
    /// Before reviewing.
    pub import_hint: &'static str,
    /// After a count: folders.
    pub import_folders: &'static str,
    /// After a count: requests.
    pub import_requests: &'static str,
    /// After a count: environments.
    pub import_environments_count: &'static str,
    /// Heading of the warnings.
    pub import_not_kept: &'static str,
    /// Where a warning about the collection is.
    pub import_collection: &'static str,
    /// Field: the new collection's folder.
    pub import_folder: &'static str,
    /// When the files changed after reviewing.
    pub import_review_again: &'static str,
    /// Button: write the reviewed plan.
    pub import: &'static str,
    /// Menu and action: copy a request as a cURL command.
    pub copy_as_curl: &'static str,
    /// After copying a request as cURL.
    pub copied_as_curl: &'static str,
    /// After copying, when something rendered empty; what follows.
    pub copied_missing: &'static str,
    /// When a request cannot be copied; why follows.
    pub copy_failed: &'static str,
    /// Note after pasting a cURL: options follow.
    pub curl_ignored: &'static str,
    /// Note when a pasted cURL cannot be read.
    pub curl_failed: &'static str,
    /// Request tab: saved examples.
    pub examples: &'static str,
    /// Examples tab without examples.
    pub examples_empty: &'static str,
    /// Field: an example's name.
    pub example_name: &'static str,
    /// Field: an example's description.
    pub example_description: &'static str,
    /// Button: delete an example.
    pub example_delete: &'static str,
    /// Accessible name of an example's body.
    pub example_body: &'static str,
    /// An example whose response had no body.
    pub example_no_body: &'static str,
    /// Response button: keep it as an example of the request.
    pub save_as_example: &'static str,
    /// Hover text of that button.
    pub save_as_example_hint: &'static str,
    /// After saving an example; its name follows.
    pub example_saved: &'static str,
    /// Heading of a collection's DNS overrides.
    pub dns_overrides: &'static str,
    /// Field: the host a DNS override is for.
    pub dns_host: &'static str,
    /// Field: the address a DNS override gives.
    pub dns_address: &'static str,
    /// Next to an address that is not an IP.
    pub dns_not_ip: &'static str,
    /// Button: add a DNS override.
    pub dns_add: &'static str,
    /// Under the DNS overrides.
    pub dns_hint: &'static str,
    /// Button: add a part to a multipart body.
    pub add_part: &'static str,
    /// Selector: whether a part is text or files.
    pub part_kind: &'static str,
    /// A part with text.
    pub part_text: &'static str,
    /// A part with files.
    pub part_files: &'static str,
    /// Field: the Content-Type of a part or file.
    pub part_content_type: &'static str,
    /// Placeholder of a content type left to apifast.
    pub part_content_type_hint: &'static str,
    /// Button: pick files for a part.
    pub choose_files: &'static str,
    /// Button: take a file out of a part.
    pub remove_file: &'static str,
    /// A file part with no files yet.
    pub no_files: &'static str,
    /// Field: the file a file body sends.
    pub body_file_path: &'static str,
    /// Under the file of a file body.
    pub body_file_hint: &'static str,
    /// Tab of the response, and heading of the cookie settings.
    pub cookies: &'static str,
    /// Title of the cookie editor.
    pub cookies_title: &'static str,
    /// Root menu and palette: open the cookie editor.
    pub cookies_menu: &'static str,
    /// Action: open the cookie editor.
    pub action_cookies: &'static str,
    /// Cookie editor without a collection.
    pub cookies_no_collection: &'static str,
    /// Cookie editor with an empty jar.
    pub cookies_empty: &'static str,
    /// Button: add a cookie.
    pub cookies_add: &'static str,
    /// Button: delete every cookie.
    pub cookies_clear: &'static str,
    /// Asks before deleting every cookie.
    pub cookies_clear_confirm: &'static str,
    /// Button: delete the cookies of a domain.
    pub cookies_delete_domain: &'static str,
    /// Button: change a cookie.
    pub cookies_edit: &'static str,
    /// Button: delete a cookie.
    pub cookies_delete: &'static str,
    /// Heading of the form of a new cookie.
    pub cookies_add_title: &'static str,
    /// Heading of the form of a cookie being changed.
    pub cookies_edit_title: &'static str,
    /// Column and field: the cookie's name.
    pub cookies_name: &'static str,
    /// Column and field: the cookie's value.
    pub cookies_value: &'static str,
    /// Column and field: the cookie's domain.
    pub cookies_domain: &'static str,
    /// Column and field: the cookie's path.
    pub cookies_path: &'static str,
    /// Column and field: when the cookie expires.
    pub cookies_expires: &'static str,
    /// Placeholder of the expiry: empty for a session cookie.
    pub cookies_expires_hint: &'static str,
    /// Column: `Secure`, `HttpOnly`, `SameSite`.
    pub cookies_flags: &'static str,
    /// A cookie with no expiry.
    pub cookies_session: &'static str,
    /// A cookie that has expired.
    pub cookies_expired: &'static str,
    /// Flag: the cookie also goes to subdomains.
    pub cookies_subdomains: &'static str,
    /// Field: `SameSite`.
    pub cookies_same_site: &'static str,
    /// Checkbox: only this host.
    pub cookies_host_only: &'static str,
    /// Checkbox: `Secure`.
    pub cookies_secure: &'static str,
    /// Checkbox: `HttpOnly`.
    pub cookies_http_only: &'static str,
    /// A cookie name that cannot be.
    pub cookies_bad_name: &'static str,
    /// A cookie with no domain.
    pub cookies_bad_domain: &'static str,
    /// A path that does not start with /.
    pub cookies_bad_path: &'static str,
    /// An expiry that is not a date.
    pub cookies_bad_expires: &'static str,
    /// Cookies tab of a response that set none.
    pub response_no_cookies: &'static str,
    /// Settings: send the cookies of the jar.
    pub send_cookies: &'static str,
    /// Settings: keep the cookies responses set.
    pub store_cookies: &'static str,
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
    /// Request and response tab with the assertions and captures.
    pub tests: &'static str,
    /// Heading of the assertions.
    pub assertions: &'static str,
    /// Banner over the editor; `{environment}` and `{names}` are replaced.
    pub captured_for_environment: &'static str,
    /// Writes the captured values to the environment.
    pub save_to_environment: &'static str,
    /// Forgets the captured values.
    pub discard_captures: &'static str,
    /// Title of the runner window.
    pub runner: &'static str,
    /// Tree menu: open the runner on a collection or folder.
    pub run_menu: &'static str,
    /// Runner: include subfolders.
    pub run_recursive: &'static str,
    /// Runner: only requests with one of these tags.
    pub run_tags: &'static str,
    /// Runner: no requests with these tags.
    pub run_exclude_tags: &'static str,
    /// Runner: pause between requests.
    pub run_delay: &'static str,
    /// Runner: repetitions.
    pub run_iterations: &'static str,
    /// Runner: the data file, and the title of its picker.
    pub run_data: &'static str,
    /// Runner: choose a data file.
    pub run_data_pick: &'static str,
    /// Runner: stop using the data file.
    pub run_data_clear: &'static str,
    /// Runner: stop at the first failure.
    pub run_bail: &'static str,
    /// Runner: start.
    pub run_start: &'static str,
    /// Runner: stop the run.
    pub run_stop: &'static str,
    /// Runner: forget the results.
    pub run_clear: &'static str,
    /// Runner: before the iteration number.
    pub run_iteration: &'static str,
    /// Runner totals; the braces are replaced.
    pub run_summary: &'static str,
    /// Runner: before the report formats.
    pub run_export: &'static str,
    /// Runner: mark of a skipped request.
    pub run_skip: &'static str,
    /// Runner: mark of a request that could not be sent.
    pub run_error: &'static str,
    /// Runner: hover of a step.
    pub run_open_step: &'static str,
    /// Accessible name of the response filter.
    pub filter_body: &'static str,
    /// Placeholder of the filter on a JSON body.
    pub filter_jsonpath: &'static str,
    /// Placeholder of the filter on an XML body.
    pub filter_xpath: &'static str,
    /// Above the filtered values; `{count}` is replaced.
    pub filter_results: &'static str,
    /// Auth type: HTTP Digest.
    pub auth_digest: &'static str,
    /// Auth type: NTLM.
    pub auth_ntlm: &'static str,
    /// NTLM auth field: the Windows domain.
    pub ntlm_domain: &'static str,
    /// Auth type: AWS Signature Version 4.
    pub auth_aws_v4: &'static str,
    /// AWS auth field.
    pub aws_access_key_id: &'static str,
    /// AWS auth field.
    pub aws_secret_access_key: &'static str,
    /// AWS auth field, optional.
    pub aws_session_token: &'static str,
    /// AWS auth field: the service in the signature.
    pub aws_service: &'static str,
    /// AWS auth field.
    pub aws_region: &'static str,
    /// Auth type: OAuth 2.0.
    pub auth_oauth2: &'static str,
    /// OAuth 2.0: the grant type.
    pub oauth2_flow: &'static str,
    /// OAuth 2.0 grant.
    pub oauth2_client_credentials: &'static str,
    /// OAuth 2.0 grant.
    pub oauth2_password: &'static str,
    /// OAuth 2.0 grant.
    pub oauth2_authorization_code: &'static str,
    /// OAuth 2.0 grant, read from files but not sent.
    pub oauth2_implicit: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_authorization_url: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_callback_url: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_access_token_url: &'static str,
    /// OAuth 2.0 field, optional.
    pub oauth2_refresh_token_url: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_client_id: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_client_secret: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_scope: &'static str,
    /// OAuth 2.0 field.
    pub oauth2_state: &'static str,
    /// OAuth 2.0: how the client credentials travel.
    pub oauth2_send_credentials: &'static str,
    /// OAuth 2.0: credentials in the body.
    pub oauth2_in_body: &'static str,
    /// OAuth 2.0: credentials as Basic auth.
    pub oauth2_in_basic: &'static str,
    /// OAuth 2.0: use PKCE.
    pub oauth2_pkce: &'static str,
    /// OAuth 2.0: where the token goes.
    pub oauth2_token_in: &'static str,
    /// OAuth 2.0: the prefix before the token in the header.
    pub oauth2_header_prefix: &'static str,
    /// OAuth 2.0: the query parameter of the token.
    pub oauth2_query_key: &'static str,
    /// OAuth 2.0: the token's name in the cache.
    pub oauth2_token_id: &'static str,
    /// OAuth 2.0: which token of the response is sent.
    pub oauth2_token_source: &'static str,
    /// OAuth 2.0: automatic behaviour.
    pub oauth2_automatic: &'static str,
    /// OAuth 2.0 option.
    pub oauth2_auto_fetch: &'static str,
    /// OAuth 2.0 option.
    pub oauth2_auto_refresh: &'static str,
    /// OAuth 2.0: extra parameters; `{count}` is replaced.
    pub oauth2_parameters_kept: &'static str,
    /// OAuth 2.0: before the token's state.
    pub token_label: &'static str,
    /// OAuth 2.0: no token stored.
    pub token_none: &'static str,
    /// OAuth 2.0: the token expired.
    pub token_expired: &'static str,
    /// OAuth 2.0: `{time}` is replaced.
    pub token_expires_in: &'static str,
    /// OAuth 2.0: a token without expiry.
    pub token_no_expiry: &'static str,
    /// OAuth 2.0: get a new token now.
    pub token_get: &'static str,
    /// OAuth 2.0: forget the stored token.
    pub token_forget: &'static str,
    /// Settings section and collection setting: the proxy.
    pub proxy: &'static str,
    /// Proxy mode.
    pub proxy_system: &'static str,
    /// Proxy mode.
    pub proxy_off: &'static str,
    /// Proxy mode.
    pub proxy_manual: &'static str,
    /// Collection proxy mode: the application's.
    pub proxy_inherit: &'static str,
    /// Collection proxy mode: its own.
    pub proxy_custom: &'static str,
    /// Proxy field.
    pub proxy_protocol: &'static str,
    /// Proxy field.
    pub proxy_host: &'static str,
    /// Proxy field.
    pub proxy_port: &'static str,
    /// Proxy field: send credentials.
    pub proxy_auth: &'static str,
    /// Proxy field: hosts that go direct.
    pub proxy_bypass: &'static str,
    /// Placeholder when a password is kept.
    pub proxy_password_saved: &'static str,
    /// Keeps the proxy password, sealed.
    pub proxy_save_password: &'static str,
    /// Forgets the proxy password.
    pub proxy_forget_password: &'static str,
    /// Under the application proxy.
    pub proxy_hint: &'static str,
    /// Title of a collection's settings.
    pub collection_settings: &'static str,
    /// Root menu: a collection's settings.
    pub collection_settings_menu: &'static str,
    /// Under a collection's own proxy.
    pub collection_proxy_hint: &'static str,
    /// Settings section: trust in servers.
    pub tls: &'static str,
    /// Settings: extra certificate authorities.
    pub tls_ca_file: &'static str,
    /// Settings: do not check server certificates.
    pub tls_insecure: &'static str,
    /// Under the setting, when on.
    pub tls_insecure_warning: &'static str,
    /// Opens a file picker.
    pub choose: &'static str,
    /// Collection settings: client certificates.
    pub client_certificates: &'static str,
    /// Client certificate field.
    pub client_certificate_domain: &'static str,
    /// Client certificate field.
    pub client_certificate_file: &'static str,
    /// Client certificate field.
    pub client_certificate_key: &'static str,
    /// Adds a client certificate.
    pub client_certificate_add: &'static str,
    /// Under the client certificates.
    pub client_certificates_hint: &'static str,
    /// Mark of an assertion that held.
    pub pass: &'static str,
    /// Mark of an assertion that did not.
    pub fail: &'static str,
    /// Heading of the captures.
    pub captures: &'static str,
    /// Placeholder and name of the field with what is checked or captured.
    pub expression: &'static str,
    /// Accessible name of an assertion's operator.
    pub operator: &'static str,
    /// Placeholder of an assertion's expected value.
    pub expected: &'static str,
    /// Shown instead of the expected value of an operator that takes none.
    pub no_expected: &'static str,
    /// Placeholder and name of the variable a capture sets.
    pub capture_variable: &'static str,
    /// Accessible name of a capture's scope.
    pub capture_scope: &'static str,
    /// Under the headings of the Tests tab.
    pub tests_hint: &'static str,
    /// Response tab when nothing ran after the response: the request has
    /// no assertions or captures, or the response comes from the history.
    pub tests_none: &'static str,
    /// Between a capture's variable and its value.
    pub captured_as: &'static str,
    /// Timeline event: a redirect was followed.
    pub event_redirect: &'static str,
    /// Timeline: the addresses came from the collection's DNS overrides.
    pub event_dns_override: &'static str,
    /// Timeline: the body came compressed, with its size on the wire.
    pub event_decompressed: &'static str,
    /// Timeline: a name was resolved.
    pub event_dns: &'static str,
    /// Timeline: a new connection was opened.
    pub event_connected: &'static str,
    /// Timeline: the request went on an open connection.
    pub event_reused: &'static str,
    /// Timeline: the TLS handshake ended.
    pub event_tls: &'static str,
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
    no_collection: "Create a collection or open one to start.",
    new_collection: "New collection…",
    open_collection: "Open collection…",
    new_collection_title: "New collection",
    collection_name: "Name",
    new_collection_hint: "Shop API",
    collection_parent: "Folder",
    collection_goes_in: "It goes in",
    pick_parent: "Choose where to create the collection",
    create_here: "Create one here",
    close_collection: "Close",
    remove_collection: "Remove from workspace",
    copy_folder_path: "Copy folder path",
    workspace: "Workspace",
    new_workspace: "New workspace…",
    open_workspace: "Open workspace…",
    rename_workspace: "Rename workspace…",
    pick_workspace: "Open a workspace folder",
    history_of: "History of",
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
    import_collection_action: "Import…",
    import_title: "Import a collection",
    import_collection_file: "Collection or spec",
    import_environments: "Postman environments",
    import_add_environments: "Add environments…",
    import_review: "Review",
    import_hint: "Choose a Postman export (v2.0 or v2.1) or an OpenAPI 3 spec (JSON or YAML), and review what it creates before importing.",
    import_folders: "folders",
    import_requests: "requests",
    import_environments_count: "environments",
    import_not_kept: "Not kept",
    import_collection: "Collection",
    import_folder: "Folder name",
    import_review_again: "The files changed: review them again before importing.",
    import: "Import",
    copy_as_curl: "Copy as cURL",
    copied_as_curl: "Copied as cURL.",
    copied_missing: "Copied as cURL, with something missing:",
    copy_failed: "Cannot copy as cURL:",
    curl_ignored: "Pasted the cURL command. Not kept:",
    curl_failed: "The pasted cURL command cannot be read",
    examples: "Examples",
    examples_empty: "No examples yet. Send the request and press “Save as example” on its response.",
    example_name: "Example name",
    example_description: "Description",
    example_delete: "Delete example",
    example_body: "Example body",
    example_no_body: "No body.",
    save_as_example: "Save as example",
    save_as_example_hint: "Keep this response in the request's file, with the request as written. Saved with the request (Ctrl+S).",
    example_saved: "Added the example, saved with the request:",
    dns_overrides: "DNS overrides",
    dns_host: "Host",
    dns_address: "IP address",
    dns_not_ip: "not an IP address",
    dns_add: "Add override",
    dns_hint: "Like /etc/hosts, for this collection only: * matches any run of characters (*.example.com). Saved in .apifast/collection.yml, which goes to git.",
    add_part: "Add part",
    part_kind: "Part type",
    part_text: "Text",
    part_files: "Files",
    part_content_type: "Content type",
    part_content_type_hint: "auto",
    choose_files: "Choose files…",
    remove_file: "Remove file",
    no_files: "No files yet.",
    body_file_path: "File",
    body_file_hint: "Read from disk as it is sent. A path inside the collection is kept relative to it; without a content type, application/octet-stream goes.",
    cookies: "Cookies",
    cookies_title: "Cookies",
    cookies_menu: "Cookies…",
    action_cookies: "Cookies",
    cookies_no_collection: "Open a collection to see its cookies.",
    cookies_empty: "No cookies yet. They appear here when a response sets them.",
    cookies_add: "Add cookie",
    cookies_clear: "Delete all",
    cookies_clear_confirm: "Delete every cookie of this collection? Servers will set them again on the next requests.",
    cookies_delete_domain: "Delete domain",
    cookies_edit: "Edit",
    cookies_delete: "Delete",
    cookies_add_title: "New cookie",
    cookies_edit_title: "Edit cookie",
    cookies_name: "Name",
    cookies_value: "Value",
    cookies_domain: "Domain",
    cookies_path: "Path",
    cookies_expires: "Expires (UTC)",
    cookies_expires_hint: "Session · 2030-01-31 12:00:00",
    cookies_flags: "Flags",
    cookies_session: "Session",
    cookies_expired: "Expired",
    cookies_subdomains: "subdomains",
    cookies_same_site: "SameSite",
    cookies_host_only: "Only this host, not its subdomains",
    cookies_secure: "Secure: only over HTTPS (or to this machine)",
    cookies_http_only: "HttpOnly: not for scripts",
    cookies_bad_name: "The name cannot be empty or have =, ; or spaces.",
    cookies_bad_domain: "The domain cannot be empty.",
    cookies_bad_path: "The path must start with /.",
    cookies_bad_expires: "Write the expiry as 2030-01-31 12:00:00, or leave it empty for a session cookie.",
    response_no_cookies: "This response set no cookies. Responses opened from the history do not keep them.",
    send_cookies: "Send cookies",
    store_cookies: "Store cookies",
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
    tests: "Tests",
    assertions: "Assertions",
    captured_for_environment: "Captured for the environment {environment}: {names}. They are used for now; save them to keep them.",
    save_to_environment: "Save to environment",
    discard_captures: "Discard",
    runner: "Runner",
    run_menu: "Run…",
    run_recursive: "Subfolders",
    run_tags: "Only tags",
    run_exclude_tags: "Skip tags",
    run_delay: "Delay",
    run_iterations: "Iterations",
    run_data: "Data file",
    run_data_pick: "Choose CSV or JSON…",
    run_data_clear: "Remove",
    run_bail: "Stop at the first failure",
    run_start: "Run",
    run_stop: "Stop",
    run_clear: "Clear results",
    run_iteration: "iteration",
    run_summary: "{passed} passed · {failed} failed · {errors} errors · {skipped} skipped · {time}",
    run_export: "Save report:",
    run_skip: "SKIP",
    run_error: "ERROR",
    run_open_step: "Open with this response",
    filter_body: "Filter the body",
    filter_jsonpath: "Filter with JSONPath: $.items[*].id",
    filter_xpath: "Filter with XPath: //item/@id",
    filter_results: "{count} selected",
    auth_digest: "Digest",
    auth_ntlm: "NTLM",
    ntlm_domain: "Domain (optional)",
    auth_aws_v4: "AWS Signature v4",
    aws_access_key_id: "Access key ID",
    aws_secret_access_key: "Secret access key",
    aws_session_token: "Session token (optional)",
    aws_service: "Service",
    aws_region: "Region",
    auth_oauth2: "OAuth 2.0",
    oauth2_flow: "Grant type",
    oauth2_client_credentials: "Client credentials",
    oauth2_password: "Password",
    oauth2_authorization_code: "Authorization code",
    oauth2_implicit: "Implicit (not supported)",
    oauth2_authorization_url: "Authorization URL",
    oauth2_callback_url: "Callback URL",
    oauth2_access_token_url: "Access token URL",
    oauth2_refresh_token_url: "Refresh token URL (optional)",
    oauth2_client_id: "Client ID",
    oauth2_client_secret: "Client secret",
    oauth2_scope: "Scope",
    oauth2_state: "State",
    oauth2_send_credentials: "Send client credentials",
    oauth2_in_body: "In the body",
    oauth2_in_basic: "As Basic auth",
    oauth2_pkce: "Use PKCE",
    oauth2_token_in: "Send the token",
    oauth2_header_prefix: "Prefix",
    oauth2_query_key: "Parameter",
    oauth2_token_id: "Token ID",
    oauth2_token_source: "Token to send",
    oauth2_automatic: "Automatically",
    oauth2_auto_fetch: "Get a token when there is none",
    oauth2_auto_refresh: "Refresh an expired token",
    oauth2_parameters_kept: "{count} extra parameters, kept from the file",
    token_label: "Token",
    token_none: "none yet: one is obtained when sending",
    token_expired: "expired",
    token_expires_in: "valid for {time}",
    token_no_expiry: "valid, no expiry given",
    token_get: "Get new token",
    token_forget: "Forget token",
    proxy: "Proxy",
    proxy_system: "System",
    proxy_off: "None",
    proxy_manual: "Manual",
    proxy_inherit: "The application's",
    proxy_custom: "Its own",
    proxy_protocol: "Protocol",
    proxy_host: "Host",
    proxy_port: "Port",
    proxy_auth: "Credentials",
    proxy_bypass: "Direct for",
    proxy_password_saved: "saved, sealed",
    proxy_save_password: "Save password",
    proxy_forget_password: "Forget",
    proxy_hint: "For collections that use the application's proxy. System reads HTTP_PROXY, HTTPS_PROXY, ALL_PROXY and NO_PROXY. The password is kept sealed with the master key, never in settings.yml.",
    collection_settings: "Collection settings",
    collection_settings_menu: "Collection settings…",
    collection_proxy_hint: "Saved in opencollection.yml, password included, as Bruno does: use a proxy without credentials, or the application's, for collections in git.",
    tls: "TLS",
    tls_ca_file: "Extra CA file (PEM)",
    tls_insecure: "Do not check certificates",
    tls_insecure_warning: "Server certificates are not checked: anyone on the way can read and change the traffic. Turn it off when you are done.",
    choose: "Choose…",
    client_certificates: "Client certificates",
    client_certificate_domain: "Host",
    client_certificate_file: "Certificate (PEM)",
    client_certificate_key: "Private key (PEM)",
    client_certificate_add: "Add certificate",
    client_certificates_hint: "Paths are relative to the collection. Host takes a port and * wildcards: localhost:8443, *.example.com. PKCS#12 files and encrypted keys are kept but not presented yet.",
    pass: "PASS",
    fail: "FAIL",
    captures: "Captures",
    expression: "res.body.id",
    operator: "Operator",
    expected: "Expected",
    no_expected: "no value",
    capture_variable: "Variable",
    capture_scope: "Scope",
    tests_hint: "Checked after every send, captures first. Expressions: res.status, res.headers.<name>, res.body.<path> (JSONPath too), res.responseTime, res.size.",
    tests_none: "No assertions or captures ran. Responses from the history do not keep their results.",
    captured_as: "=",
    event_redirect: "Redirect",
    event_dns_override: "DNS override",
    event_decompressed: "Decompressed",
    event_dns: "DNS",
    event_connected: "Connected to",
    event_reused: "Reused connection to",
    event_tls: "TLS handshake,",
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
