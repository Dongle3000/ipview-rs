//! The filter side panel.

use std::collections::BTreeSet;

use eframe::egui::{self, Ui};
use serde::{Deserialize, Serialize};

use ipview::Ticket;
use ipview::fields::{self, Field};
use ipview::filter::{Condition, Filter, Op};

use crate::i18n::{self, t};

const OPS: &[(Op, &str)] = &[
    (Op::Eq, "="),
    (Op::Ne, "≠"),
    (Op::Lt, "<"),
    (Op::Le, "≤"),
    (Op::Gt, ">"),
    (Op::Ge, "≥"),
    (Op::Contains, "contains"),
    (Op::NotContains, "not contains"),
];

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DynCondition {
    pub field: u16,
    #[serde(with = "op_serde")]
    pub op: Op,
    pub value: String,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilterState {
    /// Show tickets with MOS at or below this value.
    pub mos_max: String,
    /// Show tickets with packet loss at or above this percentage.
    pub loss_min: String,
    pub equipment: Option<String>,
    pub codec: Option<String>,
    /// Matches local or distant ID.
    pub number: String,
    /// Matches local or distant IP.
    pub ip: String,
    /// Second IP: with both set, only calls between the two.
    pub ip_b: String,
    /// End-of-communication date prefixes, e.g. "2026-09" or "2026-09-14 10".
    pub from: String,
    pub to: String,
    pub jitter_corrupt_only: bool,
    pub conditions: Vec<DynCondition>,
    /// Combine `conditions` with OR instead of AND.
    pub any: bool,
}

/// Distinct values present in the data, for the combo boxes.
#[derive(Default)]
pub struct Choices {
    pub equipment: Vec<String>,
    pub codecs: Vec<String>,
}

impl Choices {
    pub fn from_tickets(tickets: &[Ticket]) -> Choices {
        let distinct = |id: u16| {
            let set: BTreeSet<String> = tickets.iter().filter_map(|t| t.value(id)).map(|v| v.to_string()).collect();
            set.into_iter().collect()
        };
        Choices { equipment: distinct(6), codecs: distinct(15) }
    }
}

/// A compiled filter: invalid numeric inputs are ignored rather than matching nothing.
pub struct Compiled {
    mos_max: Option<f64>,
    loss_min: Option<f64>,
    equipment: Option<String>,
    codec: Option<String>,
    number: String,
    ip: String,
    ip_b: String,
    from: String,
    to: String,
    jitter_corrupt_only: bool,
    dynamic: Filter,
}

impl FilterState {
    pub fn is_empty(&self) -> bool {
        *self == FilterState { any: self.any, ..Default::default() }
    }

    pub fn compile(&self) -> Compiled {
        let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok();
        let conditions = self
            .conditions
            .iter()
            .filter(|c| !c.value.trim().is_empty())
            .filter_map(|c| {
                Some(Condition { field: fields::by_id(c.field)?, op: c.op, value: c.value.trim().to_string() })
            })
            .collect();
        Compiled {
            mos_max: num(&self.mos_max),
            loss_min: num(&self.loss_min),
            equipment: self.equipment.clone(),
            codec: self.codec.clone(),
            number: self.number.trim().to_string(),
            ip: self.ip.trim().to_string(),
            ip_b: self.ip_b.trim().to_string(),
            from: self.from.trim().to_string(),
            to: self.to.trim().to_string(),
            jitter_corrupt_only: self.jitter_corrupt_only,
            dynamic: Filter { conditions, any: self.any },
        }
    }
}

impl Compiled {
    pub fn matches(&self, t: &Ticket) -> bool {
        let text = |id: u16| t.value(id).map(|v| v.to_string()).unwrap_or_default();
        let needs_metrics = self.mos_max.is_some() || self.loss_min.is_some() || self.jitter_corrupt_only;
        if needs_metrics {
            let m = t.metrics();
            if self.mos_max.is_some_and(|max| !m.mos.is_some_and(|mos| mos <= max)) {
                return false;
            }
            if self.loss_min.is_some_and(|min| !m.loss_pct.is_some_and(|l| l >= min)) {
                return false;
            }
            if self.jitter_corrupt_only && !m.jitter_corrupt {
                return false;
            }
        }
        if self.equipment.as_ref().is_some_and(|e| text(6) != *e) || self.codec.as_ref().is_some_and(|c| text(15) != *c)
        {
            return false;
        }
        if !self.number.is_empty() && !text(10).contains(&self.number) && !text(11).contains(&self.number) {
            return false;
        }
        if !self.ip.is_empty() {
            let (local, distant) = (text(8), text(9));
            let ok = if self.ip_b.is_empty() {
                local.contains(&self.ip) || distant.contains(&self.ip)
            } else {
                (local == self.ip && distant == self.ip_b) || (local == self.ip_b && distant == self.ip)
            };
            if !ok {
                return false;
            }
        }
        if !self.from.is_empty() || !self.to.is_empty() {
            let time = text(1);
            if !self.from.is_empty() && time.as_str() < self.from.as_str() {
                return false;
            }
            // `to` is inclusive at its precision: "2026-09" includes all of September.
            if !self.to.is_empty() && time.get(..self.to.len()).unwrap_or(&time) > self.to.as_str() {
                return false;
            }
        }
        self.dynamic.matches(t)
    }
}

/// Draws the filter panel. Returns true if anything changed.
pub fn panel(ui: &mut Ui, state: &mut FilterState, choices: &Choices) -> bool {
    let before = state.clone();

    ui.heading(t("Filters"));
    ui.add_space(4.0);

    section(ui, t("Quality"), |ui| {
        egui::Grid::new("quality").num_columns(2).show(ui, |ui| {
            let l = ui.label(t("MOS ≤"));
            ui.add(egui::TextEdit::singleline(&mut state.mos_max).desired_width(70.0).hint_text(t("e.g. 3.6")))
                .labelled_by(l.id);
            ui.end_row();
            let l = ui.label(t("Loss % ≥"));
            ui.add(egui::TextEdit::singleline(&mut state.loss_min).desired_width(70.0).hint_text(t("e.g. 1")))
                .labelled_by(l.id);
            ui.end_row();
        });
        ui.checkbox(&mut state.jitter_corrupt_only, t("Only implausible jitter counters"));
    });

    section(ui, t("Call"), |ui| {
        egui::Grid::new("call").num_columns(2).show(ui, |ui| {
            let l = ui.label(t("Equipment"));
            choice(ui, "equipment", &mut state.equipment, &choices.equipment).labelled_by(l.id);
            ui.end_row();
            let l = ui.label(t("Codec"));
            choice(ui, "codec", &mut state.codec, &choices.codecs).labelled_by(l.id);
            ui.end_row();
            let l = ui.label(t("Number"));
            ui.add(egui::TextEdit::singleline(&mut state.number).desired_width(130.0).hint_text(t("local or distant")))
                .labelled_by(l.id);
            ui.end_row();
        });
    });

    section(ui, t("Connection"), |ui| {
        egui::Grid::new("conn").num_columns(2).show(ui, |ui| {
            let l = ui.label(t("IP"));
            ui.add(egui::TextEdit::singleline(&mut state.ip).desired_width(130.0).hint_text(t("local or distant")))
                .labelled_by(l.id);
            ui.end_row();
            let l = ui.label(t("and IP"));
            ui.add(egui::TextEdit::singleline(&mut state.ip_b).desired_width(130.0).hint_text(t("calls between both")))
                .labelled_by(l.id);
            ui.end_row();
        });
    });

    section(ui, t("Period (UTC)"), |ui| {
        egui::Grid::new("period").num_columns(2).show(ui, |ui| {
            let l = ui.label(t("From"));
            ui.add(egui::TextEdit::singleline(&mut state.from).desired_width(130.0).hint_text("2026-09-01"))
                .labelled_by(l.id);
            ui.end_row();
            let l = ui.label(t("To"));
            ui.add(egui::TextEdit::singleline(&mut state.to).desired_width(130.0).hint_text("2026-09-30"))
                .labelled_by(l.id);
            ui.end_row();
        });
    });

    section(ui, t("Field conditions"), |ui| {
        let mut remove = None;
        for (i, c) in state.conditions.iter_mut().enumerate() {
            // The number labels the row's widgets for screen readers.
            let number = ui
                .horizontal(|ui| {
                    let n = ui.label(format!("{}.", i + 1));
                    let field = fields::by_id(c.field).map_or("?", i18n::field_name);
                    egui::ComboBox::from_id_salt(("cond_field", i))
                        .width(120.0)
                        .selected_text(field)
                        .show_ui(ui, |ui| {
                            for f in fields::FIELDS {
                                ui.selectable_value(&mut c.field, f.id, i18n::field_name(f));
                            }
                        })
                        .response
                        .labelled_by(n.id);
                    if ui.small_button("×").on_hover_text(t("Remove")).clicked() {
                        remove = Some(i);
                    }
                    n.id
                })
                .inner;
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                let op = OPS.iter().find(|(o, _)| *o == c.op).map_or("?", |(_, s)| t(s));
                egui::ComboBox::from_id_salt(("cond_op", i))
                    .width(70.0)
                    .selected_text(op)
                    .show_ui(ui, |ui| {
                        for (o, s) in OPS {
                            ui.selectable_value(&mut c.op, *o, t(s));
                        }
                    })
                    .response
                    .labelled_by(number);
                ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(100.0)).labelled_by(number);
            });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            state.conditions.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button(t("+ Add condition")).clicked() {
                state.conditions.push(DynCondition { field: 503, op: Op::Lt, value: String::new() });
            }
            if state.conditions.len() > 1 {
                ui.selectable_value(&mut state.any, false, t("AND"));
                ui.selectable_value(&mut state.any, true, t("OR"));
            }
        });
    });

    ui.add_space(8.0);
    if ui.add_enabled(!state.is_empty(), egui::Button::new(t("Clear all filters (F6)"))).clicked() {
        *state = FilterState::default();
    }

    *state != before
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(title).default_open(true).show(ui, add);
}

fn choice(ui: &mut Ui, id: &str, value: &mut Option<String>, options: &[String]) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .width(130.0)
        .selected_text(value.as_deref().unwrap_or(t("any")))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, t("any"));
            for o in options {
                ui.selectable_value(value, Some(o.clone()), o);
            }
        })
        .response
}

/// A condition for "field = value" from a table cell's context menu.
pub fn condition_for(field: &Field, op: Op, value: String) -> DynCondition {
    DynCondition { field: field.id, op, value }
}

mod op_serde {
    use ipview::filter::Op;
    use serde::{Deserialize, Deserializer, Serializer};

    const NAMES: &[(Op, &str)] = &[
        (Op::Eq, "eq"),
        (Op::Ne, "ne"),
        (Op::Lt, "lt"),
        (Op::Le, "le"),
        (Op::Gt, "gt"),
        (Op::Ge, "ge"),
        (Op::Contains, "contains"),
        (Op::NotContains, "not_contains"),
    ];

    pub fn serialize<S: Serializer>(op: &Op, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(NAMES.iter().find(|(o, _)| o == op).map_or("eq", |(_, n)| n))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Op, D::Error> {
        let name = String::deserialize(d)?;
        Ok(NAMES.iter().find(|(_, n)| *n == name).map_or(Op::Eq, |(o, _)| *o))
    }
}
