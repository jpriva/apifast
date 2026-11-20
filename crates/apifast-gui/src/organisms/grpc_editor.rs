//! The body of a gRPC request (docs/09): the method it calls and how it
//! streams, picked from those the server or the `.proto` offers, the
//! `.proto` file that describes it (or none, to ask the server), and its
//! messages, edited as JSON; while the call is open, sending them and
//! ending the client's stream.

use apifast_domain::{GrpcCall, GrpcCatalog, GrpcMessage, GrpcMethodType};
use eframe::egui::{self, RichText};

use super::request_panel::{Connection, code_body, muted};
use crate::atoms::{Completer, VariableHints};
use crate::code::{self, Language};
use crate::core::MethodsView;
use crate::strings::Strings;
use crate::theme;

/// The method types offered, `None` first: not known yet.
const TYPES: [Option<GrpcMethodType>; 5] = [
    None,
    Some(GrpcMethodType::Unary),
    Some(GrpcMethodType::ClientStreaming),
    Some(GrpcMethodType::ServerStreaming),
    Some(GrpcMethodType::BidiStreaming),
];

/// Draws the editor over `call`, offering the methods of `methods`.
/// `(shortcuts, now)` format the message shown, as the body's editor does;
/// `connection` is the call's, and takes what the user asks of it.
/// Returns whether anything changed.
pub fn grpc_call(
    ui: &mut egui::Ui,
    (call, methods): (&mut GrpcCall, Option<&MethodsView>),
    (shortcuts, now): (&[egui::KeyboardShortcut], bool),
    connection: &mut Connection,
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let mut changed = false;
    egui::Grid::new("grpc-call")
        .num_columns(2)
        .spacing([theme::space::M, theme::space::S])
        .show(ui, |ui| {
            ui.label(strings.grpc_method);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut call.method)
                        .hint_text(strings.grpc_method_hint)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name(strings.grpc_method)
                .changed();
            ui.end_row();

            ui.label("");
            changed |= method_picker(ui, call, methods, connection, strings);
            ui.end_row();

            ui.label(strings.grpc_method_type);
            let before = call.method_type;
            egui::ComboBox::from_id_salt("grpc-method-type")
                .selected_text(type_label(call.method_type, strings))
                .show_ui(ui, |ui| {
                    for kind in TYPES {
                        ui.selectable_value(&mut call.method_type, kind, type_label(kind, strings));
                    }
                })
                .response
                .accessible_name(strings.grpc_method_type);
            changed |= call.method_type != before;
            ui.end_row();

            ui.label(strings.grpc_proto);
            let mut path = call.proto_path.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut path)
                        .hint_text(strings.grpc_proto_hint)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name(strings.grpc_proto)
                .changed()
            {
                call.proto_path = (!path.trim().is_empty()).then_some(path);
                changed = true;
            }
            ui.end_row();
        });
    ui.add_space(theme::space::M);
    let streams = call.method_type.is_none_or(GrpcMethodType::client_streams);
    let call_state = (connection, streams);
    // With the method's type known, its messages are completed from it.
    let catalog = methods.and_then(|m| m.catalog.as_deref());
    let input = catalog
        .and_then(|c| c.method(&call.method))
        .map(|m| m.input.clone());
    let schema = catalog.zip(input.as_deref());
    let messages_of = (&mut call.messages, schema);
    changed
        | messages(
            ui,
            messages_of,
            (shortcuts, now),
            call_state,
            hints,
            strings,
        )
}

/// "Load methods", and the methods loaded to pick one: picking it sets the
/// method and how it streams. Returns whether one was picked.
fn method_picker(
    ui: &mut egui::Ui,
    call: &mut GrpcCall,
    methods: Option<&MethodsView>,
    connection: &mut Connection,
    strings: &Strings,
) -> bool {
    let mut picked = false;
    ui.horizontal(|ui| {
        let loading = methods.is_some_and(|m| m.loading);
        if loading {
            ui.spinner();
            ui.label(muted(ui, strings.grpc_loading));
        } else if ui
            .button(strings.grpc_load_methods)
            .on_hover_text(strings.grpc_load_methods_hint)
            .clicked()
        {
            connection.load_methods = true;
        }
        let services = methods
            .and_then(|m| m.catalog.as_deref())
            .map(|c| &c.services);
        if let Some(services) = services.filter(|s| !s.is_empty()) {
            egui::ComboBox::from_id_salt("grpc-method-pick")
                .selected_text(strings.grpc_pick_method)
                .show_ui(ui, |ui| {
                    for method in services.iter().flat_map(|s| &s.methods) {
                        let chosen = call.method.trim() == method.path;
                        let label = RichText::new(&method.path).monospace();
                        if ui.selectable_label(chosen, label).clicked() && !chosen {
                            call.method.clone_from(&method.path);
                            call.method_type = Some(method.method_type);
                            picked = true;
                        }
                    }
                })
                .response
                .accessible_name(strings.grpc_pick_method);
        } else if services.is_some() {
            ui.label(muted(ui, strings.grpc_no_methods));
        }
        if let Some(error) = methods.and_then(|m| m.error.as_deref()) {
            let danger = theme::palette(ui).danger;
            ui.label(RichText::new(error).color(danger).small());
        }
    });
    picked
}

/// "Send" the message shown, and "End stream", while the call is open.
fn stream_buttons(ui: &mut egui::Ui, connection: &mut Connection, shown: usize, strings: &Strings) {
    let open = connection.open;
    if ui
        .add_enabled(open, egui::Button::new(strings.ws_send))
        .on_hover_text(strings.grpc_send_hint)
        .on_disabled_hover_text(strings.grpc_send_closed)
        .clicked()
    {
        connection.send = Some(shown);
    }
    if ui
        .add_enabled(open, egui::Button::new(strings.grpc_end_stream))
        .on_hover_text(strings.grpc_end_stream_hint)
        .on_disabled_hover_text(strings.grpc_send_closed)
        .clicked()
    {
        connection.end = true;
    }
}

fn type_label(kind: Option<GrpcMethodType>, strings: &Strings) -> &str {
    match kind {
        None => strings.grpc_type_unknown,
        Some(GrpcMethodType::Unary) => strings.grpc_unary,
        Some(GrpcMethodType::ClientStreaming) => strings.grpc_client_streaming,
        Some(GrpcMethodType::ServerStreaming) => strings.grpc_server_streaming,
        Some(_) => strings.grpc_bidi_streaming,
    }
}

/// The messages: which one is shown, adding and removing them, and the one
/// shown, its name and its JSON.
fn messages(
    ui: &mut egui::Ui,
    (messages, schema): (&mut Vec<GrpcMessage>, Option<(&GrpcCatalog, &str)>),
    (shortcuts, now): (&[egui::KeyboardShortcut], bool),
    (connection, streams): (&mut Connection, bool),
    hints: &VariableHints,
    strings: &Strings,
) -> bool {
    let editor = ui.make_persistent_id("grpc-message-text");
    let shown_id = editor.with("shown");
    let mut shown: usize = ui.data(|d| d.get_temp(shown_id)).unwrap_or_default();
    shown = shown.min(messages.len().saturating_sub(1));
    let mut changed = false;
    let label = |i: usize, m: &GrpcMessage| {
        if m.name.trim().is_empty() {
            format!("message {}", i + 1)
        } else {
            m.name.clone()
        }
    };
    let mut format = now;
    ui.horizontal(|ui| {
        if !messages.is_empty() {
            egui::ComboBox::from_id_salt("grpc-message-shown")
                .selected_text(label(shown, &messages[shown]))
                .show_ui(ui, |ui| {
                    for (i, message) in messages.iter().enumerate() {
                        ui.selectable_value(&mut shown, i, label(i, message));
                    }
                })
                .response
                .on_hover_text(strings.ws_message_shown);
        }
        if ui.button(strings.ws_add_message).clicked() {
            let name = if messages.is_empty() {
                String::new()
            } else {
                format!("message {}", messages.len() + 1)
            };
            messages.push(GrpcMessage::new(name, "{}"));
            shown = messages.len() - 1;
            changed = true;
        }
        if !messages.is_empty() {
            if ui.button(strings.ws_remove_message).clicked() {
                messages.remove(shown);
                shown = shown.min(messages.len().saturating_sub(1));
                changed = true;
            }
            format |= ui
                .button(strings.format)
                .on_hover_text(strings.format_hint)
                .clicked();
            // A client that streams sends more while the call is open.
            if streams {
                stream_buttons(ui, connection, shown, strings);
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(shown_id, shown));
    connection.shown = shown;
    let Some(message) = messages.get_mut(shown) else {
        ui.label(muted(ui, strings.grpc_no_messages));
        return changed;
    };
    if ui.memory(|m| m.has_focus(editor)) {
        format |= ui.input_mut(|i| {
            let pressed = shortcuts.iter().any(|s| i.consume_shortcut(s));
            if pressed {
                i.events.retain(|e| !matches!(e, egui::Event::Text(_)));
            }
            pressed
        });
    }
    ui.add_space(theme::space::S);
    changed |= ui
        .add(egui::TextEdit::singleline(&mut message.name).hint_text(strings.ws_message_name))
        .accessible_name(strings.ws_message_name)
        .changed();
    let target = (editor, strings.ws_messages);
    let complete = |text: &str, at: usize| {
        schema.map_or_else(code::Completion::default, |(catalog, input)| {
            code::complete_message(text, at, catalog, input)
        })
    };
    let completer = schema.map(|_| &complete as Completer<'_>);
    changed
        | code_body(
            ui,
            &mut message.data,
            (Language::Json, completer),
            target,
            format,
            hints,
            strings,
        )
}
