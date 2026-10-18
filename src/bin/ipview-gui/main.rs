//! Desktop GUI for OXE VoIP tickets.
//!
//! Usage: `ipview-gui [FILES OR FOLDERS...]`
//!
//! Set `IPVIEW_GUI_SCREENSHOT=out.png` to render once, save a screenshot and
//! exit. `IPVIEW_GUI_STATS=1` opens the statistics window first, and
//! `IPVIEW_GUI_FILTER="mos<3.6;codec=G722"` applies field conditions and
//! `IPVIEW_GUI_SORT=mos` sorts (prefix `-` for descending).
//! Used to check the UI without clicking through it.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod columns;
mod detail;
mod download;
mod filters;
mod i18n;
mod loader;
mod style;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;

pub struct Screenshot {
    path: PathBuf,
    stats: bool,
    conditions: Vec<filters::DynCondition>,
    sort: Option<(u16, bool)>,
    /// Select a ticket and open windows before the shot.
    prepare: bool,
    frames: u32,
    requested: bool,
    done: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Screenshot {
    fn from_env() -> Option<Screenshot> {
        let path = std::env::var_os("IPVIEW_GUI_SCREENSHOT")?;
        Some(Screenshot {
            path: path.into(),
            stats: std::env::var_os("IPVIEW_GUI_STATS").is_some(),
            conditions: std::env::var("IPVIEW_GUI_FILTER")
                .unwrap_or_default()
                .split(';')
                .filter_map(|c| c.parse::<ipview::filter::Condition>().ok())
                .map(|c| filters::DynCondition { field: c.field.id, op: c.op, value: c.value })
                .collect(),
            sort: std::env::var("IPVIEW_GUI_SORT").ok().and_then(|s| {
                let (key, desc) = s.strip_prefix('-').map_or((s.as_str(), false), |k| (k, true));
                ipview::fields::lookup(key).map(|f| (f.id, desc))
            }),
            prepare: true,
            frames: 0,
            requested: false,
            done: Arc::default(),
        })
    }

    /// Called every frame once loading has finished.
    fn step(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        // Give layout a few frames to settle (column widths, windows).
        if self.frames == 10 && !self.requested {
            self.requested = true;
            let (path, done) = (self.path.clone(), self.done.clone());
            ctx.request_screenshot(move |image| {
                let [w, h] = image.size;
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                let result = image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string());
                *done.lock().unwrap() = Some(result);
            });
        }
        if let Some(result) = self.done.lock().unwrap().take() {
            match result {
                Ok(()) => eprintln!("screenshot saved to {}", self.path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let screenshot = Screenshot::from_env();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("IPView")
            .with_inner_size([1600.0, 920.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        persist_window: screenshot.is_none(),
        ..Default::default()
    };
    eframe::run_native("IPView", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths, screenshot)))))
}
