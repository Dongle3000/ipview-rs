//! Column picker and the column header context menu.

use eframe::egui::{self, Key, RichText, Ui};

use ipview::fields::{self, Field, Group};
use ipview::filter::Op;

use crate::filters::DynCondition;

const FILTER_OPS: &[(Op, &str)] = &[
    (Op::Eq, "="),
    (Op::Ne, "≠"),
    (Op::Lt, "<"),
    (Op::Le, "≤"),
    (Op::Gt, ">"),
    (Op::Ge, "≥"),
    (Op::Contains, "contains"),
];

/// What the user chose in a header context menu.
pub enum HeaderAction {
    Sort { field: u16, descending: bool },
    ClearSort,
    AddFilter(DynCondition),
    RemoveFilters(u16),
    Move { field: u16, by: isize },
    Hide(u16),
}

/// Filter being typed in a header menu; kept across frames while the menu is open.
#[derive(Default)]
pub struct HeaderFilterDraft {
    field: u16,
    op: Option<Op>,
    value: String,
    /// Whether a header menu was shown last frame; reset by [`HeaderFilterDraft::frame_end`].
    open: bool,
    shown_this_frame: bool,
}

impl HeaderFilterDraft {
    /// Call once per frame after the table, so a re-opened menu starts fresh.
    pub fn frame_end(&mut self) {
        self.open = std::mem::take(&mut self.shown_this_frame);
    }
}

pub struct HeaderMenu<'a> {
    pub field: &'static Field,
    pub sort: Option<(u16, bool)>,
    pub position: usize,
    pub column_count: usize,
    pub filters_on_field: usize,
    pub draft: &'a mut HeaderFilterDraft,
    pub columns: &'a mut Vec<u16>,
}

impl HeaderMenu<'_> {
    pub fn show(self, ui: &mut Ui) -> Option<HeaderAction> {
        let f = self.field;
        let mut action = None;
        ui.set_min_width(230.0);
        ui.label(RichText::new(f.name).strong());
        ui.separator();

        let current = self.sort.filter(|(id, _)| *id == f.id).map(|(_, d)| d);
        if ui.add_enabled(current != Some(false), egui::Button::new("Sort ascending")).clicked() {
            action = Some(HeaderAction::Sort { field: f.id, descending: false });
        }
        if ui.add_enabled(current != Some(true), egui::Button::new("Sort descending")).clicked() {
            action = Some(HeaderAction::Sort { field: f.id, descending: true });
        }
        if ui.add_enabled(self.sort.is_some(), egui::Button::new("Clear sorting")).clicked() {
            action = Some(HeaderAction::ClearSort);
        }
        ui.separator();

        // Inline filter editor: operator buttons, value, Enter or "Add filter".
        let draft = self.draft;
        let opened = draft.field != f.id || !draft.open;
        if opened {
            *draft = HeaderFilterDraft { field: f.id, open: true, ..Default::default() };
        }
        draft.shown_this_frame = true;
        let op = *draft.op.get_or_insert(default_op(f));
        let filter_label = ui.label("Filter");
        let mut refocus = false;
        ui.horizontal_wrapped(|ui| {
            for (o, label) in FILTER_OPS {
                if ui.selectable_label(op == *o, *label).clicked() {
                    draft.op = Some(*o);
                    refocus = true;
                }
            }
        });
        let mut submit = false;
        ui.horizontal(|ui| {
            let edit = ui
                .add(egui::TextEdit::singleline(&mut draft.value).desired_width(140.0).hint_text(hint(f)))
                .labelled_by(filter_label.id);
            // Keep the value focused (also after picking an operator) so one can type and press Enter.
            if opened || refocus {
                edit.request_focus();
            }
            submit |= edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            submit |= ui.add_enabled(!draft.value.trim().is_empty(), egui::Button::new("Add filter")).clicked();
        });
        if submit && !draft.value.trim().is_empty() {
            action = Some(HeaderAction::AddFilter(DynCondition {
                field: f.id,
                op: draft.op.unwrap_or(op),
                value: draft.value.trim().to_string(),
            }));
            draft.value.clear();
        }
        if self.filters_on_field > 0 {
            let label = format!("Remove filters on this column ({})", self.filters_on_field);
            if ui.button(label).clicked() {
                action = Some(HeaderAction::RemoveFilters(f.id));
            }
        }
        ui.separator();

        ui.horizontal(|ui| {
            if ui.add_enabled(self.position > 0, egui::Button::new("Move left")).clicked() {
                action = Some(HeaderAction::Move { field: f.id, by: -1 });
            }
            if ui.add_enabled(self.position + 1 < self.column_count, egui::Button::new("Move right")).clicked() {
                action = Some(HeaderAction::Move { field: f.id, by: 1 });
            }
        });
        if ui.add_enabled(self.column_count > 1, egui::Button::new("Hide column")).clicked() {
            action = Some(HeaderAction::Hide(f.id));
        }
        picker_button(ui, self.columns);

        if action.is_some() {
            ui.close();
        }
        action
    }
}

/// "Columns" menu button with the picker; stays open while ticking boxes.
pub fn picker_button(ui: &mut Ui, columns: &mut Vec<u16>) {
    let config =
        egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new("Columns").config(config).ui(ui, |ui| picker(ui, columns));
}

/// Checkbox list of all fields, grouped like the detail view. Returns true on change.
pub fn picker(ui: &mut Ui, columns: &mut Vec<u16>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button("Defaults").clicked() {
            *columns = fields::DEFAULT_COLUMNS.to_vec();
            changed = true;
        }
        if ui.button("Quality set").on_hover_text("End time, IPs, equipment, codec, loss, delays, MOS").clicked() {
            *columns = vec![1, 8, 9, 6, 15, 12, 24, 500, 501, 502, 503];
            changed = true;
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        let mut groups: Vec<Group> = fields::FIELDS.iter().map(|f| f.group).collect();
        groups.dedup();
        groups.sort();
        groups.dedup();
        for g in groups {
            ui.label(RichText::new(crate::detail::group_title(g)).strong().small());
            for f in fields::FIELDS.iter().filter(|f| f.group == g) {
                let mut shown = columns.contains(&f.id);
                let r = ui.checkbox(&mut shown, f.name).on_hover_text(format!("field {} · {}", f.id, f.key));
                if r.changed() {
                    if shown {
                        columns.push(f.id);
                    } else if columns.len() > 1 {
                        columns.retain(|&c| c != f.id);
                    }
                    changed = true;
                }
            }
        }
    });
    changed
}

fn default_op(f: &Field) -> Op {
    match f.id {
        503 | 504 => Op::Lt,
        500..=502 => Op::Gt,
        8 | 9 | 10 | 11 | 39 => Op::Contains,
        _ => Op::Eq,
    }
}

fn hint(f: &Field) -> &'static str {
    match f.id {
        1 => "2026-09-14",
        503 => "3.6",
        500 => "1",
        8 | 9 => "192.168.",
        6 => "NOE-IP",
        15 => "G711A",
        _ => "value",
    }
}
