//! The Missing fonts window (#752): the fonts a file uses that aren't installed, whose text is
//! drawn in a fallback font. It opens by itself after the user opens a file that has any (once
//! the installed fonts are indexed and any import report is closed), and from Text ▸ Missing fonts.

use std::path::{Path, PathBuf};

use egui::RichText;
use figcraft_engine::fonts::{Missing, missing};
use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{tr, trf, trn};
use crate::theme::{Tokens, regular};

/// The font files the picker offers (the same ones `text.loadFont` reads).
const FONT_EXTS: &[&str] = &["ttf", "otf", "ttc", "otc"];

#[derive(Clone, Debug)]
pub struct Shown {
    /// The document it lists: the window goes when that document closes.
    pub uid: u64,
    pub families: Vec<Missing>,
    /// Asked for before the installed fonts were indexed: the list comes once they are.
    pub indexing: bool,
}

impl Shown {
    pub fn inspect(&self) -> Value {
        json!({
            "families": self.families.iter().map(|m| json!({"family": m.family, "layers": m.layers})).collect::<Vec<_>>(),
            "indexing": self.indexing,
        })
    }
}

/// After the user opened a file, or imported pages into one (results with a `summary`; not a
/// placed image or SVG, a file that was open already, or Clear recent): look for missing fonts in
/// it once that can be told ([`show`]). Not in the browser, which has only its built-in fonts.
pub fn check_later(app: &mut FigApp, command: &str, result: Option<&Value>) {
    if !cfg!(target_arch = "wasm32")
        && matches!(command, "file.open" | "file.openRecent" | "file.import")
        && result.is_some_and(|r| r.get("summary").is_some())
        && let Ok(ds) = app.session.ds()
    {
        app.ui.font_checks.push(ds.uid);
    }
}

/// The window for document `uid`, listing what it misses (even nothing, when asked for).
fn list(app: &mut FigApp, uid: u64) -> Option<Vec<Missing>> {
    let ds = app.session.docs.iter().find(|d| d.uid == uid)?;
    let families = missing(&ds.doc);
    app.ui.missing_fonts = Some(Shown { uid, families: families.clone(), indexing: false });
    Some(families)
}

/// Text ▸ Missing fonts (`dialog.missingFonts`): the window for the file in front.
pub fn request(app: &mut FigApp) {
    let Ok(uid) = app.session.ds().map(|d| d.uid) else { return };
    if figcraft_text::installed_fonts_known() {
        list(app, uid);
    } else {
        app.ui.missing_fonts = Some(Shown { uid, families: Vec::new(), indexing: true });
    }
}

pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    // Whether the installed fonts are indexed is asked only while a list waits for it.
    let waiting = app.ui.missing_fonts.as_ref().filter(|m| m.indexing).map(|m| m.uid);
    if (waiting.is_some() || !app.ui.font_checks.is_empty()) && figcraft_text::installed_fonts_known() {
        if let Some(uid) = waiting {
            list(app, uid);
        }
        if !app.ui.font_checks.is_empty() && app.ui.import_report.is_none() && app.ui.missing_fonts.is_none() {
            let uid = app.ui.font_checks.remove(0);
            if list(app, uid).is_some_and(|f| f.is_empty()) {
                app.ui.missing_fonts = None;
            }
        }
    }
    if app.ui.missing_fonts.as_ref().is_some_and(|m| !app.session.docs.iter().any(|d| d.uid == m.uid)) {
        app.ui.missing_fonts = None;
    }
    let Some(shown) = app.ui.missing_fonts.clone() else { return };
    let t = Tokens::get(ctx);
    let mut open = true;
    let mut close_clicked = false;
    crate::dialogs::window(tr("Missing fonts"), ctx).open(&mut open).show(ctx, |ui| {
        ui.set_width(380.0);
        // Installing is the first thing this window is for, so the button comes first.
        if app.fonts_dir().is_some() && crate::widgets::button(ui, tr("Install font…"), true, true).clicked() {
            install_dialog(app);
        }
        ui.add_space(10.0);
        if shown.indexing {
            ui.label(tr("Checking the fonts installed on this computer…"));
        } else if shown.families.is_empty() {
            ui.label(tr("All the fonts this file uses are installed"));
        } else {
            ui.label(
                RichText::new(tr(
                    "This file uses fonts that aren't installed on this computer. Their text is drawn in a fallback font until you install them.",
                ))
                .color(t.text_secondary)
                .font(regular(11.0)),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical().id_salt("missing_fonts").max_height(260.0).show(ui, |ui| {
                for m in &shown.families {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&m.family).strong());
                        let used =
                            if m.layers == 0 { tr("only in a text style or variable").to_string() } else { trn("{n} text layers", m.layers, &[]) };
                        ui.label(RichText::new(used).color(t.text_secondary));
                    });
                }
            });
        }
        ui.add_space(12.0);
        close_clicked = crate::widgets::button(ui, tr("Close"), false, true).clicked();
    });
    if !open || close_clicked {
        app.ui.missing_fonts = None;
    }
}

/// Pick a font file to install (the Typography section and the Missing fonts window).
pub fn install_dialog(app: &mut FigApp) {
    app.pick_open(tr("Install font…"), FONT_EXTS, install);
}

/// Copy `path` into the app's font folder, load it (every face of a collection) and list the open
/// document's missing fonts again. The copy stays for the next run.
fn install(app: &mut FigApp, path: PathBuf) {
    let Some(dir) = app.fonts_dir().map(Path::to_path_buf) else {
        app.show_error(tr("This build can't keep installed fonts.").to_string());
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        install_failed(app, e);
        return;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() > figcraft_text::fontdb::MAX_FONT_FILE) {
        app.show_error(tr("This file is too large for a font (over 256 MB).").to_string());
        return;
    }
    let name = path.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("font.ttf"));
    // Load from a staging file in the folder, then move it to its final name: a file that isn't a
    // font leaves nothing behind, and the document refits once (`text.loadFont`).
    let tmp = dir.join(format!(".{}.import", name.to_string_lossy()));
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = std::fs::copy(&path, &tmp) {
        install_failed(app, e);
        return;
    }
    let Some(result) = app.run("text.loadFont", json!({"path": tmp.to_string_lossy()})) else {
        let _ = std::fs::remove_file(&tmp);
        return;
    };
    let families: Vec<String> = result
        .get("families")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let kept = match keep(&dir, &name, &tmp) {
        Ok(dest) => {
            log::info!("fonts: installed {} as {}", path.display(), dest.display());
            true
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            app.show_error(trf("The font is loaded for now, but couldn't be kept: {error}", &[("error", &e)]));
            false
        }
    };
    crate::font_preview::clear();
    let family = families.join(", ");
    if kept && !family.is_empty() {
        app.toast(trf("Installed {name}", &[("name", &family)]));
    }
    let uid = app.ui.missing_fonts.as_ref().map(|m| m.uid);
    if let Some(uid) = uid {
        list(app, uid);
    }
}

/// Show what kept a font file from being installed.
fn install_failed(app: &mut FigApp, error: impl std::fmt::Display) {
    app.show_error(trf("Couldn't install the font: {error}", &[("error", &error)]));
}

/// Move the loaded staging file to `dir/name`; if a different font is there already, use the first
/// free `name-2.ext` so an older font isn't lost. A file with the same bytes is kept once.
fn keep(dir: &Path, name: &Path, tmp: &Path) -> std::io::Result<PathBuf> {
    let dest = dir.join(name);
    if !dest.exists() {
        std::fs::rename(tmp, &dest)?;
        return Ok(dest);
    }
    if same_bytes(tmp, &dest) {
        let _ = std::fs::remove_file(tmp);
        return Ok(dest);
    }
    let stem = name.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "font".into());
    let ext = name.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "ttf".into());
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem}-{n}.{ext}"));
        if !candidate.exists() {
            std::fs::rename(tmp, &candidate)?;
            return Ok(candidate);
        }
        if same_bytes(tmp, &candidate) {
            let _ = std::fs::remove_file(tmp);
            return Ok(candidate);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "too many fonts with that name"))
}

/// Whether two files hold the same bytes (size first: a font can be large).
fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => match (std::fs::read(a), std::fs::read(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
        _ => false,
    }
}
