//! Loads ticket files on a background thread so the window stays responsive.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use ipview::{Ticket, file};

pub struct Loaded {
    pub tickets: Vec<Ticket>,
    pub files: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

pub struct Loader {
    pub total: usize,
    done: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Loaded>,
    /// Whether the result replaces the current tickets or is added to them.
    pub replace: bool,
}

impl Loader {
    /// Starts loading; `paths` may contain files and directories.
    pub fn start(paths: Vec<PathBuf>, replace: bool, ctx: eframe::egui::Context) -> Loader {
        let (files, mut errors) = match file::expand(&paths) {
            Ok(files) => (files, Vec::new()),
            Err(e) => (Vec::new(), vec![format!("{e:#}")]),
        };
        let done = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        let total = files.len();
        {
            let (done, cancel) = (done.clone(), cancel.clone());
            thread::spawn(move || {
                let mut tickets = Vec::new();
                let mut cancelled = false;
                for path in &files {
                    if cancel.load(Ordering::Relaxed) {
                        cancelled = true;
                        break;
                    }
                    match file::read(path) {
                        Ok(ts) => tickets.extend(ts),
                        Err(e) => errors.push(format!("{e:#}")),
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                    ctx.request_repaint();
                }
                let files = done.load(Ordering::Relaxed);
                let _ = tx.send(Loaded { tickets, files, errors, cancelled });
                ctx.request_repaint();
            });
        }
        Loader { total, done, cancel, result, replace }
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Returns the result once loading has finished.
    pub fn poll(&self) -> Option<Loaded> {
        match self.result.try_recv() {
            Ok(loaded) => Some(loaded),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Loaded {
                tickets: Vec::new(),
                files: 0,
                errors: vec!["loader thread stopped unexpectedly".into()],
                cancelled: false,
            }),
        }
    }
}
