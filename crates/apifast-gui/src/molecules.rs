//! Molecules: small groups of atoms and egui widgets with one purpose.

use eframe::egui;

/// Picks the active environment, or none.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentSelector<'a> {
    environments: &'a [String],
    active: Option<&'a str>,
    none_label: &'a str,
    label: &'a str,
}

impl<'a> EnvironmentSelector<'a> {
    /// A selector over `environments` with `active` chosen. `none_label` is
    /// the entry for "no environment" and `label` the accessible name.
    pub fn new(
        environments: &'a [String],
        active: Option<&'a str>,
        none_label: &'a str,
        label: &'a str,
    ) -> Self {
        Self {
            environments,
            active,
            none_label,
            label,
        }
    }

    /// Draws the selector. Returns the new choice when the user changes it.
    pub fn show(self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let mut choice = None;
        let response = egui::ComboBox::from_id_salt("environment")
            .selected_text(self.active.unwrap_or(self.none_label))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.active.is_none(), self.none_label)
                    .clicked()
                {
                    choice = Some(None);
                }
                for name in self.environments {
                    if ui
                        .selectable_label(self.active == Some(name.as_str()), name)
                        .clicked()
                    {
                        choice = Some(Some(name.clone()));
                    }
                }
            })
            .response;
        response.on_hover_text(self.label);
        // Picking the active entry again is not a change.
        choice.filter(|new| new.as_deref() != self.active)
    }
}
