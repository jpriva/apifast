//! The body of a gRPC request (docs/09): the method it calls and how it
//! streams, the `.proto` file that describes it (or none, to ask the
//! server), and its messages, edited as JSON.

use apifast_domain::{GrpcCall, GrpcMessage, GrpcMethodType};
use eframe::egui;

use super::request_panel::{code_body, muted};
use crate::atoms::VariableHints;
use crate::code::Language;
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

/// Draws the editor over `call`. `(shortcuts, now)` format the message
/// shown, as the body's editor does. Returns whether anything changed.
pub fn grpc_call(
    ui: &mut egui::Ui,
    call: &mut GrpcCall,
    (shortcuts, now): (&[egui::KeyboardShortcut], bool),
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
    changed | messages(ui, &mut call.messages, (shortcuts, now), hints, strings)
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
    messages: &mut Vec<GrpcMessage>,
    (shortcuts, now): (&[egui::KeyboardShortcut], bool),
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
        }
    });
    ui.data_mut(|d| d.insert_temp(shown_id, shown));
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
    changed
        | code_body(
            ui,
            &mut message.data,
            (Language::Json, None),
            target,
            format,
            hints,
            strings,
        )
}
