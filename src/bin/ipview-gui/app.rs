use std::path::PathBuf;

use eframe::egui::{self, Color32, Key, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};

use ipview::export;
use ipview::fields::{self, Field};
use ipview::filter::Op;
use ipview::stats::{MOS_BANDS, Stats, Summary};
use ipview::ticket::{self, Ticket, Value};

use crate::columns::{self, HeaderAction, HeaderFilterDraft, HeaderMenu};
use crate::download::{DownloadDialog, DownloadSettings};
use crate::filters::{self, Choices, FilterState};
use crate::i18n::{self, LanguagePref, t, tf};
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
    pub language: LanguagePref,
    pub download: DownloadSettings,
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
            language: LanguagePref::System,
            download: DownloadSettings::default(),
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
    /// Status bar text; `None` shows the (translated) welcome hint.
    status: Option<String>,
    errors: Vec<String>,
    show_errors: bool,
    show_stats: bool,
    show_columns: bool,
    show_about: bool,
    stats: Option<Stats>,
    /// Scroll the table to the selection on the next frame.
    scroll_to_selected: bool,
    header_draft: HeaderFilterDraft,
    download: DownloadDialog,
    /// Shown before the next "Loaded … files" message (e.g. the download summary).
    status_note: Option<String>,
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
        i18n::apply(settings.language);
        let mut app = App {
            settings,
            tickets: Vec::new(),
            visible: Vec::new(),
            selected: None,
            choices: Choices::default(),
            loader: None,
            status: None,
            errors: Vec::new(),
            show_errors: false,
            show_stats: false,
            show_columns: false,
            show_about: false,
            stats: None,
            scroll_to_selected: false,
            header_draft: HeaderFilterDraft::default(),
            download: DownloadDialog::default(),
            status_note: None,
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
        let status = if loaded.cancelled {
            tf("Cancelled. Loaded {files} files.", &[("files", &loaded.files)])
        } else if self.errors.is_empty() {
            tf("Loaded {files} files.", &[("files", &loaded.files)])
        } else {
            tf(
                "Loaded {files} files, {errors} could not be read.",
                &[("files", &loaded.files), ("errors", &self.errors.len())],
            )
        };
        self.status = Some(match self.status_note.take() {
            Some(note) => format!("{note} {status}"),
            None => status,
        });
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
            .add_filter(t("Ticket files"), &["DAT", "dat", "ipv", "IPV"])
            .add_filter(t("All files"), &["*"])
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
        let Some(path) = self.dialog().add_filter(t("IPView file"), &["ipv"]).set_file_name("tickets.ipv").save_file()
        else {
            return;
        };
        let tickets: Vec<&Ticket> = self.visible_tickets().collect();
        self.status = Some(match ipview::file::write_ipv(&path, tickets) {
            Ok(n) => tf("Saved {n} tickets to {path}.", &[("n", &n), ("path", &path.display())]),
            Err(e) => tf("Saving failed: {error}", &[("error", &format!("{e:#}"))]),
        });
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
        self.status = Some(match result {
            Ok(()) => tf("Exported {n} tickets to {path}.", &[("n", &self.visible.len()), ("path", &path.display())]),
            Err(e) => tf("Export failed: {error}", &[("error", &format!("{e:#}"))]),
        });
    }

    fn clear_database(&mut self) {
        self.tickets.clear();
        self.choices = Choices::default();
        self.selected = None;
        self.refresh();
        self.status = Some(t("Database cleared.").into());
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
            ui.menu_button(t("File"), |ui| {
                if ui.add_enabled(idle, egui::Button::new(t("Open files…")).shortcut_text("F3")).clicked() {
                    self.open_files(true, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new(t("Open folder…"))).clicked() {
                    self.open_folder(true, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new(t("Add files…"))).clicked() {
                    self.open_files(false, &ctx);
                }
                if ui.add_enabled(idle, egui::Button::new(t("Add folder…"))).clicked() {
                    self.open_folder(false, &ctx);
                }
                ui.separator();
                if ui.add_enabled(idle, egui::Button::new(t("Download from OXE (SFTP)…"))).clicked() {
                    self.download.open = true;
                }
                ui.separator();
                let any = !self.visible.is_empty();
                if ui.add_enabled(any, egui::Button::new(t("Save visible tickets as .ipv…"))).clicked() {
                    self.save_ipv();
                }
                if ui.add_enabled(any, egui::Button::new(t("Export visible tickets to CSV…"))).clicked() {
                    self.export(false);
                }
                if ui.add_enabled(any, egui::Button::new(t("Export visible tickets to JSON…"))).clicked() {
                    self.export(true);
                }
                ui.menu_button(t("CSV delimiter"), |ui| {
                    for (d, label) in [(';', t("Semicolon (Excel, European)")), (',', t("Comma")), ('\t', t("Tab"))] {
                        ui.radio_value(&mut self.settings.csv_delimiter, d, label);
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(!self.tickets.is_empty(), egui::Button::new(t("Clear database")).shortcut_text("F8"))
                    .clicked()
                {
                    self.clear_database();
                }
                if ui.button(t("Quit")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(t("View"), |ui| {
                if ui.button(t("Columns…")).clicked() {
                    self.show_columns = true;
                }
                if ui.checkbox(&mut self.show_stats, t("Statistics")).changed() {
                    self.refresh();
                }
                ui.checkbox(&mut self.settings.show_detail, t("Ticket details"));
                if !self.errors.is_empty() {
                    ui.checkbox(&mut self.show_errors, t("Load errors"));
                }
                ui.separator();
                // egui's built-in theme buttons use emoji icons the default fonts lack.
                ui.menu_button(t("Theme"), |ui| {
                    let mut theme = ui.options(|o| o.theme_preference);
                    for (choice, label) in [
                        (egui::ThemePreference::System, t("Follow system")),
                        (egui::ThemePreference::Light, t("Light")),
                        (egui::ThemePreference::Dark, t("Dark")),
                    ] {
                        if ui.radio_value(&mut theme, choice, label).clicked() {
                            ui.ctx().set_theme(theme);
                        }
                    }
                });
                ui.menu_button(t("Language"), |ui| {
                    // Language names are shown in their own language.
                    let system = match i18n::system_language() {
                        i18n::Lang::De => "Deutsch",
                        i18n::Lang::En => "English",
                    };
                    let mut pref = self.settings.language;
                    let changed = [
                        ui.radio_value(&mut pref, LanguagePref::System, format!("{} ({system})", t("Follow system"))),
                        ui.radio_value(&mut pref, LanguagePref::English, "English"),
                        ui.radio_value(&mut pref, LanguagePref::German, "Deutsch"),
                    ]
                    .iter()
                    .any(|r| r.clicked());
                    if changed {
                        self.settings.language = pref;
                        i18n::apply(pref);
                    }
                });
            });
            ui.menu_button(t("Help"), |ui| {
                if ui.button(t("About")).clicked() {
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
                        .text(tf("Loading {done} / {total} files", &[("done", &done), ("total", &loader.total)])),
                );
                if ui.button(t("Cancel")).clicked() {
                    loader.cancel();
                }
            } else {
                ui.label(
                    self.status
                        .as_deref()
                        .unwrap_or(t("Open ticket files (IP*.DAT, *.ipv) with File > Open, F3, or drag and drop.")),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(tf(
                    "Tickets in database: {total}   Visible: {visible}",
                    &[("total", &self.tickets.len()), ("visible", &self.visible.len())],
                ));
            });
        });
    }

    fn table(&mut self, ui: &mut Ui) {
        if self.tickets.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(t("No tickets loaded.\nFile > Open (F3) or drop IP*.DAT files here."))
                        .size(16.0)
                        .weak(),
                );
            });
            return;
        }

        ui.horizontal(|ui| {
            columns::picker_button(ui, &mut self.settings.columns);
            ui.label(RichText::new(t("Right-click a column header to sort, filter, move or hide it.")).weak().small());
        });

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
        let mut header_action = None;
        let mut new_condition = None;
        let mut clicked_row = None;
        table
            .header(row_height + 4.0, |mut header| {
                for (position, f) in columns.iter().enumerate() {
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
                            .on_hover_text(tf(
                                "{name} (field {id}). Click to sort, right-click for options.",
                                &[("name", &i18n::field_name(f)), ("id", &f.id)],
                            ));
                        if r.clicked() {
                            clicked_sort = Some(f.id);
                        }
                        let filters_on_field =
                            self.settings.filters.conditions.iter().filter(|c| c.field == f.id).count();
                        // Stays open while choosing an operator or typing; actions close it.
                        let popup =
                            egui::Popup::context_menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                        popup.show(|ui| {
                            let menu = HeaderMenu {
                                field: f,
                                sort: self.settings.sort,
                                position,
                                column_count: columns.len(),
                                filters_on_field,
                                draft: &mut self.header_draft,
                                columns: &mut self.settings.columns,
                            };
                            if let Some(a) = menu.show(ui) {
                                header_action = Some(a);
                            }
                        });
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, self.visible.len(), |mut row| {
                    let index = self.visible[row.index()];
                    let ticket = &self.tickets[index];
                    row.set_selected(self.selected == Some(index));
                    let metrics = ticket.metrics();
                    for f in &columns {
                        row.col(|ui| {
                            let value = ticket.value(f.id);
                            let text = value.as_ref().map(i18n::value_text).unwrap_or_default();
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
                                    ui.label(RichText::new(i18n::field_name(f)).strong());
                                    for (op, verb) in [(Op::Eq, "="), (Op::Ne, "≠"), (Op::Lt, "<"), (Op::Gt, ">")] {
                                        if ui
                                            .button(tf("Filter {op} {value}", &[("op", &verb), ("value", &text)]))
                                            .clicked()
                                        {
                                            new_condition = Some(filters::condition_for(f, op, text.clone()));
                                        }
                                    }
                                    if ui.button(t("Copy value")).clicked() {
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
        self.header_draft.frame_end();
        if let Some(action) = header_action {
            self.apply_header_action(action);
        }
    }

    fn apply_header_action(&mut self, action: HeaderAction) {
        let cols = &mut self.settings.columns;
        match action {
            HeaderAction::Sort { field, descending } => self.settings.sort = Some((field, descending)),
            HeaderAction::ClearSort => self.settings.sort = None,
            HeaderAction::AddFilter(c) => self.settings.filters.conditions.push(c),
            HeaderAction::RemoveFilters(field) => self.settings.filters.conditions.retain(|c| c.field != field),
            HeaderAction::Move { field, by } => {
                if let Some(i) = cols.iter().position(|&c| c == field) {
                    let j = i.saturating_add_signed(by).min(cols.len() - 1);
                    cols.swap(i, j);
                }
                return;
            }
            HeaderAction::Hide(field) => {
                if cols.len() > 1 {
                    cols.retain(|&c| c != field);
                }
                return;
            }
        }
        self.refresh();
    }

    fn columns_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_columns;
        egui::Window::new(t("Columns"))
            .id(egui::Id::unique("columns_window"))
            .open(&mut open)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label(t("Shown columns, in order:"));
                let mut action = None;
                for (i, id) in self.settings.columns.iter().enumerate() {
                    let name = fields::by_id(*id).map_or("?", i18n::field_name);
                    ui.horizontal(|ui| {
                        if ui.small_button(t("Up")).clicked() && i > 0 {
                            action = Some((i, -1));
                        }
                        if ui.small_button(t("Down")).clicked() {
                            action = Some((i, 1));
                        }
                        if ui.add_enabled(self.settings.columns.len() > 1, egui::Button::new("×").small()).clicked() {
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
                ui.label(t("Available columns:"));
                columns::picker(ui, &mut self.settings.columns);
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
        egui::Window::new(t("Statistics (visible tickets)"))
            .id(egui::Id::unique("stats_window"))
            .open(&mut open)
            .default_width(460.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(tf("Tickets: {n}", &[("n", &stats.tickets)]));
                if let Some((a, b)) = stats.period {
                    ui.label(tf(
                        "Period: {from} – {to} UTC",
                        &[("from", &ticket::format_timestamp(a)), ("to", &ticket::format_timestamp(b))],
                    ));
                }
                ui.add_space(6.0);
                egui::Grid::new("summary").striped(true).num_columns(7).show(ui, |ui| {
                    for h in ["", t("mean"), t("min"), t("p5"), t("median"), t("p95"), t("max")] {
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
                    row(ui, t("MOS"), stats.mos);
                    row(ui, t("Loss %"), stats.loss_pct);
                    row(ui, t("Round-trip ms"), stats.rtd);
                    row(ui, t("Jitter ms"), stats.jitter_delay);
                });

                if let Some(mos) = stats.mos {
                    ui.add_space(8.0);
                    ui.strong(t("MOS bands"));
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
                        cols.iter_mut().zip([(t("Codecs"), &stats.codecs), (t("Equipment"), &stats.equipment)])
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
        egui::Window::new(t("Load errors"))
            .id(egui::Id::unique("errors_window"))
            .open(&mut self.show_errors)
            .default_width(500.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for e in &self.errors {
                        ui.label(e);
                    }
                });
            });
        egui::Window::new(t("About"))
            .id(egui::Id::unique("about_window"))
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading(format!("IPView (Rust) {}", env!("CARGO_PKG_VERSION")));
                ui.label(t("Analyzer for Alcatel-Lucent OmniPCX Enterprise VoIP tickets (IP*.DAT, .ipv)."));
                ui.label(t("A rewrite of IPView 2.0.1 by Sven Fabricius (2007)."));
                ui.add_space(6.0);
                ui.label(t("MOS: ITU-T G.107 E-model (narrowband), G.107.1 (wideband, G.722)."));
                ui.label(t("Times are shown in UTC."));
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
        1 => t("End time"),
        12 => t("Dur."),
        22 => t("RTP rx"),
        23 => t("RTP tx"),
        24 => t("RTP lost"),
        500 => t("Loss"),
        501 => t("Round-trip"),
        502 => t("Jitter"),
        504 => t("R (WB)"),
        505 => t("Jitter corrupt"),
        _ => i18n::field_name(f),
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
                        ui.label(RichText::new(t("Select a ticket to see all of its fields.")).weak());
                    }
                }
            });
        }

        egui::CentralPanel::default_margins().show(ui, |ui| self.table(ui));

        self.columns_window(&ctx);
        self.stats_window(&ctx);
        self.misc_windows(&ctx);
        if let Some(done) = self.download.show(&ctx, &mut self.settings.download) {
            self.status_note = self.download.take_message();
            self.load(done.paths, done.replace, &ctx);
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn tlv(entries: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, v) in entries {
            out.extend(id.to_le_bytes());
            out.push(v.len() as u8);
            out.extend(*v);
        }
        out
    }

    /// Three G.711 calls with different durations.
    fn tickets() -> Vec<Ticket> {
        [(5u16, 100u32), (30, 101), (10, 102)]
            .iter()
            .map(|&(duration, time)| {
                Ticket::from_raw(&tlv(&[
                    (1, &(1_790_000_000 + time).to_be_bytes()),
                    (12, &duration.to_be_bytes()),
                    (15, &[0]),
                    (20, &[20]),
                    (22, &500u32.to_be_bytes()),
                    (24, &0u16.to_be_bytes()),
                ]))
            })
            .collect()
    }

    static LANGUAGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The UI language is global; tests that depend on it hold this lock.
    fn language(lang: i18n::Lang) -> std::sync::MutexGuard<'static, ()> {
        let guard = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
        i18n::set(lang);
        guard
    }

    fn harness() -> Harness<'static, App> {
        let mut h = Harness::builder().with_size([1300.0, 800.0]).build_eframe(|cc| {
            let mut app = App::new(cc, Vec::new(), None);
            app.settings = Settings { language: LanguagePref::English, ..Settings::default() };
            i18n::apply(app.settings.language);
            app.tickets = tickets();
            app.choices = Choices::from_tickets(&app.tickets);
            app.refresh();
            app
        });
        h.run();
        h
    }

    fn durations(h: &Harness<'_, App>) -> Vec<u16> {
        let app = h.state();
        app.visible.iter().map(|&i| app.tickets[i].u16(12).unwrap()).collect()
    }

    /// Saves a rendered image when IPVIEW_SNAPSHOT_DIR is set (needs a wgpu adapter).
    fn snapshot(h: &mut Harness<'_, App>, name: &str) {
        if let Some(dir) = std::env::var_os("IPVIEW_SNAPSHOT_DIR") {
            let image = h.render().expect("render");
            image.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
        }
    }

    #[test]
    fn header_menu_sorts() {
        let _lang = language(i18n::Lang::En);
        let mut h = harness();
        assert_eq!(durations(&h), [5, 30, 10]);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        snapshot(&mut h, "header_menu");
        h.get_by_label("Sort descending").click();
        h.run();
        assert_eq!(h.state().settings.sort, Some((12, true)));
        assert_eq!(durations(&h), [30, 10, 5]);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        h.get_by_label("Sort ascending").click();
        h.run();
        assert_eq!(durations(&h), [5, 10, 30]);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        h.get_by_label("Clear sorting").click();
        h.run();
        assert_eq!(h.state().settings.sort, None);
    }

    #[test]
    fn header_menu_filters_by_typing() {
        let _lang = language(i18n::Lang::En);
        let mut h = harness();
        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        // The value field keeps keyboard focus after picking an operator.
        h.get_by_label("≥").click();
        h.run();
        h.event(egui::Event::Text("10".into()));
        h.run();
        h.key_press(Key::Enter);
        h.run();
        let conditions = &h.state().settings.filters.conditions;
        assert_eq!(conditions.len(), 1, "{:?}", conditions.iter().map(|c| &c.value).collect::<Vec<_>>());
        assert_eq!((conditions[0].field, conditions[0].op, conditions[0].value.as_str()), (12, Op::Ge, "10"));
        assert_eq!(durations(&h), [30, 10]);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        h.get_by_label("Remove filters on this column (1)").click();
        h.run();
        assert!(h.state().settings.filters.conditions.is_empty());
        assert_eq!(durations(&h).len(), 3);
    }

    #[test]
    fn header_menu_moves_and_hides_columns() {
        let _lang = language(i18n::Lang::En);
        let mut h = harness();
        let pos = |h: &Harness<'_, App>| h.state().settings.columns.iter().position(|&c| c == 12).unwrap();
        let start = pos(&h);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        h.get_by_label("Move left").click();
        h.run();
        assert_eq!(pos(&h), start - 1);

        h.get_by_label("Dur. [s]").click_secondary();
        h.run();
        h.get_by_label("Hide column").click();
        h.run();
        assert!(!h.state().settings.columns.contains(&12));
    }

    #[test]
    fn theme_menu_switches_theme() {
        let _lang = language(i18n::Lang::En);
        let mut h = harness();
        h.get_by_label("View").click();
        h.run();
        h.get_by_label("Theme").click();
        h.run();
        snapshot(&mut h, "theme_menu");
        h.get_by_label("Light").click();
        h.run();
        assert_eq!(h.ctx.options(|o| o.theme_preference), egui::ThemePreference::Light);
        assert_eq!(h.ctx.theme(), egui::Theme::Light);
    }

    #[test]
    fn german_interface() {
        let _lang = language(i18n::Lang::De);
        let mut h = Harness::builder().with_size([1300.0, 800.0]).build_eframe(|cc| {
            let mut app = App::new(cc, Vec::new(), None);
            app.settings = Settings { language: LanguagePref::German, ..Settings::default() };
            i18n::apply(app.settings.language);
            app.tickets = tickets();
            app.choices = Choices::from_tickets(&app.tickets);
            app.selected = Some(0);
            app.refresh();
            app
        });
        h.run();
        snapshot(&mut h, "german");
        for label in ["Datei", "Ansicht", "Hilfe", "Filter", "Gesprächsdauer", "Kompressionsart"] {
            assert!(h.query_by_label(label).is_some(), "missing German label {label:?}");
        }
        h.get_by_label("Datei").click();
        h.run();
        assert!(h.query_by_label("Von OXE herunterladen (SFTP)…").is_some());
        snapshot(&mut h, "german_file_menu");
    }

    /// Full dialog flow against scripts/test_sftp_server.py (needs IPVIEW_SFTP_TEST_PYTHON).
    #[test]
    fn download_dialog_fetches_and_loads() {
        let Ok(python) = std::env::var("IPVIEW_SFTP_TEST_PYTHON") else {
            eprintln!("IPVIEW_SFTP_TEST_PYTHON not set, skipping");
            return;
        };
        let _lang = language(i18n::Lang::En);
        let base = std::env::temp_dir().join(format!("ipview-gui-sftp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let account = base.join("usr4/account");
        std::fs::create_dir_all(&account).unwrap();
        // Two ticket files from the test data: one recent, one old.
        let mut raw = Vec::new();
        for t in tickets() {
            raw.extend(t.raw());
        }
        std::fs::write(account.join("IPNEW.DAT"), &raw).unwrap();
        std::fs::write(account.join("IPOLD.DAT"), &raw).unwrap();
        std::fs::File::options()
            .write(true)
            .open(account.join("IPOLD.DAT"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400))
            .unwrap();

        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/test_sftp_server.py");
        struct KillOnDrop(std::process::Child);
        impl Drop for KillOnDrop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut server = std::process::Command::new(python)
            .arg(script)
            .arg(&base)
            .arg(base.join("host_key"))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(server.stdout.take().unwrap()), &mut line).unwrap();
        let port: u16 = line.trim().parse().unwrap();
        let _server = KillOnDrop(server);

        let mut h = harness();
        {
            let app = h.state_mut();
            app.tickets.clear();
            app.refresh();
            app.download.known_hosts = base.join("known_hosts");
            app.download.password = "secret".into();
            let d = &mut app.settings.download;
            d.target.host = "127.0.0.1".into();
            d.target.port = port;
            d.selection = ipview::remote::Selection::LastDays(2);
            d.local_dir = Some(base.join("downloads"));
        }
        h.run();
        h.get_by_label("File").click();
        h.run();
        h.get_by_label("Download from OXE (SFTP)…").click();
        h.run();
        snapshot(&mut h, "download_dialog");
        h.get_by_label("Download and open").click();

        // The progress bar animates while working, so step frames instead of waiting to settle.
        let wait_for = |h: &mut Harness<'_, App>, what: &str, done: &dyn Fn(&Harness<'_, App>) -> bool| {
            for _ in 0..400 {
                h.run_steps(2);
                if done(h) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            panic!("timed out waiting for {what}");
        };
        wait_for(&mut h, "host key prompt", &|h| h.query_by_label("Trust and continue").is_some());
        snapshot(&mut h, "download_host_key");
        h.get_by_label("Trust and continue").click();
        wait_for(&mut h, "tickets", &|h| !h.state().tickets.is_empty());

        let app = h.state();
        assert_eq!(app.tickets.len(), 3, "only the recent file (3 tickets) is downloaded and loaded");
        assert!(base.join("downloads/IPNEW.DAT").exists());
        assert!(!base.join("downloads/IPOLD.DAT").exists());
        let status = app.status.as_deref().unwrap_or_default();
        assert!(status.contains("1 files downloaded"), "{status}");
        assert_eq!(app.settings.download.recent.first().map(|t| t.port), Some(port));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn column_picker_adds_and_removes() {
        let _lang = language(i18n::Lang::En);
        let mut h = harness();
        assert!(!h.state().settings.columns.contains(&15));
        h.get_by_label("Columns").click();
        h.run();
        snapshot(&mut h, "column_picker");
        h.get_by_label("Compression type").scroll_to_me();
        h.run();
        h.get_by_label("Compression type").click();
        h.run();
        assert!(h.state().settings.columns.contains(&15));
        // The picker stays open; the new column header has the same name, so pick the checkbox.
        h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Compression type").click();
        h.run();
        assert!(!h.state().settings.columns.contains(&15));
    }
}
