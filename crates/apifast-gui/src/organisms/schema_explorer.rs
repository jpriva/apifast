//! The schema of a GraphQL request, to explore: its root types, then each
//! type's fields with their arguments and what they return, going from one
//! type to another by its name, and back.

use apifast_domain::{GraphQlSchema, InputValue, SchemaType, TypeKind, TypeRef};
use eframe::egui::{self, RichText};

use crate::strings::Strings;
use crate::theme;

/// Height of the explorer's list, in points.
const LIST_HEIGHT: f32 = 260.0;

/// Where the explorer is: the types opened, the last one shown.
#[derive(Debug, Default)]
pub struct SchemaExplorer {
    path: Vec<String>,
    filter: String,
}

impl SchemaExplorer {
    /// The type shown, if any: the last of the way there.
    pub fn current(&self) -> Option<&str> {
        self.path.last().map(String::as_str)
    }

    /// Draws `schema`.
    pub fn show(&mut self, ui: &mut egui::Ui, schema: &GraphQlSchema, strings: &Strings) {
        // A type gone from a new schema is not shown.
        while self
            .current()
            .is_some_and(|name| schema.type_named(name).is_none())
        {
            self.path.pop();
        }
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("schema-explorer")
            .max_height(LIST_HEIGHT)
            .show(ui, |ui| {
                open = match self.current().and_then(|name| schema.type_named(name)) {
                    Some(shown) => self.type_view(ui, shown, strings),
                    None => self.roots(ui, schema, strings),
                };
            });
        if let Some(name) = open {
            self.path.push(name);
        }
    }

    /// The root types, and every type (filtered by name).
    fn roots(
        &mut self,
        ui: &mut egui::Ui,
        schema: &GraphQlSchema,
        strings: &Strings,
    ) -> Option<String> {
        let mut open = None;
        ui.label(RichText::new(strings.schema_roots).strong());
        for name in [&schema.query, &schema.mutation, &schema.subscription]
            .into_iter()
            .flatten()
        {
            if type_link(ui, name) {
                open = Some(name.clone());
            }
        }
        ui.add_space(theme::space::S);
        ui.label(RichText::new(strings.schema_all_types).strong());
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(strings.schema_filter))
            .accessible_name(strings.schema_filter);
        let filter = self.filter.trim().to_lowercase();
        for shown in schema
            .user_types()
            .filter(|t| filter.is_empty() || t.name.to_lowercase().contains(&filter))
        {
            ui.horizontal(|ui| {
                ui.label(muted(ui, shown.kind.keyword()));
                if type_link(ui, &shown.name) {
                    open = Some(shown.name.clone());
                }
            });
        }
        open
    }

    /// One type: what it is, then its fields, input fields, values or
    /// possible types.
    fn type_view(
        &mut self,
        ui: &mut egui::Ui,
        shown: &SchemaType,
        strings: &Strings,
    ) -> Option<String> {
        let mut open = None;
        ui.horizontal(|ui| {
            if ui.button(strings.schema_back).clicked() {
                self.path.pop();
            }
            ui.label(muted(ui, &self.path.join(" › ")));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(muted(ui, shown.kind.keyword()));
            ui.label(RichText::new(&shown.name).strong().monospace());
            if !shown.interfaces.is_empty() {
                ui.label(muted(ui, strings.schema_implements));
                for name in &shown.interfaces {
                    if type_link(ui, name) {
                        open = Some(name.clone());
                    }
                }
            }
        });
        if let Some(description) = &shown.description {
            ui.label(muted(ui, description));
        }
        ui.add_space(theme::space::S);
        if !shown.fields.is_empty() {
            ui.label(RichText::new(strings.schema_fields).strong());
            for field in &shown.fields {
                ui.horizontal_wrapped(|ui| {
                    let name = RichText::new(&field.name).monospace();
                    ui.label(if field.deprecated {
                        name.strikethrough()
                    } else {
                        name
                    });
                    if !field.args.is_empty() {
                        ui.label(muted(ui, "("));
                        for (i, arg) in field.args.iter().enumerate() {
                            if i > 0 {
                                ui.label(muted(ui, ","));
                            }
                            if argument(ui, arg) {
                                open = Some(arg.type_ref.name().to_owned());
                            }
                        }
                        ui.label(muted(ui, ")"));
                    }
                    ui.label(muted(ui, ":"));
                    if type_ref(ui, &field.type_ref) {
                        open = Some(field.type_ref.name().to_owned());
                    }
                    if field.deprecated {
                        ui.label(muted(ui, strings.schema_deprecated));
                    }
                });
                if let Some(description) = &field.description {
                    ui.label(muted(ui, description).small());
                }
            }
        }
        if !shown.input_fields.is_empty() {
            ui.label(RichText::new(strings.schema_fields).strong());
            for field in &shown.input_fields {
                ui.horizontal_wrapped(|ui| {
                    if argument(ui, field) {
                        open = Some(field.type_ref.name().to_owned());
                    }
                });
            }
        }
        if !shown.enum_values.is_empty() {
            ui.label(RichText::new(strings.schema_values).strong());
            for value in &shown.enum_values {
                let name = RichText::new(&value.name).monospace();
                ui.label(if value.deprecated {
                    name.strikethrough()
                } else {
                    name
                });
            }
        }
        if matches!(shown.kind, TypeKind::Union | TypeKind::Interface)
            && !shown.possible_types.is_empty()
        {
            ui.label(RichText::new(strings.schema_possible).strong());
            for name in &shown.possible_types {
                if type_link(ui, name) {
                    open = Some(name.clone());
                }
            }
        }
        open
    }
}

/// `name: Type = default`; whether its type was clicked.
fn argument(ui: &mut egui::Ui, value: &InputValue) -> bool {
    ui.label(RichText::new(format!("{}:", value.name)).monospace());
    let clicked = type_ref(ui, &value.type_ref);
    if let Some(default) = &value.default {
        ui.label(muted(ui, &format!("= {default}")));
    }
    clicked
}

/// A type reference, its named type a link; whether it was clicked.
fn type_ref(ui: &mut egui::Ui, reference: &TypeRef) -> bool {
    let text = reference.to_string();
    let link = egui::Button::new(
        RichText::new(text)
            .monospace()
            .color(theme::palette(ui).accent),
    )
    .frame(false);
    ui.add(link)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A type's name as a link; whether it was clicked.
fn type_link(ui: &mut egui::Ui, name: &str) -> bool {
    type_ref(ui, &TypeRef::Named(name.to_owned()))
}

fn muted(ui: &egui::Ui, text: &str) -> RichText {
    RichText::new(text).color(theme::palette(ui).text_muted)
}
