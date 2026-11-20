//! Scripts (docs/08, F9): the Scripts tab of a request, and the window
//! with those of a folder or the collection. One editor per kind of script
//! (before the request, after the response, tests), highlighted as
//! JavaScript.

use std::path::PathBuf;

use apifast_domain::{ScriptBlock, ScriptKind};
use eframe::egui::{self, RichText};

use crate::atoms::{CodeEditor, TemplateFieldTexts, VariableHints};
use crate::code::Language;
use crate::strings::Strings;
use crate::theme;

/// The kinds of script a request runs, in the order they run.
pub const ALL_KINDS: [ScriptKind; 3] = [
    ScriptKind::BeforeRequest,
    ScriptKind::AfterResponse,
    ScriptKind::Tests,
];

/// How many kinds of script in `scripts` have code: what a tab counts.
pub fn script_count(scripts: &[ScriptBlock]) -> usize {
    (ALL_KINDS.iter())
        .filter(|kind| apifast_domain::script_of(scripts, kind).is_some())
        .count()
}

fn label<'a>(kind: &ScriptKind, strings: &'a Strings) -> &'a str {
    match kind {
        ScriptKind::BeforeRequest => strings.script_before_request,
        ScriptKind::AfterResponse => strings.script_after_response,
        _ => strings.script_tests_kind,
    }
}

/// The editors of the `kinds` of script in `scripts`, which it edits in
/// place, one shown at a time. `id` keeps which one apart from other
/// editors. Returns whether the user changed something.
pub fn scripts_editor(
    ui: &mut egui::Ui,
    scripts: &mut Vec<ScriptBlock>,
    kinds: &[ScriptKind],
    id: egui::Id,
    strings: &Strings,
) -> bool {
    let shown_id = id.with("kind");
    let mut shown: usize = ui.data(|d| d.get_temp(shown_id)).unwrap_or(0);
    if shown >= kinds.len() {
        shown = 0;
    }
    let muted = theme::palette(ui).text_muted;
    ui.label(RichText::new(strings.scripts_hint).color(muted).small());
    ui.add_space(theme::space::S);
    if kinds.len() > 1 {
        ui.horizontal(|ui| {
            for (index, kind) in kinds.iter().enumerate() {
                let has = apifast_domain::script_of(scripts, kind).is_some();
                let text = if has {
                    format!("{} •", label(kind, strings))
                } else {
                    label(kind, strings).to_owned()
                };
                ui.selectable_value(&mut shown, index, text);
            }
        });
        ui.data_mut(|d| d.insert_temp(shown_id, shown));
    }
    let Some(kind) = kinds.get(shown) else {
        return false;
    };
    // Several scripts of one kind run joined; the first one is edited.
    let more = scripts.iter().filter(|s| s.kind == *kind).count();
    if more > 1 {
        let warning = theme::palette(ui).warning;
        ui.label(RichText::new(strings.scripts_more).color(warning).small());
    }
    let index = scripts.iter().position(|s| s.kind == *kind);
    let mut code = index.map(|i| scripts[i].code.clone()).unwrap_or_default();
    let hints = VariableHints::default();
    let texts = TemplateFieldTexts {
        placeholder: "",
        missing: strings.variable_missing,
        generated: strings.variable_generated,
    };
    let changed = CodeEditor::new(&mut code, Language::JavaScript, &hints, texts)
        .id(id.with(("code", shown)))
        .desired_rows(16)
        .accessible_name(label(kind, strings))
        .show(ui)
        .changed();
    if !changed {
        return false;
    }
    match index {
        // An emptied script goes away, so the file keeps no empty block.
        Some(i) if code.trim().is_empty() && more == 1 => {
            scripts.remove(i);
        }
        Some(i) => scripts[i].code = code,
        None => scripts.push(ScriptBlock::new(kind.clone(), code)),
    }
    true
}

/// Whose scripts the window edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptsTarget {
    /// The collection.
    pub root: PathBuf,
    /// The folder, relative to it; `None` for the collection itself.
    pub folder: Option<String>,
}

/// The window with the scripts of a folder or the collection.
#[derive(Debug, Default)]
pub struct ScriptsWindow {
    /// Whose scripts, how the title names it, and the scripts as edited.
    editing: Option<(ScriptsTarget, String, Vec<ScriptBlock>)>,
}

impl ScriptsWindow {
    /// Opens the window on the scripts of `target`, called `name`.
    pub fn open_for(&mut self, target: ScriptsTarget, name: String, scripts: Vec<ScriptBlock>) {
        self.editing = Some((target, name, scripts));
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.editing.is_some()
    }

    /// Draws the window. Returns whose scripts to save and the scripts,
    /// once the user saves.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        strings: &Strings,
    ) -> Option<(ScriptsTarget, Vec<ScriptBlock>)> {
        let (target, name, scripts) = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        egui::Window::new(format!("{} · {name}", strings.scripts))
            .id(egui::Id::unique("scripts-window"))
            .open(&mut open)
            .default_width(560.0)
            .collapsible(false)
            .show(ctx, |ui| {
                let id = ui.make_persistent_id("scripts-window-editor");
                scripts_editor(ui, scripts, &ALL_KINDS, id, strings);
                ui.add_space(theme::space::S);
                if ui.button(strings.save).clicked() {
                    saved = Some((target.clone(), scripts.clone()));
                }
            });
        if saved.is_some() || !open {
            self.editing = None;
        }
        saved
    }
}
