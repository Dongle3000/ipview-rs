//! "Download from OXE (SFTP)" dialog.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};

use ipview::remote::{self, Connection, HostKeyProblem, Selection, Target, TrustNewKey};

use crate::i18n::{t, tf};

const MAX_RECENT: usize = 10;

/// Remembered between sessions; the password is never stored.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    pub target: Target,
    pub selection: Selection,
    /// Empty: Downloads/IPView/<host>.
    pub local_dir: Option<PathBuf>,
    pub add_to_loaded: bool,
    pub recent: Vec<Target>,
}

/// Files to load after a successful download.
pub struct Finished {
    pub paths: Vec<PathBuf>,
    pub replace: bool,
}

enum Stage {
    Connecting,
    Listing,
    Downloading { index: usize, count: usize, name: String },
}

enum Outcome {
    Done { paths: Vec<PathBuf>, fetched: usize, skipped: usize, dir: PathBuf },
    NothingSelected { total: usize },
    HostKey(HostKeyProblem),
    Failed(String),
}

struct Worker {
    stage: Arc<Mutex<Stage>>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Outcome>,
}

pub struct DownloadDialog {
    pub open: bool,
    pub(crate) password: String,
    /// Where trusted host keys are stored.
    pub(crate) known_hosts: PathBuf,
    worker: Option<Worker>,
    /// Fingerprint waiting for the user's confirmation.
    confirm_key: Option<String>,
    message: Option<(bool, String)>,
}

impl Default for DownloadDialog {
    fn default() -> Self {
        DownloadDialog {
            open: false,
            password: String::new(),
            known_hosts: remote::known_hosts_path(),
            worker: None,
            confirm_key: None,
            message: None,
        }
    }
}

impl DownloadDialog {
    /// Draws the dialog; returns files to open once a download finished.
    pub fn show(&mut self, ctx: &egui::Context, settings: &mut DownloadSettings) -> Option<Finished> {
        if !self.open {
            return None;
        }
        let finished = self.poll(settings);
        let mut open = true;
        egui::Window::new(t("Download from OXE (SFTP)"))
            .id(egui::Id::unique("download_window"))
            .open(&mut open)
            .default_width(430.0)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| self.contents(ui, settings));
        if !open {
            if let Some(w) = &self.worker {
                w.cancel.store(true, Ordering::Relaxed);
            }
            self.open = false;
        }
        finished
    }

    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    fn contents(&mut self, ui: &mut Ui, s: &mut DownloadSettings) {
        let busy = self.busy();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Grid::new("sftp_target").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                let l = ui.label(t("Host"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.target.host).desired_width(180.0).hint_text("10.0.0.10"))
                        .labelled_by(l.id);
                    if !s.recent.is_empty() {
                        let mut picked = None;
                        egui::ComboBox::from_id_salt("recent_targets").selected_text(t("Recent")).width(90.0).show_ui(
                            ui,
                            |ui| {
                                for r in &s.recent {
                                    if ui.selectable_label(false, format!("{}@{}", r.user, r.host)).clicked() {
                                        picked = Some(r.clone());
                                    }
                                }
                            },
                        );
                        if let Some(r) = picked {
                            s.target = r;
                        }
                    }
                });
                ui.end_row();

                let l = ui.label(t("Port"));
                ui.add(egui::DragValue::new(&mut s.target.port).range(1..=65535)).labelled_by(l.id);
                ui.end_row();

                let l = ui.label(t("User"));
                ui.add(egui::TextEdit::singleline(&mut s.target.user).desired_width(180.0)).labelled_by(l.id);
                ui.end_row();

                let l = ui.label(t("Password"));
                let pw = ui
                    .add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(180.0))
                    .labelled_by(l.id)
                    .on_hover_text(t("The password is not saved."));
                if pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.start(s, TrustNewKey::Ask, ui.ctx());
                }
                ui.end_row();

                let l = ui.label(t("Remote folder"));
                ui.add(egui::TextEdit::singleline(&mut s.target.dir).desired_width(180.0)).labelled_by(l.id);
                ui.end_row();

                let l = ui.label(t("Download"));
                selection_editor(ui, &mut s.selection, l.id);
                ui.end_row();

                let l = ui.label(t("Local folder"));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let mut text = s.local_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                        let default = remote::default_download_dir(&s.target.host);
                        let edit = ui
                            .add(
                                egui::TextEdit::singleline(&mut text)
                                    .desired_width(180.0)
                                    .hint_text(default.display().to_string()),
                            )
                            .labelled_by(l.id);
                        if edit.changed() {
                            s.local_dir = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
                        }
                        if ui.button(t("Browse…")).clicked()
                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                        {
                            s.local_dir = Some(dir);
                        }
                    });
                    if s.local_dir.is_none() {
                        let default = remote::default_download_dir(&s.target.host);
                        ui.label(RichText::new(tf("Default: {path}", &[("path", &default.display())])).small().weak());
                    }
                });
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.add_to_loaded, false, t("Replace loaded tickets"));
                ui.radio_value(&mut s.add_to_loaded, true, t("Add to loaded tickets"));
            });
        });

        ui.separator();
        if let Some(fingerprint) = self.confirm_key.clone() {
            ui.label(RichText::new(t("The host key of this server is not known yet.")).strong());
            ui.monospace(&fingerprint);
            ui.label(t("Only continue if the fingerprint matches the one of your OXE."));
            ui.horizontal(|ui| {
                if ui.button(t("Trust and continue")).clicked() {
                    self.confirm_key = None;
                    self.start(s, TrustNewKey::Accept, ui.ctx());
                }
                if ui.button(t("Cancel")).clicked() {
                    self.confirm_key = None;
                }
            });
            return;
        }

        if let Some(w) = &self.worker {
            let text = match &*w.stage.lock().unwrap() {
                Stage::Connecting => tf("Connecting to {host}…", &[("host", &s.target.host)]),
                Stage::Listing => t("Listing files…").to_string(),
                Stage::Downloading { index, count, name } => {
                    tf("Downloading {i} / {n}: {name}", &[("i", &(index + 1)), ("n", count), ("name", name)])
                }
            };
            let fraction = match &*w.stage.lock().unwrap() {
                Stage::Downloading { index, count, .. } => *index as f32 / (*count).max(1) as f32,
                _ => 0.0,
            };
            ui.add(egui::ProgressBar::new(fraction).text(text).animate(true));
            if ui.button(t("Cancel")).clicked() {
                w.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }

        if let Some((ok, msg)) = &self.message {
            let color = if *ok { ui.visuals().text_color() } else { ui.visuals().error_fg_color };
            ui.label(RichText::new(msg).color(color));
        }
        ui.horizontal(|ui| {
            if ui.button(RichText::new(t("Download and open")).strong()).clicked() {
                self.start(s, TrustNewKey::Ask, ui.ctx());
            }
            if ui.button(t("Close")).clicked() {
                self.open = false;
            }
        });
    }

    fn start(&mut self, s: &mut DownloadSettings, trust: TrustNewKey, ctx: &egui::Context) {
        s.target.host = s.target.host.trim().to_string();
        if s.target.host.is_empty() {
            self.message = Some((false, t("Enter the host name or IP address of the OXE.").into()));
            return;
        }
        if self.password.is_empty() {
            self.message = Some((false, t("Enter the password.").into()));
            return;
        }
        self.message = None;
        let target = s.target.clone();
        let password = self.password.clone();
        let selection = s.selection;
        let known_hosts = self.known_hosts.clone();
        let dir = s.local_dir.clone().unwrap_or_else(|| remote::default_download_dir(&target.host));
        let stage = Arc::new(Mutex::new(Stage::Connecting));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        {
            let (stage, cancel, ctx) = (stage.clone(), cancel.clone(), ctx.clone());
            thread::spawn(move || {
                let job = Job { target, password, selection, dir, known_hosts, trust };
                let outcome = run(&job, &stage, &cancel, &ctx);
                let _ = tx.send(outcome);
                ctx.request_repaint();
            });
        }
        self.worker = Some(Worker { stage, cancel, result });
    }

    fn poll(&mut self, s: &mut DownloadSettings) -> Option<Finished> {
        let w = self.worker.as_ref()?;
        let outcome = match w.result.try_recv() {
            Ok(o) => o,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Outcome::Failed("worker stopped".into()),
        };
        self.worker = None;
        match outcome {
            Outcome::Done { paths, fetched, skipped, dir } => {
                remember(s);
                self.message = Some((
                    true,
                    tf(
                        "{fetched} files downloaded, {skipped} already present, in {path}.",
                        &[("fetched", &fetched), ("skipped", &skipped), ("path", &dir.display())],
                    ),
                ));
                self.open = false;
                Some(Finished { paths, replace: !s.add_to_loaded })
            }
            Outcome::NothingSelected { total } => {
                remember(s);
                self.message = Some((
                    false,
                    tf("No files match the selection ({total} ticket files on the OXE).", &[("total", &total)]),
                ));
                None
            }
            Outcome::HostKey(HostKeyProblem::Unknown { fingerprint }) => {
                self.confirm_key = Some(fingerprint);
                None
            }
            Outcome::HostKey(HostKeyProblem::Changed { .. }) => {
                let path = self.known_hosts.clone();
                self.message = Some((
                    false,
                    tf(
                        "The host key of this server has CHANGED. Someone may be intercepting the connection. If the OXE was reinstalled, remove its entry from {path}.",
                        &[("path", &path.display())],
                    ),
                ));
                None
            }
            Outcome::Failed(error) => {
                self.message = Some((false, error));
                None
            }
        }
    }

    /// The last result message, e.g. for the status bar.
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take().filter(|(ok, _)| *ok).map(|(_, m)| m)
    }
}

fn remember(s: &mut DownloadSettings) {
    s.recent.retain(|r| r.host != s.target.host || r.port != s.target.port);
    s.recent.insert(0, s.target.clone());
    s.recent.truncate(MAX_RECENT);
}

fn selection_editor(ui: &mut Ui, selection: &mut Selection, label: egui::Id) {
    let (mut kind, mut n) = match *selection {
        Selection::LastFiles(n) => (0, n),
        Selection::LastDays(n) => (1, n),
        Selection::LastHours(n) => (2, n),
    };
    ui.horizontal(|ui| {
        ui.label(t("Last"));
        ui.add(egui::DragValue::new(&mut n).range(1..=9999)).labelled_by(label);
        egui::ComboBox::from_id_salt("selection_kind")
            .width(90.0)
            .selected_text([t("files"), t("days"), t("hours")][kind])
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, 0, t("files"));
                ui.selectable_value(&mut kind, 1, t("days"));
                ui.selectable_value(&mut kind, 2, t("hours"));
            })
            .response
            .labelled_by(label);
    });
    *selection = match kind {
        0 => Selection::LastFiles(n),
        1 => Selection::LastDays(n),
        _ => Selection::LastHours(n),
    };
}

struct Job {
    target: Target,
    password: String,
    selection: Selection,
    dir: PathBuf,
    known_hosts: PathBuf,
    trust: TrustNewKey,
}

fn run(job: &Job, stage: &Mutex<Stage>, cancel: &AtomicBool, ctx: &egui::Context) -> Outcome {
    let Job { target, password, selection, dir, known_hosts, trust } = job;
    let conn = match Connection::open(target, password, known_hosts, *trust) {
        Ok(c) => c,
        Err(e) => {
            if let Some(problem) = e.downcast_ref::<HostKeyProblem>() {
                return Outcome::HostKey(problem.clone());
            }
            let text = format!("{e:#}");
            return Outcome::Failed(if text.contains("login failed") {
                t("Login failed: wrong user name or password?").into()
            } else {
                tf("Download failed: {error}", &[("error", &text)])
            });
        }
    };
    *stage.lock().unwrap() = Stage::Listing;
    ctx.request_repaint();

    let result = (|| -> anyhow::Result<Outcome> {
        let files = conn.list()?;
        let chosen = remote::select(&files, *selection, remote::now());
        if chosen.is_empty() {
            return Ok(Outcome::NothingSelected { total: files.len() });
        }
        let (mut paths, mut fetched, mut skipped) = (Vec::new(), 0, 0);
        for (index, f) in chosen.iter().enumerate() {
            *stage.lock().unwrap() = Stage::Downloading { index, count: chosen.len(), name: f.name.clone() };
            ctx.request_repaint();
            let (path, new) = conn.download(f, dir, cancel, |_| {})?;
            if new {
                fetched += 1
            } else {
                skipped += 1
            }
            paths.push(path);
        }
        Ok(Outcome::Done { paths, fetched, skipped, dir: dir.clone() })
    })();
    conn.disconnect();
    result.unwrap_or_else(|e| Outcome::Failed(tf("Download failed: {error}", &[("error", &format!("{e:#}"))])))
}
