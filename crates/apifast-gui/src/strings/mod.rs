//! Visible texts. Components receive them as parameters and never choose a
//! language; the screens pass the active [`Strings`].
//!
//! Each language is a file of this folder named by its code (`en.rs`,
//! `es.rs`, `pt_br.rs`), with its `NAME` and its `STRINGS`. That file is the
//! whole of adding one: the build script finds it and lists it in
//! [`LANGUAGES`], the compiler refuses it while a text is missing, and the
//! tests check that its templates have the same `{placeholders}` as
//! English's. A text with a count is a [`Plural`], picked by the language's
//! [`PluralRule`].

use std::fmt::Display;

use apifast_domain::Origin;

// `pub mod en; pub mod es; …` and `LANGUAGES`, one per file of this folder.
include!(concat!(env!("OUT_DIR"), "/languages.rs"));

/// English: what a language that is not there falls back to.
pub const EN: Strings = en::STRINGS;

/// A language the interface can show.
#[derive(Debug, Clone, Copy)]
pub struct Language {
    /// Its code: the name of its file, `en` or `pt_br`.
    pub code: &'static str,
    /// Its name, in it.
    pub name: &'static str,
    /// Its texts.
    pub strings: &'static Strings,
}

/// English, the language that is always there.
#[must_use]
pub fn english() -> &'static Language {
    const ENGLISH: Language = Language {
        code: "en",
        name: en::NAME,
        strings: &EN,
    };
    LANGUAGES
        .iter()
        .find(|l| l.code == "en")
        .unwrap_or(&ENGLISH)
}

/// The language `code` names, or English if apifast does not have it.
#[must_use]
pub fn english_or(code: &str) -> &'static Language {
    language(code).unwrap_or_else(english)
}

/// The language `code` names (`es`, `es_CO`, `es-CO.UTF-8`, `pt_BR`): the
/// one for its region if there is one, otherwise the one for its language.
/// `None` if neither is there.
#[must_use]
pub fn language(code: &str) -> Option<&'static Language> {
    let code = code.split(['.', '@']).next().unwrap_or_default();
    let code = code.replace('-', "_").to_lowercase();
    let base = code.split('_').next().unwrap_or_default();
    (LANGUAGES.iter().find(|l| l.code == code))
        .or_else(|| LANGUAGES.iter().find(|l| l.code == base))
}

/// The system's language, from what `var` gives for `LC_ALL`,
/// `LC_MESSAGES` and `LANG`, the first that is set; `None` when none is,
/// or it is `C` or `POSIX`.
pub fn system_language(var: impl Fn(&str) -> Option<String>) -> Option<&'static Language> {
    let code = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| var(name))
        .find(|value| !value.is_empty())?;
    language(&code)
}

/// Which form of a [`Plural`] a count takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// One, in English and Spanish.
    One,
    /// For languages with a form for a few (Polish, Russian, Arabic).
    Few,
    /// For languages with a form for many.
    Many,
    /// Every other count.
    Other,
}

/// How a language picks the form of a count.
#[derive(Clone, Copy)]
pub struct PluralRule(pub fn(u64) -> Form);

impl PluralRule {
    /// One for 1, other for the rest: English, Spanish and many more.
    pub const ONE_OTHER: Self = Self(|n| if n == 1 { Form::One } else { Form::Other });
}

impl std::fmt::Debug for PluralRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PluralRule")
    }
}

/// A text with a count, `{n}`, in each form its language has.
#[derive(Debug, Clone, Copy)]
pub struct Plural {
    /// For [`Form::One`].
    pub one: &'static str,
    /// For [`Form::Few`].
    pub few: &'static str,
    /// For [`Form::Many`].
    pub many: &'static str,
    /// For [`Form::Other`].
    pub other: &'static str,
}

impl Plural {
    /// A text with two forms, the most languages have: few and many are
    /// the other.
    #[must_use]
    pub const fn two(one: &'static str, other: &'static str) -> Self {
        Self {
            one,
            few: other,
            many: other,
            other,
        }
    }

    fn form(&self, form: Form) -> &'static str {
        match form {
            Form::One => self.one,
            Form::Few => self.few,
            Form::Many => self.many,
            Form::Other => self.other,
        }
    }
}

/// `template` with each `{name}` of `values` replaced by its value. What
/// is not a placeholder of `values`, `{{host}}` for one, stays as it is.
#[must_use]
pub fn fill(template: &str, values: &[(&str, &dyn Display)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let value = rest.find('}').and_then(|end| {
            let name = &rest[1..end];
            let (_, value) = values.iter().find(|(key, _)| *key == name)?;
            Some((end, value))
        });
        if let Some((end, value)) = value {
            let _ = write!(out, "{value}");
            rest = &rest[end + 1..];
        } else {
            out.push('{');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// A text of [`Strings`]: what the checks of the languages read.
trait Text {
    fn visit(&self, field: &'static str, visit: &mut dyn FnMut(&'static str, &'static str));
}

impl Text for &'static str {
    fn visit(&self, field: &'static str, visit: &mut dyn FnMut(&'static str, &'static str)) {
        visit(field, self);
    }
}

impl Text for Plural {
    fn visit(&self, field: &'static str, visit: &mut dyn FnMut(&'static str, &'static str)) {
        for text in [self.one, self.few, self.many, self.other] {
            visit(field, text);
        }
    }
}

impl Text for PluralRule {
    fn visit(&self, _: &'static str, _: &mut dyn FnMut(&'static str, &'static str)) {}
}

/// A struct of texts, with `each` over them; it is a [`Text`] too, so one
/// can hold another.
macro_rules! texts {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $($(#[$field_meta:meta])* pub $field:ident: $ty:ty,)* }
    ) => {
        $(#[$meta])*
        pub struct $name { $($(#[$field_meta])* pub $field: $ty,)* }

        impl $name {
            /// Calls `visit` with each text and its field's name; each form
            /// of a [`Plural`], and the texts of a struct it holds.
            pub fn each(&self, mut visit: impl FnMut(&'static str, &'static str)) {
                Text::visit(self, "", &mut visit);
            }
        }

        impl Text for $name {
            fn visit(&self, _: &'static str, visit: &mut dyn FnMut(&'static str, &'static str)) {
                $(Text::visit(&self.$field, stringify!($field), visit);)*
            }
        }
    };
}

texts! {
/// Every text the interface shows.
#[derive(Debug, Clone, Copy)]
pub struct Strings {
    /// How this language picks the form of a count.
    pub plural_rule: PluralRule,
    /// The errors and notices of the layers under the interface.
    pub errors: Errors,
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
    /// The history's heading for a request: `{name}`.
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
    /// Asks before deleting the environment `{name}`.
    pub delete_environment_confirm: &'static str,
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
    /// Title of the editor's request pane.
    pub pane_request: &'static str,
    /// Title of the editor's response pane.
    pub pane_response: &'static str,
    /// Button on a pane's tabs that opens it in a window of its own.
    pub pane_take_out: &'static str,
    /// Button in a pane's own window that puts it back.
    pub pane_bring_back: &'static str,
    /// Action that puts the request and response side by side or one under the other.
    pub action_toggle_layout: &'static str,
    /// Title of the Layouts window.
    pub layouts_title: &'static str,
    /// What the Layouts window says first.
    pub layouts_hint: &'static str,
    /// Shown when no layout was saved.
    pub layouts_none: &'static str,
    /// Button that puts a saved layout on screen.
    pub layout_apply: &'static str,
    /// Placeholder of a layout's name.
    pub layout_name_hint: &'static str,
    /// Button that saves the layout on screen with the name typed.
    pub layout_save: &'static str,
    /// Button that puts the default layout on screen.
    pub layout_reset: &'static str,
    /// Told when a layout in layouts.yml cannot be read.
    pub layout_not_understood: &'static str,
    /// Action that opens the Layouts window.
    pub action_layouts: &'static str,
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
    /// A file dialog's filter for every file.
    pub all_files: &'static str,
    /// Settings: the interface's language.
    pub language: &'static str,
    /// The language: the system's.
    pub language_system: &'static str,
    /// A file dialog's filter for data files.
    pub csv_or_json: &'static str,
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
    /// Button: pick a Bruno collection folder to import.
    pub import_bruno_folder: &'static str,
    /// Hover text of that button.
    pub import_bruno_folder_hint: &'static str,
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
    /// The folders an import creates: `{n}`.
    pub import_folders: Plural,
    /// The requests an import creates: `{n}`.
    pub import_requests: Plural,
    /// The environments an import creates: `{n}`.
    pub import_environments_count: Plural,
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
    /// Menu: export the collection to Postman.
    pub export_postman_menu: &'static str,
    /// Where an export went: `{file}`.
    pub exported_to: &'static str,
    /// After an export: the environments written beside it, `{n}`.
    pub exported_environments: Plural,
    /// Before what an export did not keep.
    pub export_not_kept: &'static str,
    /// Before why an export failed.
    pub export_failed: &'static str,
    /// Menu: import a new version of the collection's source.
    pub reimport_menu: &'static str,
    /// Title of the reimport window.
    pub reimport_title: &'static str,
    /// Label: the collection being updated.
    pub reimport_into: &'static str,
    /// Field: the new version of the source.
    pub reimport_file: &'static str,
    /// Before reviewing a reimport.
    pub reimport_hint: &'static str,
    /// A request new in the source.
    pub reimport_new: &'static str,
    /// A request changed only in the source.
    pub reimport_changed: &'static str,
    /// A request changed in the source and here.
    pub reimport_conflict: &'static str,
    /// A request deleted here.
    pub reimport_deleted: &'static str,
    /// A request the source no longer has.
    pub reimport_removed: &'static str,
    /// After a count: requests the same as the source.
    pub reimport_unchanged: &'static str,
    /// When the source did not change.
    pub reimport_nothing: &'static str,
    /// Button: apply the chosen changes.
    pub reimport_apply: &'static str,
    /// Menu and action: copy a request as a cURL command.
    pub copy_as_curl: &'static str,
    /// Menu and action: copy a request as JavaScript.
    pub copy_as_javascript: &'static str,
    /// Menu and action: copy a request as Python.
    pub copy_as_python: &'static str,
    /// Menu and action: copy a request as Go.
    pub copy_as_go: &'static str,
    /// Submenu: copy a request as code.
    pub copy_as: &'static str,
    /// A request copied as code, in `{language}`.
    pub copied_as: &'static str,
    /// A request copied as code, in `{language}`, without what `{missing}` names.
    pub copied_missing: &'static str,
    /// Before why a request could not be copied.
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
    /// Title of the git window.
    pub git_title: &'static str,
    /// The git window, for a collection in no repository.
    pub git_no_repository: &'static str,
    /// The branch, when HEAD is on none.
    pub git_detached: &'static str,
    /// Before the branch, in the git window.
    pub git_branch: &'static str,
    /// The branch chooser, before one is chosen.
    pub git_branch_pick: &'static str,
    /// Button: check out the branch chosen.
    pub git_switch: &'static str,
    /// Button: read the repository again.
    pub git_refresh: &'static str,
    /// The git window, with no change.
    pub git_nothing_changed: &'static str,
    /// The git window's diff, before a file is picked.
    pub git_pick_file: &'static str,
    /// Hint of the commit message.
    pub git_message: &'static str,
    /// Button: commit the files checked.
    pub git_commit: &'static str,
    /// The box of a file, for screen readers: whether it goes in the commit.
    pub git_include: &'static str,
    /// Diff of a new file.
    pub git_added: &'static str,
    /// Diff of a deleted file.
    pub git_deleted: &'static str,
    /// Diff of a file whose content changed but not what apifast reads.
    pub git_same: &'static str,
    /// A field that was not there, or is not anymore.
    pub git_none: &'static str,
    /// The status bar: how many files changed since the last commit, `{n}`.
    pub git_changed: Plural,
    /// After a commit: `{n}` files, the commit `{id}`.
    pub git_committed: Plural,
    /// After switching to the branch `{branch}`.
    pub git_switched: &'static str,
    /// Refuses to switch to `{branch}` while `{n}` tabs have unsaved changes.
    pub git_unsaved_tabs: Plural,
    /// Action: open the git window.
    pub action_git: &'static str,
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
    /// Rename this entry: a named entry is kept whatever the history's limits.
    pub history_rename: &'static str,
    /// Placeholder of an entry's name while renaming it.
    pub history_name_hint: &'static str,
    /// Hover of a named entry.
    pub history_named: &'static str,
    /// Button that empties the history.
    pub history_clear: &'static str,
    /// Title of the question before emptying the history.
    pub history_clear_title: &'static str,
    /// What emptying the history does.
    pub history_clear_warning: &'static str,
    /// Button that empties the history, once asked.
    pub history_clear_confirm: &'static str,
    /// Button with the earlier responses of the request shown.
    pub response_history: &'static str,
    /// Hover of that button.
    pub response_history_hint: &'static str,
    /// Tab of the response with the request as it was sent.
    pub sent_request: &'static str,
    /// Shown when the history has no record of what was sent.
    pub sent_not_kept: &'static str,
    /// What the Request tab shows, and what it does not.
    pub sent_note: &'static str,
    /// Hover of the button that copies the request sent.
    pub sent_copy: &'static str,
    /// Below a body kept in part; {n} is how many bytes were left out.
    pub sent_body_cut: &'static str,
    /// A send that failed, in the history.
    pub failed: &'static str,
    /// How long ago, in seconds: `{n}`.
    pub seconds_ago: &'static str,
    /// How long ago, in minutes: `{n}`.
    pub minutes_ago: &'static str,
    /// How long ago, in hours: `{n}`.
    pub hours_ago: &'static str,
    /// How long ago, in days: `{n}`.
    pub days_ago: &'static str,
    /// Response tab with the events of the send.
    pub timeline: &'static str,
    /// Shown when a response has no timeline.
    pub timeline_empty: &'static str,
    /// Response tab with the events of an event stream.
    pub events: &'static str,
    /// Placeholder of the field that filters the events.
    pub events_filter: &'static str,
    /// Shown while a stream has sent no event yet.
    pub events_waiting: &'static str,
    /// Shown when no event matches the filter.
    pub events_none_match: &'static str,
    /// Shown when the stream has more events than the list reads.
    pub events_partial: &'static str,
    /// Shown next to the spinner while an event stream arrives.
    pub streaming: &'static str,
    /// Button that ends an event stream, keeping what arrived.
    pub stop: &'static str,
    /// Header of an event's name column.
    pub event_name: &'static str,
    /// Header of an event's id column.
    pub event_id: &'static str,
    /// Header of an event's data column.
    pub event_data: &'static str,
    /// Label of an event's reconnection time.
    pub event_retry: &'static str,
    /// Shown when no event is selected.
    pub event_pick: &'static str,
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
    /// The runner's progress: iteration `{n}` of `{total}`.
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
    /// The nodes a filter of the body selects: `{n}`.
    pub filter_results: Plural,
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
    /// Extra OAuth 2.0 parameters the file has and the editor keeps: `{n}`.
    pub oauth2_parameters_kept: Plural,
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
    /// Response tab: heading of the tests of the scripts.
    pub script_tests: &'static str,
    /// Request tab and window title: the scripts.
    pub scripts: &'static str,
    /// Menu of a folder or the collection: edit its scripts.
    pub scripts_menu: &'static str,
    /// Above the scripts: what they are and what they see.
    pub scripts_hint: &'static str,
    /// Request tab and window title: the variables of the request, a folder
    /// or the collection.
    pub variables: &'static str,
    /// Menu of a folder or the collection: edit its variables.
    pub variables_menu: &'static str,
    /// Above the variables of a folder or the collection: who sees them and
    /// what overrides them.
    pub variables_hint: &'static str,
    /// Above the variables of a request: what they override.
    pub request_variables_hint: &'static str,
    /// In place of the value of a secret that is not an environment's.
    pub variable_secret_elsewhere: &'static str,
    /// When a file has several scripts of one kind: only the first is
    /// edited.
    pub scripts_more: &'static str,
    /// The script that runs before the request.
    pub script_before_request: &'static str,
    /// The script that runs after the response.
    pub script_after_response: &'static str,
    /// The tests script.
    pub script_tests_kind: &'static str,
    /// Response tab: heading of what the scripts wrote to the console.
    pub script_console: &'static str,
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
    /// Body kind: a GraphQL operation.
    pub body_graphql: &'static str,
    /// Editor of a GraphQL body: the operation.
    pub graphql_query: &'static str,
    /// Editor of a GraphQL body: its variables.
    pub graphql_variables: &'static str,
    /// Button: ask the server for its GraphQL schema.
    pub schema_fetch: &'static str,
    /// Button: ask for the schema again.
    pub schema_fetch_again: &'static str,
    /// While the schema is being asked for.
    pub schema_fetching: &'static str,
    /// When no schema is known yet.
    pub schema_none: &'static str,
    /// Before why fetching the schema failed.
    pub schema_failed: &'static str,
    /// Heading of the schema explorer.
    pub schema_explorer: &'static str,
    /// The types of a GraphQL schema: `{n}`.
    pub schema_types: Plural,
    /// Button: back to the previous type.
    pub schema_back: &'static str,
    /// Field: filter the schema's types.
    pub schema_filter: &'static str,
    /// Heading: the schema's root types.
    pub schema_roots: &'static str,
    /// Heading: every type of the schema.
    pub schema_all_types: &'static str,
    /// Heading: a type's fields.
    pub schema_fields: &'static str,
    /// Heading: an enum's values.
    pub schema_values: &'static str,
    /// Heading: a union's or interface's types.
    pub schema_possible: &'static str,
    /// Mark of a deprecated field or value.
    pub schema_deprecated: &'static str,
    /// Before the interfaces a type implements.
    pub schema_implements: &'static str,
    /// Body type: URL-encoded form.
    pub body_form: &'static str,
    /// Body type: multipart.
    pub body_multipart: &'static str,
    /// Body type: file.
    pub body_file: &'static str,
    /// Shown for body types this version cannot edit.
    pub body_not_editable: &'static str,
    /// Request tab with a WebSocket request's messages.
    pub ws_messages: &'static str,
    /// How many messages a connection had: `{n}`.
    pub ws_message_count: Plural,
    /// Placeholder of a WebSocket message's name.
    pub ws_message_name: &'static str,
    /// Button that adds a WebSocket message.
    pub ws_add_message: &'static str,
    /// Button that removes the WebSocket message shown.
    pub ws_remove_message: &'static str,
    /// Shown when a WebSocket request has no message.
    pub ws_no_messages: &'static str,
    /// Hover text of the selector of the message shown.
    pub ws_message_shown: &'static str,
    /// Hover text of a message's kind selector.
    pub ws_message_kind: &'static str,
    /// Shown instead of the method of a WebSocket request.
    pub ws_badge: &'static str,
    /// Button that opens a WebSocket connection.
    pub ws_connect: &'static str,
    /// Button that closes a WebSocket connection.
    pub ws_disconnect: &'static str,
    /// Shown next to the spinner while a WebSocket connection is open.
    pub ws_connected: &'static str,
    /// Button that sends the message shown over the open connection.
    pub ws_send: &'static str,
    /// Hover of the send button.
    pub ws_send_hint: &'static str,
    /// Hover of the send button while there is no connection.
    pub ws_send_disconnected: &'static str,
    /// Placeholder of the field that filters a connection's messages.
    pub ws_filter: &'static str,
    /// Shown while nothing went over a connection yet.
    pub ws_waiting: &'static str,
    /// Shown when no message matches the filter.
    pub ws_none_match: &'static str,
    /// Shown when a connection's log is larger than the list reads.
    pub ws_partial: &'static str,
    /// Asks to pick a message to see it whole.
    pub ws_pick: &'static str,
    /// A message that went out.
    pub ws_sent: &'static str,
    /// A message that arrived.
    pub ws_received: &'static str,
    /// A message that could not be sent, or a broken connection.
    pub ws_failed: &'static str,
    /// The connection closed: by apifast.
    pub ws_closed_by_client: &'static str,
    /// The connection closed: by the server.
    pub ws_closed_by_server: &'static str,
    /// Size of a binary message, before its bytes.
    pub ws_binary: &'static str,
    /// Hover of the `gRPC` badge in the URL bar.
    pub grpc_badge: &'static str,
    /// The headers tab of a gRPC request.
    pub grpc_metadata: &'static str,
    /// Label of the method a gRPC request calls.
    pub grpc_method: &'static str,
    /// Placeholder of the method field.
    pub grpc_method_hint: &'static str,
    /// Label of how the method streams.
    pub grpc_method_type: &'static str,
    /// The method type is not known yet.
    pub grpc_type_unknown: &'static str,
    /// Method type: unary.
    pub grpc_unary: &'static str,
    /// Method type: client streaming.
    pub grpc_client_streaming: &'static str,
    /// Method type: server streaming.
    pub grpc_server_streaming: &'static str,
    /// Method type: bidirectional streaming.
    pub grpc_bidi_streaming: &'static str,
    /// Label of the `.proto` file of a gRPC request.
    pub grpc_proto: &'static str,
    /// Placeholder of the `.proto` field.
    pub grpc_proto_hint: &'static str,
    /// Shown when a gRPC request has no message.
    pub grpc_no_messages: &'static str,
    /// Button that calls a gRPC request.
    pub grpc_call: &'static str,
    /// Shown next to the spinner while a gRPC call is open.
    pub grpc_calling: &'static str,
    /// Button that loads the methods of a gRPC request.
    pub grpc_load_methods: &'static str,
    /// Hover of that button.
    pub grpc_load_methods_hint: &'static str,
    /// Shown while the methods load.
    pub grpc_loading: &'static str,
    /// The selector of the methods loaded.
    pub grpc_pick_method: &'static str,
    /// Shown when the server or the `.proto` has no method.
    pub grpc_no_methods: &'static str,
    /// Hover of "Send" in a gRPC call.
    pub grpc_send_hint: &'static str,
    /// Hover of the call's buttons while there is no call.
    pub grpc_send_closed: &'static str,
    /// Button that ends the client's stream of a gRPC call.
    pub grpc_end_stream: &'static str,
    /// Hover of that button.
    pub grpc_end_stream_hint: &'static str,
    /// Button that formats the body.
    pub format: &'static str,
    /// Hover of the format button, with its shortcut.
    pub format_hint: &'static str,
    /// A body that cannot be formatted, at `{line}`.
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
    /// Settings: the section of the log.
    pub settings_log: &'static str,
    /// Settings: how much is logged.
    pub settings_log_level: &'static str,
    /// Settings: the log's file.
    pub settings_log_file: &'static str,
    /// Settings: `APIFAST_LOG` sets the log, with `{filter}`.
    pub settings_log_env: &'static str,
    /// Settings: what the log is for.
    pub settings_log_hint: &'static str,
    /// The log level: errors.
    pub log_error: &'static str,
    /// The log level: warnings.
    pub log_warn: &'static str,
    /// The log level: information.
    pub log_info: &'static str,
    /// The log level: debug.
    pub log_debug: &'static str,
    /// The log level: everything.
    pub log_trace: &'static str,
    /// Settings section: general.
    pub settings_general: &'static str,
    /// Settings section: the code editor.
    pub settings_editor: &'static str,
    /// Settings section: network.
    pub settings_network: &'static str,
    /// Settings section: keyboard shortcuts.
    pub settings_shortcuts: &'static str,
    /// Settings section: data and folders.
    pub settings_data: &'static str,
    /// Setting: open the last workspace at the start.
    pub settings_reopen_workspace: &'static str,
    /// Hover of reopening the last workspace.
    pub settings_reopen_workspace_hint: &'static str,
    /// Setting: ask before closing with unsaved changes.
    pub settings_confirm_close: &'static str,
    /// Explains that unsaved changes are kept on closing.
    pub settings_confirm_close_hint: &'static str,
    /// Setting: accent color.
    pub settings_accent: &'static str,
    /// Accent: blue.
    pub accent_blue: &'static str,
    /// Accent: green.
    pub accent_green: &'static str,
    /// Accent: teal.
    pub accent_teal: &'static str,
    /// Accent: purple.
    pub accent_purple: &'static str,
    /// Accent: pink.
    pub accent_pink: &'static str,
    /// Accent: orange.
    pub accent_orange: &'static str,
    /// Setting: how much room controls have.
    pub settings_density: &'static str,
    /// Density: compact.
    pub density_compact: &'static str,
    /// Density: normal.
    pub density_normal: &'static str,
    /// Density: comfortable.
    pub density_comfortable: &'static str,
    /// Setting: the font of code.
    pub settings_code_font: &'static str,
    /// Hint of the code font.
    pub settings_code_font_hint: &'static str,
    /// The code font could not be used, with `{error}`.
    pub settings_code_font_error: &'static str,
    /// The code font file is not a font.
    pub settings_code_font_invalid: &'static str,
    /// The code font when none is chosen: the one included.
    pub settings_code_font_default: &'static str,
    /// Button that goes back to the included code font.
    pub settings_code_font_reset: &'static str,
    /// Setting: the size of code.
    pub settings_code_size: &'static str,
    /// Setting: what the Tab key inserts in code.
    pub settings_tab: &'static str,
    /// Tab option: a tab character.
    pub tab_character: &'static str,
    /// Tab option: `{n}` spaces.
    pub tab_spaces: &'static str,
    /// Setting: wrap long lines of code.
    pub settings_wrap_lines: &'static str,
    /// Setting: show line numbers in code.
    pub settings_line_numbers: &'static str,
    /// Setting: the default timeout.
    pub settings_timeout: &'static str,
    /// Explains the default timeout.
    pub settings_timeout_hint: &'static str,
    /// Explains the shortcuts section.
    pub settings_shortcuts_hint: &'static str,
    /// Button that opens the shortcuts editor.
    pub settings_edit_shortcuts: &'static str,
    /// Settings: the folders apifast uses.
    pub settings_folders: &'static str,
    /// Folder: data (history, responses, workspaces).
    pub folder_data: &'static str,
    /// Folder: settings and shortcuts.
    pub folder_config: &'static str,
    /// Folder: the log.
    pub folder_logs: &'static str,
    /// Button that opens a folder in the system's file manager.
    pub settings_open_folder: &'static str,
    /// The space the data takes, `{size}`.
    pub settings_space_used: &'static str,
    /// While the space the data takes is measured.
    pub settings_space_measuring: &'static str,
    /// A folder could not be opened, with `{error}`.
    pub settings_folder_error: &'static str,
    /// Title of the question before closing with unsaved changes.
    pub close_title: &'static str,
    /// Button: save every tab, then close.
    pub close_save_all: &'static str,
    /// Button: close without saving.
    pub close_anyway: &'static str,
    /// Name of the application menu's button.
    pub menu: &'static str,
    /// Action: open the About window.
    pub action_about: &'static str,
    /// Action: quit apifast.
    pub action_quit: &'static str,
    /// Title of the About window.
    pub about_title: &'static str,
    /// What apifast is, under its name.
    pub about_summary: &'static str,
    /// Label: the version.
    pub about_version: &'static str,
    /// Label: apifast's license.
    pub about_license: &'static str,
    /// Link to apifast's site.
    pub about_site: &'static str,
    /// Link to report a bug.
    pub about_bugs: &'static str,
    /// Label: the operating system.
    pub about_system: &'static str,
    /// Label: the graphics backend and adapter.
    pub about_graphics: &'static str,
    /// Button: copy what a bug report needs.
    pub about_copy: &'static str,
    /// Said once the information is copied.
    pub about_copied: &'static str,
    /// The build had no list of dependencies.
    pub about_dependencies_none: &'static str,
    /// Explains the list of dependencies.
    pub about_dependencies_hint: &'static str,
    /// Dependencies under the less used licenses, `{n}`.
    pub about_other_licenses: Plural,
    /// The count of dependencies, `{n}`.
    pub about_dependencies: Plural,
    /// Window button: minimize.
    pub window_minimize: &'static str,
    /// Window button: maximize.
    pub window_maximize: &'static str,
    /// Window button: restore from maximized.
    pub window_restore: &'static str,
    /// Window button: close.
    pub window_close: &'static str,
    /// Name of the × of a window inside apifast's (settings, environments…).
    pub close_window: &'static str,
    /// Button in the title bar that opens the command palette.
    pub title_search: &'static str,
    /// Setting: apifast draws the title bar.
    pub settings_own_title_bar: &'static str,
    /// Explains the title bar setting.
    pub settings_own_title_bar_hint: &'static str,
    /// Title of the main area with no collection open.
    pub empty_collection_title: &'static str,
    /// Title of the main area with no request open.
    pub empty_request_title: &'static str,
    /// How many tabs have unsaved changes, `{n}`.
    pub close_unsaved: Plural,
}
}

texts! {
/// The errors and notices of the layers under the interface (the
/// application, the vault, the importers, git), in the interface's language.
/// Each is filled with what the error carries: [`crate::messages`].
#[derive(Debug, Clone, Copy)]
pub struct Errors {
    /// English: {path} does not exist.
    pub not_found: &'static str,
    /// English: {path} is not valid: {message}.
    pub invalid_file: &'static str,
    /// English: {path}: {what} is not supported yet.
    pub unsupported_file: &'static str,
    /// English: cannot access {path}: {message}.
    pub cannot_access: &'static str,
    /// English: invalid URL: {why}.
    pub invalid_url: &'static str,
    /// English: invalid request: {why}.
    pub invalid_request: &'static str,
    /// English: TLS error: {why}.
    pub tls_failed: &'static str,
    /// English: protocol error: {why}.
    pub protocol_failed: &'static str,
    /// English: stopped after {n} redirects.
    pub too_many_redirects: Plural,
    /// English: timed out after {ms} ms.
    pub timed_out: &'static str,
    /// English: cancelled.
    pub send_cancelled: &'static str,
    /// English: response store: {why}.
    pub response_store: &'static str,
    /// English: environment {name} does not exist.
    pub environment_not_found: &'static str,
    /// English: cycle in extends: {chain}.
    pub environment_cycle: &'static str,
    /// English: variable {name} is not defined.
    pub variable_undefined: &'static str,
    /// English: secret {name} has no value available.
    pub secret_unavailable: &'static str,
    /// English: variables reference each other in a loop: {chain}.
    pub variables_cycle: &'static str,
    /// English: variable {name} nests more than {max} levels deep.
    pub variable_too_deep: &'static str,
    /// English: unknown dynamic variable ${name}.
    pub dynamic_unknown: &'static str,
    /// English: process variable {name} is not set.
    pub process_env_missing: &'static str,
    /// English: no value given for prompt {name}.
    pub prompt_missing: &'static str,
    /// English: function {name} failed: {message}.
    pub function_failed: &'static str,
    /// English: git: set your name and email first: git config user.name, git config user.email.
    pub git_no_identity: &'static str,
    /// English: git: there is no branch {branch}.
    pub git_no_branch: &'static str,
    /// English: git: commit or discard the changes first: {files}.
    pub git_uncommitted: &'static str,
    /// English: git: the branch has files that are here untracked; move them first: {files}.
    pub git_untracked: &'static str,
    /// English: git: {why}.
    pub git_failed: &'static str,
    /// English: an environment needs a name.
    pub environment_name_empty: &'static str,
    /// English: an environment named {name} already exists.
    pub environment_exists: &'static str,
    /// English: {name} cannot be deleted: {children} extend it.
    pub environment_extended: &'static str,
    /// English: a folder cannot go inside itself.
    pub folder_into_itself: &'static str,
    /// English: {name} already exists in that folder.
    pub exists_in_folder: &'static str,
    /// English: the name cannot be empty.
    pub name_empty: &'static str,
    /// English: the request needs a name.
    pub request_name_empty: &'static str,
    /// English: {path} is not a folder of the collection.
    pub not_a_folder: &'static str,
    /// English: the server answered {status} to the introspection query.
    pub introspection_status: &'static str,
    /// English: the schema is larger than {mb} MB.
    pub schema_too_large: &'static str,
    /// English: the answer is not JSON: {why}.
    pub schema_not_json: &'static str,
    /// English: the server refused the introspection: {why}.
    pub introspection_refused: &'static str,
    /// English: the answer is not an introspection result.
    pub not_introspection: &'static str,
    /// English: a commit needs a message.
    pub commit_message_empty: &'static str,
    /// English: choose the files to commit.
    pub commit_nothing_chosen: &'static str,
    /// English: AWS Signature v4 needs the {field}.
    pub aws_missing: &'static str,
    /// English: access key id.
    pub aws_access_key_id: &'static str,
    /// English: secret access key.
    pub aws_secret_access_key: &'static str,
    /// English: service.
    pub aws_service: &'static str,
    /// English: region.
    pub aws_region: &'static str,
    /// English: an OAuth 2.0 request is rendered with its token, when it is sent.
    pub oauth2_rendered_alone: &'static str,
    /// English: auth scheme {kind} is not supported yet.
    pub auth_scheme_unsupported: &'static str,
    /// English: this auth scheme is not supported yet.
    pub auth_scheme_not_yet: &'static str,
    /// English: this body kind is not supported yet.
    pub body_kind_not_yet: &'static str,
    /// English: the GraphQL variables are not valid JSON: {why}.
    pub graphql_variables_not_json: &'static str,
    /// English: the GraphQL variables must be a JSON object.
    pub graphql_variables_not_object: &'static str,
    /// English: OAuth 2.0 needs the access token URL.
    pub oauth2_no_token_url: &'static str,
    /// English: OAuth 2.0 needs the {field}.
    pub oauth2_missing: &'static str,
    /// English: authorization URL.
    pub oauth2_field_authorization_url: &'static str,
    /// English: callback URL.
    pub oauth2_field_callback_url: &'static str,
    /// English: the token request failed ({status}): {reason}.
    pub token_request_failed: &'static str,
    /// English: the answer is not JSON.
    pub token_answer_not_json: &'static str,
    /// English: status {status}.
    pub token_status: &'static str,
    /// English: the token response has no `access_token`.
    pub no_access_token: &'static str,
    /// English: the token response has no `id_token`.
    pub no_id_token: &'static str,
    /// English: the implicit grant is deprecated and not supported.
    pub implicit_grant: &'static str,
    /// English: the authorization code grant needs a browser, which is not available here.
    pub no_browser: &'static str,
    /// English: the authorization was refused: {error}.
    pub authorization_refused: &'static str,
    /// English: the authorization answer has another state; try again.
    pub authorization_state: &'static str,
    /// English: the authorization answer has no code.
    pub authorization_no_code: &'static str,
    /// English: nobody authorized in the browser in time.
    pub authorization_timed_out: &'static str,
    /// English: the token cannot be refreshed.
    pub no_refresh_token: &'static str,
    /// English: the OAuth 2.0 token has expired.
    pub token_expired: &'static str,
    /// English: there is no OAuth 2.0 token, and getting one automatically is off.
    pub no_token: &'static str,
    /// English: this request does not use OAuth 2.0.
    pub not_oauth2: &'static str,
    /// English: the OAuth 2.0 callback URL {url} {problem}.
    pub callback_url: &'static str,
    /// English: is not a URL.
    pub callback_not_url: &'static str,
    /// English: must be http://localhost:‹port›/… or http://127.0.0.1:‹port›/…, so apifast can catch the redirect.
    pub callback_not_local: &'static str,
    /// English: cannot be listened on: {why}.
    pub callback_cannot_listen: &'static str,
    /// English: cannot open the browser: {why}; open this URL: {url}.
    pub browser_failed: &'static str,
    /// English: the OAuth 2.0 callback failed: {why}.
    pub callback_failed: &'static str,
    /// English: requests depend on each other: {chain}.
    pub dependency_cycle: &'static str,
    /// English: too many requests chained with `response()`.
    pub too_many_chained: &'static str,
    /// English: {dependency}, used by `response()`: {error}.
    pub dependency_failed: &'static str,
    /// English: {path} {kind}.
    pub wrong_kind: &'static str,
    /// English: is a WebSocket request: it connects, it is not sent.
    pub is_websocket: &'static str,
    /// English: is not a WebSocket request.
    pub not_websocket: &'static str,
    /// English: is a gRPC request: it is called, not sent.
    pub is_grpc: &'static str,
    /// English: is not a gRPC request.
    pub not_grpc: &'static str,
    /// English: the system keyring cannot be used: {why}.
    pub keyring_failed: &'static str,
    /// English: cannot use {path}: {message}.
    pub cannot_use: &'static str,
    /// English: the stored master key is damaged.
    pub master_key_damaged: &'static str,
    /// English: this collection already shares secrets: ask your team for its key.
    pub collection_shares_already: &'static str,
    /// English: secret {name} is shared with the team, and this machine does not have the collection's key.
    pub shared_without_key: &'static str,
    /// English: that is not this collection's key.
    pub wrong_collection_key: &'static str,
    /// English: this is not an apifast collection key (AFK- and 54 characters).
    pub key_format: &'static str,
    /// English: the key has a typo: its check does not match.
    pub key_check: &'static str,
    /// English: not an encrypted value.
    pub not_an_envelope: &'static str,
    /// English: encrypted with a newer version ({version}).
    pub newer_envelope: &'static str,
    /// English: cannot be decrypted: wrong key, or the value was changed or moved.
    pub decryption_rejected: &'static str,
    /// English: no randomness available: {why}.
    pub no_randomness: &'static str,
    /// English: {path} has no bruno.json: not a Bruno collection.
    pub bruno_not_collection: &'static str,
    /// English: cannot read {path}: {why}.
    pub cannot_read: &'static str,
    /// English: cannot write {path}: {why}.
    pub cannot_write: &'static str,
    /// English: not an Insomnia export (v4 or v5).
    pub not_insomnia: &'static str,
    /// English: not an OpenAPI document.
    pub not_openapi: &'static str,
    /// English: Swagger 2.0 is not supported yet; convert it to OpenAPI 3 first.
    pub swagger2: &'static str,
    /// English: not JSON: {why}.
    pub not_json: &'static str,
    /// English: not a Postman collection (v2.0 or v2.1).
    pub not_postman_collection: &'static str,
    /// English: not a Postman environment.
    pub not_postman_environment: &'static str,
    /// English: no environment {name}; the collection has: {known}.
    pub export_no_environment: &'static str,
    /// English: cannot import into {path}: {why}.
    pub import_into_failed: &'static str,
    /// English: not a curl command.
    pub not_curl: &'static str,
    /// English: a {quote} quote is not closed.
    pub curl_unclosed: &'static str,
    /// English: the curl command has no URL.
    pub curl_no_url: &'static str,
    /// English: {option} needs a value.
    pub curl_missing_value: &'static str,
    /// English: invalid {language} {expression}: {reason} at character {at}.
    pub query_syntax: &'static str,
    /// English: the body is not JSON.
    pub body_not_json: &'static str,
    /// English: the body is not XML: {why}.
    pub body_not_xml: &'static str,
    /// English: the query visits more than {max} values; make it narrower.
    pub query_too_expensive: &'static str,
    /// English: invalid regular expression {expression}.
    pub bad_regex: &'static str,
    /// English: the body is larger than {kb} KB, too large for an example.
    pub example_too_large: &'static str,
    /// English: the body is not text; binary bodies cannot be saved as an example yet.
    pub example_binary: &'static str,
    /// English: cannot move this collection's data: {why}.
    pub data_not_moved: &'static str,
    /// English: the vault cannot be opened.
    pub vault_unavailable: &'static str,
    /// English: the active workspace cannot be forgotten: switch first.
    pub workspace_active: &'static str,
    /// English: {path} is not a Postman collection (v2.0 or v2.1), an Insomnia export nor an OpenAPI 3 spec.
    pub not_importable: &'static str,
    /// English: {path} is not open.
    pub not_open: &'static str,
    /// English: {path}: {why}.
    pub file_problem: &'static str,
    /// English: the collection is not loaded.
    pub collection_not_loaded: &'static str,
    /// English: {path}: the data file must end in .csv or .json.
    pub data_file_extension: &'static str,
    /// English: this collection has no secrets yet.
    pub no_secrets_yet: &'static str,
    /// English: no collection is open.
    pub no_collection_open: &'static str,
    /// English: the environment {name} no longer exists.
    pub environment_gone: &'static str,
    /// English: a secret: its value is not imported; set it in the vault.
    pub note_secret_not_imported: &'static str,
    /// English: {n} secrets: Bruno keeps their values apart; set them in the vault.
    pub note_secrets_apart: Plural,
    /// English: a {mode} body is not supported.
    pub note_body_unsupported: &'static str,
    /// English: the {key} setting is not kept.
    pub note_setting_not_kept: &'static str,
    /// English: a {event} script is not imported: apifast does not run it.
    pub note_script_not_imported: &'static str,
    /// English: {n} lines of the {script} script use Postman's API that has no counterpart: they are commented out.
    pub note_script_lines_commented: Plural,
    /// English: scripts are not imported: they arrive with the JavaScript engine.
    pub note_scripts_not_imported: &'static str,
    /// English: {{${name}}} is not a variable apifast generates: sends with it fail until it is replaced.
    pub note_unknown_dynamic: &'static str,
    /// English: cannot be read (line {line}: {message}): left out.
    pub note_unparsable: &'static str,
    /// English: {kind} requests are not supported yet: left out.
    pub note_request_kind_unsupported: &'static str,
    /// English: a {kind} is not supported yet: left out.
    pub note_resource_unsupported: &'static str,
    /// English: {kind} auth is not supported: left without auth.
    pub note_auth_unsupported: &'static str,
    /// English: OAuth 2.0 grant {grant} is not supported.
    pub note_grant_unsupported: &'static str,
    /// English: OAuth 2.0 flow {flow} is not supported.
    pub note_flow_unsupported: &'static str,
    /// English: HTTP {scheme} security is not supported.
    pub note_http_security_unsupported: &'static str,
    /// English: an API key in {place} is not supported.
    pub note_api_key_in_unsupported: &'static str,
    /// English: {kind} security is not supported.
    pub note_security_unsupported: &'static str,
    /// English: security scheme {name} is not defined.
    pub note_security_scheme_undefined: &'static str,
    /// English: an OAuth 2.0 scheme without flows.
    pub note_oauth2_without_flows: &'static str,
    /// English: the spec has no servers: set baseUrl.
    pub note_no_servers: &'static str,
    /// English: its $ref cannot be followed.
    pub note_ref_not_followed: &'static str,
    /// English: a parameter's $ref cannot be followed.
    pub note_parameter_ref_not_followed: &'static str,
    /// English: the {location} parameter {name} is not imported.
    pub note_parameter_not_imported: &'static str,
    /// English: an XML body: written empty, as its schema is not turned into XML.
    pub note_xml_body_empty: &'static str,
    /// English: Insomnia template tags ({% … %}) are not imported: requests that use them need a variable instead.
    pub note_template_tags: &'static str,
    /// English: a GraphQL body that is not {"query": …}: imported as JSON.
    pub note_graphql_as_json: &'static str,
    /// English: the Postman environments were not read: a Bruno collection has its own.
    pub note_environments_ignored: &'static str,
    /// English: the folder variable {name} has the name of another one: not exported.
    pub note_folder_variable_clash: &'static str,
    /// English: {why}: exported with its own variables only.
    pub note_environment_alone: &'static str,
    /// English: it extends another environment: exported with the variables of both merged.
    pub note_environment_merged: &'static str,
    /// English: a secret: exported without its value.
    pub note_secret_without_value: &'static str,
    /// English: folder variables are moved to the collection: Postman has none.
    pub note_folder_variables_moved: &'static str,
    /// English: its headers are copied into each of its requests: Postman does not inherit headers.
    pub note_headers_copied: &'static str,
    /// English: its scripts are not exported: they use Bruno's API, not Postman's.
    pub note_scripts_not_exported: &'static str,
    /// English: its assertions are not exported: Postman checks responses in test scripts.
    pub note_assertions_not_exported: &'static str,
    /// English: its captures are not exported: Postman sets variables in scripts.
    pub note_captures_not_exported: &'static str,
    /// English: its variables are not exported: Postman requests have none.
    pub note_variables_not_exported: &'static str,
    /// English: its tags are not exported.
    pub note_tags_not_exported: &'static str,
    /// English: a SPARQL body: exported as text.
    pub note_sparql_as_text: &'static str,
    /// English: its body cannot be exported yet: left out.
    pub note_body_not_exported: &'static str,
    /// English: the AWS profile is not exported: Postman takes the keys.
    pub note_aws_profile_not_exported: &'static str,
    /// English: {kind} auth cannot be exported: left without auth.
    pub note_auth_not_exportable: &'static str,
    /// English: its auth cannot be exported yet: left without auth.
    pub note_auth_not_exported: &'static str,
    /// English: its timeout is not exported: Postman has one for all requests.
    pub note_timeout_not_exported: &'static str,
    /// English: a {kind} request cannot be exported yet: left out.
    pub note_request_not_exported: &'static str,
    /// English: cannot be read: {why}.
    pub note_unreadable: &'static str,
}
}

impl Strings {
    /// `plural` in the form `n` takes, with `{n}` and `values` filled.
    #[must_use]
    pub fn count(&self, plural: &Plural, n: usize, values: &[(&str, &dyn Display)]) -> String {
        let n = u64::try_from(n).unwrap_or(u64::MAX);
        let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
        all.extend_from_slice(values);
        fill(plural.form((self.plural_rule.0)(n)), &all)
    }

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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// The `{placeholders}` of `text`.
    fn placeholders(text: &str) -> BTreeSet<&str> {
        let mut found = BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            rest = &rest[start + 1..];
            if let Some(end) = rest.find('}') {
                let name = &rest[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    found.insert(name);
                }
            }
        }
        found
    }

    /// Each field's placeholders, every form of a plural together.
    fn all_placeholders(strings: &Strings) -> BTreeMap<&'static str, BTreeSet<&'static str>> {
        let mut fields: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        strings.each(|field, text| {
            fields.entry(field).or_default().extend(placeholders(text));
        });
        fields
    }

    #[test]
    fn every_language_has_the_placeholders_of_english_and_no_empty_text() {
        let english = all_placeholders(&EN);
        assert!(english["history_of"].contains("name"));
        for language in LANGUAGES {
            assert_eq!(
                all_placeholders(language.strings),
                english,
                "{}.rs: its {{placeholders}} differ from English's",
                language.code
            );
            language.strings.each(|field, text| {
                assert!(
                    !text.trim().is_empty(),
                    "{}.rs: `{field}` is empty",
                    language.code
                );
            });
            assert!(language.name.chars().any(char::is_alphabetic));
        }
    }

    #[test]
    fn english_and_spanish_are_there_by_their_files() {
        let codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
        assert!(codes.contains(&"en") && codes.contains(&"es"), "{codes:?}");
        assert_eq!(language("es").map(|l| l.name), Some("Español"));
    }

    #[test]
    fn a_language_is_found_by_its_region_or_its_language() {
        let code = |code| language(code).map(|l| l.code);
        assert_eq!(code("es_CO.UTF-8"), Some("es"));
        assert_eq!(code("es-MX"), Some("es"));
        assert_eq!(code("EN_us"), Some("en"));
        assert_eq!(code("C"), None);
        assert_eq!(code("tlh"), None);
        let system = |vars: &'static [(&'static str, &'static str)]| {
            system_language(|name| {
                (vars.iter())
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            })
            .map(|l| l.code)
        };
        assert_eq!(system(&[("LANG", "es_CO.UTF-8")]), Some("es"));
        assert_eq!(
            system(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "es_CO.UTF-8")]),
            Some("en")
        );
        assert_eq!(system(&[("LC_ALL", ""), ("LANG", "es_ES")]), Some("es"));
        assert_eq!(system(&[("LANG", "POSIX")]), None);
        assert_eq!(system(&[]), None);
    }

    #[test]
    fn templates_are_filled_and_counts_take_their_form() {
        assert_eq!(
            fill("History of {name}", &[("name", &"Shop")]),
            "History of Shop"
        );
        assert_eq!(
            fill("{{host}}/{path} {missing}", &[("path", &"users")]),
            "{{host}}/users {missing}"
        );
        assert_eq!(fill("{n} {n}", &[("n", &2)]), "2 2");
        let es = language("es").map_or(&EN, |l| l.strings);
        assert_eq!(es.count(&es.import_requests, 1, &[]), "1 petición");
        assert_eq!(es.count(&es.import_requests, 3, &[]), "3 peticiones");
        assert_eq!(es.count(&es.import_requests, 0, &[]), "0 peticiones");
        assert_eq!(
            EN.count(&EN.git_committed, 2, &[("id", &"1a2b3c4")]),
            "Committed 2 files: 1a2b3c4"
        );
    }
}
