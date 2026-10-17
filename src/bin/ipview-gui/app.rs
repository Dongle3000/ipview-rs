use std::path::PathBuf;

use eframe::egui::{self, Color32, Key, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};

use ipview::export;
use ipview::fields::{self, Field};
use ipview::filter::Op;
use ipview::stats::{MOS_BANDS, Stats, Summary};
use ipview::ticket::{self, Ticket, Value};

use crate::filters::{self, Choices, FilterState};
use crate::loader::Loader;
use crate::{detail, style};

const SETTINGS_KEY: &str = "ipview-settings";

/// Preferences kept between sessions.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub columns: Vec<u16>,
    pub sort: Option<(u16, bool)>,
    pub filters: FilterState,
    pub last_dir: Option<PathBuf>,
    pub show_detail: bool,
    pub csv_delimiter: char,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            columns: fields::DEFAULT_COLUMNS.to_vec(),
            sort: None,
            filters: FilterState::default(),
            last_dir: None,
            show_detail: true,
            csv_delimiter: ';',
        }
    }
}

pub struct App {
    settings: Settings,
    tickets: Vec<Ticket>,
    /// Indices into `tickets` after filtering and sorting.
    visible: Vec<usize>,
    selected: Option<usize>,
    choices: Choices,
    loader: Option<Loader>,
    status: String,
    errors: Vec<String>,
    show_errors: bool,
    show_stats: bool,
    show_columns: bool,
    show_about: bool,
    stats: Option<Stats>,
    /// Scroll the table to the selection on the next frame.
    scroll_to_selected: bool,
    screenshot: Option<crate::Screenshot>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>, screenshot: Option<crate::Screenshot>) -> App {
        style::apply(&cc.egui_ctx);
        let settings = if let Some(shot) = &screenshot {
            let mut s = Settings::default();
            s.filters.conditions = shot.conditions.clone();
            s.sort = shot.sort;
            s
        } else {
            cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default()
        };
        let mut app = App {
            settings,
            tickets: Vec::new(),
            visible: Vec::new(),
            selected: None,
            choices: Choices::default(),
            loader: None,
            status: "Open ticket files (IP*.DAT, *.ipv) with File > Open, F3, or drag and drop.".into(),
            errors: Vec::new(),
            show_errors: false,
            show_stats: false,
            show_columns: false,
            show_about: false,
            stats: None,
            scroll_to_selected: false,
            screenshot,
        };
        if !paths.is_empty() {
            app.load(paths, true, &cc.egui_ctx);
        }
        app
    }

    fn load(&mut self, paths: Vec<PathBuf>, replace: bool, ctx: &egui::Context) {
        if let Some(dir) =
            paths.first().and_then(|p| if p.is_dir() { Some(p.clone()) } else { p.parent().map(Into::into) })
        {
            self.settings.last_dir = Some(dir);
        }
        self.loader = Some(Loader::start(paths, replace, ctx.clone()));
    }

    fn poll_loader(&mut self) {
        let Some(loaded) = self.loader.as_ref().and_then(Loader::poll) else { return };
        let replace = self.loader.take().is_some_and(|l| l.replace);
        if replace {
            self.tickets = loaded.tickets;
            self.selected = None;
        } else {
            self.tickets.extend(loaded.tickets);
        }
        self.errors = loaded.errors;
        self.show_errors = !self.errors.is_empty();
        self.status = format!(
            "{}Loaded {} files{}.",
            if loaded.cancelled { "Cancelled. " } else { "" },
            loaded.files,
            if self.errors.is_empty() { String::new() } else { format!(", {} could not be read", self.errors.len()) }
        );
        self.choices = Choices::from_tickets(&self.tickets);
        self.refresh();
    }

    /// Re-applies filter and sort.
    fn refresh(&mut self) {
        let filter = self.settings.filters.compile();
        self.visible = (0..self.tickets.len()).filter(|&i| filter.matches(&self.tickets[i])).collect();
        if let Some((id, descending)) = self.settings.sort {
            let keys: Vec<Option<Value>> = self.visible.iter().map(|&i| self.tickets[i].value(id)).collect();
            let mut order: Vec<usize> = (0..self.visible.len()).collect();
            order.sort_by(|&a, &b| {
                let ord = export::cmp_values(keys[a].as_ref(), keys[b].as_ref());
                // Missing values stay last in both directions.
                if descending && keys[a].is_some() && keys[b].is_some() { ord.reverse() } else { ord }
            });
            self.visible = order.into_iter().map(|k| self.visible[k]).collect();
        }
        if self.selected.is_some_and(|s| !self.visible.contains(&s)) {
            self.selected = None;
        }
        if self.show_stats {
            self.stats = Some(Stats::compute(self.visible.iter().map(|&i| &self.tickets[i])));
        } else {
            self.stats = None;
        }
    }

    fn visible_tickets(&self) -> impl Iterator<Item = &Ticket> {
        self.visible.iter().map(|&i| &self.tickets[i])
    }

    fn dialog(&self) -> rfd::FileDialog {
        let d = rfd::FileDialog::new();
        match &self.settings.last_dir {
            Some(dir) => d.set_directory(dir),
            None => d,
        }
    }

    fn open_files(&mut self, replace: bool, ctx: &egui::Context) {
        if let Some(paths) = self
            .dialog()
            .add_filter("Ticket files", &["DAT", "dat", "ipv", "IPV"])
            .add_filter("All files", &["*"])
            .pick_files()
        {
            self.load(paths, replace, ctx);
        }
    }

    fn open_folder(&mut self, replace: bool, ctx: &egui::Context) {
        if let Some(dir) = self.dialog().pick_folder() {
            self.load(vec![dir], replace, ctx);
        }
    }

    fn save_ipv(&mut self) {
        let Some(path) = self.dialog().add_filter("IPView file", &["ipv"]).set_file_name("tickets.ipv").save_file()
        else {
            return;
        };
        let tickets: Vec<&Ticket> = self.visible_tickets().collect();
        self.status = match ipview::file::write_ipv(&path, tickets) {
            Ok(n) => format!("Saved {n} tickets to {}.", path.display()),
            Err(e) => format!("Saving failed: {e:#}"),
        };
    }

    fn export(&mut self, json: bool) {
        let (name, ext) = if json { ("JSON", "json") } else { ("CSV", "csv") };
        let Some(path) = self.dialog().add_filter(name, &[ext]).set_file_name(format!("tickets.{ext}")).save_file()
        else {
            return;
        };
        let result = std::fs::File::create(&path).map_err(anyhow::Error::from).and_then(|f| {
            let w = std::io::BufWriter::new(f);
            if json {
                export::json(w, self.visible_tickets())
            } else {
                export::csv(w, self.visible_tickets(), self.settings.csv_delimiter)
            }
        });
        self.status = match result {
            Ok(()) => format!("Exported {} tickets to {}.", self.visible.len(), path.display()),
            Err(e) => format!("Export failed: {e:#}"),
        };
    }

    fn clear_database(&mut self) {
        self.tickets.clear();
        self.choices = Choices::default();
        self.selected = None;
        self.refresh();
        self.status = "Database cleared.".into();
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() && self.loader.is_none() {
            // Dropping adds to what is loaded; use File > Open to replace.
            self.load(dropped, false, ctx);
        }

        let typing = ctx.text_edit_focused();
        let (f3, f6, f8, open, up, down) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::F3),
                i.consume_key(egui::Modifiers::NONE, Key::F6),
                i.consume_key(egui::Modifiers::NONE, Key::F8),
                i.consume_key(egui::Modifiers::COMMAND, Key::O),
                !typing && i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                !typing && i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            )
        });
        if (f3 || open) && self.loader.is_none() {
            self.open_files(true, ctx);
        }
        if f6 {
            self.settings.filters = FilterState::default();
            self.refresh();
        }
        if f8 {
            self.clear_database();
        }
        if (up || down) && !self.visible.is_empty() {
            let pos = self.selected.and_then(|s| self.visible.iter().position(|&v| v == s));
            let next = match (pos, down) {
                (None, _) => 0,
                (Some(p), true) => (p + 1).min(self.visible.len() - 1),
                (Some(p), false) => p.saturating_sub(1),
            };
            self.selected = Some(self.visible[next]);
            self.scroll_to_selected = true;
        }
    }

    fn menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            let idle = self.loader.is_none();
            ui.menu_button("File", |ui| {
                if ui.add_enabled(idle, egui::Button::new("Open files…").shortcut_text("F3")).clicked() {
                    self.open_files(true, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new("Open folder…")).clicked() {
                    self.open_folder(true, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new("Add files…")).clicked() {
                    self.open_files(false, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new("Add folder…")).clicked() {
                    self.open_folder(false, &ctx);
                }
                ui.separator();
                let any = !self.visible.is_empty();
                if ui.add_enabled(any, egui::Button::new("Save visible tickets as .ipv…")).clicked() {
                    self.save_ipv();
                }
                if ui.add_enabled(any, egui::Button::new("Export visible tickets to CSV…")).clicked() {
                    self.export(false);
                }
                if ui.add_enabled(any, egui::Button::new("Export visible tickets to JSON…")).clicked() {
                    self.export(true);
                }
                ui.menu_button("CSV delimiter", |ui| {
                    for (d, label) in [(';', "Semicolon (Excel, European)"), (',', "Comma"), ('\t', "Tab")] {
                        ui.radio_value(&mut self.settings.csv_delimiter, d, label);
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(!self.tickets.is_empty(), egui::Button::new("Clear database").shortcut_text("F8"))
                    .clicked()
                {
                    self.clear_database();
                }
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Columns…").clicked() {
                    self.show_columns = true;
                }
                if ui.checkbox(&mut self.show_stats, "Statistics").changed() {
                    self.refresh();
                }
                ui.checkbox(&mut self.settings.show_detail, "Ticket details");
                if !self.errors.is_empty() {
                    ui.checkbox(&mut self.show_errors, "Load errors");
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.show_about = true;
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(loader) = &self.loader {
                let done = loader.done();
                let frac = if loader.total == 0 { 0.0 } else { done as f32 / loader.total as f32 };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(220.0)
                        .text(format!("Loading {done} / {} files", loader.total)),
                );
                if ui.button("Cancel").clicked() {
                    loader.cancel();
                }
            } else {
                ui.label(&self.status);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("Tickets in database: {}   Visible: {}", self.tickets.len(), self.visible.len()));
            });
        });
    }

    fn table(&mut self, ui: &mut Ui) {
        if self.tickets.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No tickets loaded.\nFile > Open (F3) or drop IP*.DAT files here.").size(16.0).weak(),
                );
            });
            return;
        }

        let columns: Vec<&'static Field> = self.settings.columns.iter().filter_map(|&id| fields::by_id(id)).collect();
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;

        // Preferred widths, scaled down so all columns fit. Column widths are kept per
        // column set and (coarse) available width, so they refit when the window changes.
        let preferred: Vec<f32> = columns.iter().map(|f| preferred_width(f)).collect();
        let spacing = ui.spacing().item_spacing.x * columns.len() as f32;
        let scroll_bar = ui.spacing().scroll.allocated_width();
        // Small margin for the resize handles, so the scroll bar is not pushed out of view.
        let available = (ui.available_width() - spacing - scroll_bar - 12.0).max(100.0);
        let scale = (available / preferred.iter().sum::<f32>()).min(1.0);
        let width_key = (ui.available_width() / 40.0) as i32;

        let mut table = TableBuilder::new(ui)
            .id_salt(("tickets", &self.settings.columns, width_key))
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .min_scrolled_height(0.0)
            // Fill the panel so the scroll bar sits at its right edge at full width.
            .auto_shrink(false);
        for w in &preferred {
            table = table.column(Column::initial((w * scale).max(36.0)).at_least(30.0).clip(true));
        }
        if self.scroll_to_selected {
            if let Some(pos) = self.selected.and_then(|s| self.visible.iter().position(|&v| v == s)) {
                table = table.scroll_to_row(pos, None);
            }
            self.scroll_to_selected = false;
        }

        let mut clicked_sort = None;
        let mut new_condition = None;
        let mut clicked_row = None;
        table
            .header(row_height + 4.0, |mut header| {
                for f in &columns {
                    header.col(|ui| {
                        let sorted = self.settings.sort.filter(|(id, _)| *id == f.id).map(|(_, desc)| desc);
                        let label = match f.unit {
                            Some(u) => format!("{} [{u}]", short_name(f)),
                            None => short_name(f).to_string(),
                        };
                        if let Some(descending) = sorted {
                            sort_arrow(ui, descending);
                        }
                        let r = ui
                            .add(egui::Label::new(RichText::new(label).strong()).sense(egui::Sense::click()))
                            .on_hover_text(format!("{} (field {}). Click to sort.", f.name, f.id));
                        if r.clicked() {
                            clicked_sort = Some(f.id);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, self.visible.len(), |mut row| {
                    let index = self.visible[row.index()];
                    let t = &self.tickets[index];
                    row.set_selected(self.selected == Some(index));
                    let metrics = t.metrics();
                    for f in &columns {
                        row.col(|ui| {
                            let value = t.value(f.id);
                            let text = value.as_ref().map(|v| v.to_string()).unwrap_or_default();
                            let fill = match f.id {
                                503 => metrics.mos.map(style::mos_color),
                                500 => metrics.loss_pct.and_then(style::loss_color),
                                _ => None,
                            };
                            let label = match fill {
                                Some(c) => {
                                    ui.painter().rect_filled(ui.max_rect().expand2(egui::vec2(2.0, 1.0)), 0.0, c);
                                    RichText::new(&text).color(Color32::BLACK)
                                }
                                None => RichText::new(&text),
                            };
                            let r = ui.add(egui::Label::new(label).selectable(false));
                            if value.is_some() {
                                r.context_menu(|ui| {
                                    ui.label(RichText::new(f.name).strong());
                                    for (op, verb) in [(Op::Eq, "="), (Op::Ne, "≠"), (Op::Lt, "<"), (Op::Gt, ">")] {
                                        if ui.button(format!("Filter {verb} {text}")).clicked() {
                                            new_condition = Some(filters::condition_for(f, op, text.clone()));
                                        }
                                    }
                                    if ui.button("Copy value").clicked() {
                                        ui.ctx().copy_text(text.clone());
                                    }
                                });
                            }
                        });
                    }
                    if row.response().clicked() {
                        clicked_row = Some(index);
                    }
                });
            });

        if let Some(index) = clicked_row {
            self.selected = Some(index);
        }
        if let Some(id) = clicked_sort {
            self.settings.sort = match self.settings.sort {
                Some((cur, false)) if cur == id => Some((id, true)),
                Some((cur, true)) if cur == id => None,
                _ => Some((id, false)),
            };
            self.refresh();
        }
        if let Some(c) = new_condition {
            self.settings.filters.conditions.push(c);
            self.refresh();
        }
    }

    fn columns_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_columns;
        egui::Window::new("Columns").open(&mut open).default_width(320.0).show(ctx, |ui| {
            ui.label("Shown columns, in order:");
            let mut action = None;
            for (i, id) in self.settings.columns.iter().enumerate() {
                let name = fields::by_id(*id).map_or("?", |f| f.name);
                ui.horizontal(|ui| {
                    if ui.small_button("Up").clicked() && i > 0 {
                        action = Some((i, -1));
                    }
                    if ui.small_button("Down").clicked() {
                        action = Some((i, 1));
                    }
                    if ui.small_button("×").clicked() {
                        action = Some((i, 0));
                    }
                    ui.label(name);
                });
            }
            if let Some((i, dir)) = action {
                let cols = &mut self.settings.columns;
                match dir {
                    0 => {
                        cols.remove(i);
                    }
                    -1 => cols.swap(i, i - 1),
                    _ if i + 1 < cols.len() => cols.swap(i, i + 1),
                    _ => {}
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let mut add = None;
                egui::ComboBox::from_id_salt("add_column").selected_text("Add column…").width(220.0).show_ui(
                    ui,
                    |ui| {
                        for f in fields::FIELDS.iter().filter(|f| !self.settings.columns.contains(&f.id)) {
                            if ui.selectable_label(false, f.name).clicked() {
                                add = Some(f.id);
                            }
                        }
                    },
                );
                self.settings.columns.extend(add);
                if ui.button("Defaults").clicked() {
                    self.settings.columns = fields::DEFAULT_COLUMNS.to_vec();
                }
            });
        });
        self.show_columns = open;
    }

    fn stats_window(&mut self, ctx: &egui::Context) {
        if !self.show_stats {
            return;
        }
        if self.stats.is_none() {
            self.stats = Some(Stats::compute(self.visible_tickets()));
        }
        let mut open = true;
        let stats = self.stats.as_ref().unwrap();
        egui::Window::new("Statistics (visible tickets)")
            .open(&mut open)
            .default_width(460.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(format!("Tickets: {}", stats.tickets));
                if let Some((a, b)) = stats.period {
                    ui.label(format!("Period: {} – {} UTC", ticket::format_timestamp(a), ticket::format_timestamp(b)));
                }
                ui.add_space(6.0);
                egui::Grid::new("summary").striped(true).num_columns(7).show(ui, |ui| {
                    for h in ["", "mean", "min", "p5", "median", "p95", "max"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    let row = |ui: &mut Ui, name: &str, s: Option<Summary>| {
                        if let Some(s) = s {
                            ui.label(name);
                            for v in [s.mean, s.min, s.p5, s.median, s.p95, s.max] {
                                ui.label(format!("{v:.2}"));
                            }
                            ui.end_row();
                        }
                    };
                    row(ui, "MOS", stats.mos);
                    row(ui, "Loss %", stats.loss_pct);
                    row(ui, "Round-trip ms", stats.rtd);
                    row(ui, "Jitter ms", stats.jitter_delay);
                });

                if let Some(mos) = stats.mos {
                    ui.add_space(8.0);
                    ui.strong("MOS bands");
                    for ((lo, _, label), n) in MOS_BANDS.iter().zip(stats.mos_bands) {
                        ui.horizontal(|ui| {
                            ui.add_sized([70.0, 14.0], egui::Label::new(*label));
                            let frac = n as f32 / mos.count as f32;
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(200.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                            let mut bar = rect;
                            bar.set_width(rect.width() * frac);
                            let color = style::mos_color(if lo.is_finite() { *lo } else { 0.0 });
                            ui.painter().rect_filled(bar, 2.0, color);
                            ui.label(format!("{n}  ({:.1} %)", 100.0 * frac));
                        });
                    }
                }

                ui.add_space(8.0);
                ui.columns(2, |cols| {
                    for (ui, (title, counts)) in
                        cols.iter_mut().zip([("Codecs", &stats.codecs), ("Equipment", &stats.equipment)])
                    {
                        ui.strong(title);
                        egui::Grid::new(title).show(ui, |ui| {
                            for (k, n) in counts {
                                ui.label(k);
                                ui.label(n.to_string());
                                ui.end_row();
                            }
                        });
                    }
                });
            });
        if !open {
            self.show_stats = false;
            self.stats = None;
        }
    }

    fn misc_windows(&mut self, ctx: &egui::Context) {
        egui::Window::new("Load errors").open(&mut self.show_errors).default_width(500.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for e in &self.errors {
                    ui.label(e);
                }
            });
        });
        egui::Window::new("About").open(&mut self.show_about).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading(format!("IPView (Rust) {}", env!("CARGO_PKG_VERSION")));
            ui.label("Analyzer for Alcatel-Lucent OmniPCX Enterprise VoIP tickets (IP*.DAT, .ipv).");
            ui.label("A rewrite of IPView 2.0.1 by Sven Fabricius (2007).");
            ui.add_space(6.0);
            ui.label("MOS: ITU-T G.107 E-model (narrowband), G.107.1 (wideband, G.722).");
            ui.label("Times are shown in UTC.");
        });
    }
}

fn preferred_width(f: &Field) -> f32 {
    match f.id {
        1 => 135.0,
        8 | 9 => 105.0,
        10 | 11 => 95.0,
        6 => 110.0,
        _ => 62.0,
    }
}

/// Small triangle before a header label; the default fonts have no arrow glyphs.
fn sort_arrow(ui: &mut Ui, descending: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let c = rect.center();
    let (tip, base) = if descending { (c.y + 3.5, c.y - 3.0) } else { (c.y - 3.5, c.y + 3.0) };
    let points = vec![egui::pos2(c.x - 4.0, base), egui::pos2(c.x + 4.0, base), egui::pos2(c.x, tip)];
    ui.painter().add(egui::Shape::convex_polygon(points, ui.visuals().strong_text_color(), egui::Stroke::NONE));
}

/// Column header text: the field name without long prefixes.
fn short_name(f: &Field) -> &'static str {
    match f.id {
        1 => "End time",
        12 => "Dur.",
        22 => "RTP rx",
        23 => "RTP tx",
        24 => "RTP lost",
        500 => "Loss",
        501 => "Round-trip",
        502 => "Jitter",
        504 => "R (WB)",
        505 => "Jitter corrupt",
        _ => f.name,
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loader();
        self.handle_input(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        let mut filters_changed = false;
        egui::Panel::left("filters").resizable(true).default_size(250.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                filters_changed = filters::panel(ui, &mut self.settings.filters, &self.choices);
            });
        });
        if filters_changed {
            self.refresh();
        }

        if self.settings.show_detail {
            egui::Panel::right("detail").resizable(true).default_size(360.0).show(ui, |ui| {
                match self.selected.map(|i| &self.tickets[i]) {
                    Some(t) => detail::show(ui, t),
                    None => {
                        ui.label(RichText::new("Select a ticket to see all of its fields.").weak());
                    }
                }
            });
        }

        egui::CentralPanel::default_margins().show(ui, |ui| self.table(ui));

        self.columns_window(&ctx);
        self.stats_window(&ctx);
        self.misc_windows(&ctx);

        if let Some(shot) = &mut self.screenshot
            && self.loader.is_none()
        {
            if shot.prepare && self.selected.is_none() {
                self.selected = self.visible.first().copied();
                self.show_stats = shot.stats;
            }
            shot.step(&ctx);
        }
    }

    fn persist_egui_memory(&self) -> bool {
        self.screenshot.is_none()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.screenshot.is_none() {
            eframe::set_value(storage, SETTINGS_KEY, &self.settings);
        }
    }
}
