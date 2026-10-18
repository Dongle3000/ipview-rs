//! Detail view of a single ticket.

use eframe::egui::{self, Color32, RichText, Ui};

use ipview::fields::{self, Field};
use ipview::ticket::{Ticket, Value};

use crate::i18n::{self, t, tf};
use crate::style;

pub fn show(ui: &mut Ui, ticket: &Ticket) {
    let title = ticket.value(1).map(|v| format!("{v} UTC")).unwrap_or_else(|| t("(no timestamp)").into());
    ui.heading(title);
    let m = ticket.metrics();
    ui.horizontal(|ui| {
        if let Some(mos) = m.mos {
            ui.label(
                RichText::new(format!("MOS {mos:.2}"))
                    .strong()
                    .color(Color32::BLACK)
                    .background_color(style::mos_color(mos)),
            );
        } else {
            ui.label(RichText::new(t("no MOS")).weak());
        }
        if let Some(loss) = m.loss_pct {
            ui.label(tf("loss {loss} %", &[("loss", &format!("{loss:.2}"))]));
        }
        if m.jitter_corrupt {
            ui.label(RichText::new(t("! jitter counters implausible")).color(ui.visuals().warn_fg_color)).on_hover_text(
                t("Field 32 holds filler instead of counts. Jitter delay is not shown and MOS uses the RTP packet loss."),
            );
        }
    });
    ui.separator();

    let mut ordered: Vec<&Field> = fields::FIELDS.iter().filter(|f| f.id != 1).collect();
    ordered.sort_by_key(|f| f.group);

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let mut i = 0;
        while i < ordered.len() {
            let group = ordered[i].group;
            let members: Vec<&Field> = ordered[i..].iter().take_while(|f| f.group == group).copied().collect();
            i += members.len();
            let present: Vec<(&Field, Value)> =
                members.into_iter().filter_map(|f| ticket.value(f.id).map(|v| (f, v))).collect();
            if present.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(i18n::group_title(group))
                .id_salt(("group", group as u8))
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(("grid", group as u8)).num_columns(2).striped(true).show(ui, |ui| {
                        for (f, v) in &present {
                            ui.label(i18n::field_name(f))
                                .on_hover_text(tf("field {id} · {key}", &[("id", &f.id), ("key", &f.key)]));
                            match v {
                                Value::Buckets(counts) => buckets(ui, f, counts),
                                v => {
                                    ui.label(match f.unit {
                                        Some(u) => format!("{} {u}", i18n::value_text(v)),
                                        None => i18n::value_text(v),
                                    });
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
        }

        let unknown: Vec<String> = ticket
            .field_ids()
            .filter(|&id| fields::by_id(id).is_none())
            .map(|id| format!("{id} = {}", Value::Raw(ticket.raw_field(id).unwrap_or_default().to_vec())))
            .collect();
        if !unknown.is_empty() {
            egui::CollapsingHeader::new(t("Unknown fields")).id_salt("unknown_fields").default_open(false).show(
                ui,
                |ui| {
                    for u in unknown {
                        ui.monospace(u);
                    }
                },
            );
        }
    });
}

/// Small horizontal bar chart of a distribution, one bar per bucket.
fn buckets(ui: &mut Ui, f: &Field, counts: &[u32]) {
    let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    ui.vertical(|ui| {
        if total == 0 {
            ui.label(RichText::new(t("all zero")).weak());
            return;
        }
        let bar_color = ui.visuals().selection.bg_fill;
        for (i, &c) in counts.iter().enumerate() {
            ui.horizontal(|ui| {
                let label = f.buckets.get(i).copied().unwrap_or("?");
                let label = match f.unit {
                    Some(u) => format!("{label} {u}"),
                    None => label.to_string(),
                };
                ui.add_sized([70.0, 14.0], egui::Label::new(RichText::new(label).small()));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                if c > 0 {
                    let mut bar = rect;
                    bar.set_width((rect.width() * c as f32 / max).max(1.5));
                    ui.painter().rect_filled(bar, 2.0, bar_color);
                }
                ui.label(RichText::new(c.to_string()).small());
            });
        }
    });
}
