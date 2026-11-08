//! The Missing fonts window (#752): the fonts a file uses that aren't installed, whose text is
//! drawn in a fallback font. It opens by itself after the user opens a file that has any (once
//! the installed fonts are indexed and any import report is closed), and from Text ▸ Missing fonts.
//!
//! A family the font store has (Google Fonts, Noto) gets a Download button (#754): the first
//! one asks the user to turn downloading on, then `font.download` installs it on a thread of its
//! own, and once it's installed the file's text fits the real font and the family leaves the list.

use std::path::{Path, PathBuf};

use egui::RichText;
use figcraft_engine::fonts::{Download, Missing, allow_downloads, downloads, downloads_allowed, missing};
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
    /// The family whose Download asks the user to turn downloading fonts on.
    pub consent: Option<String>,
}

impl Shown {
    pub fn new(uid: u64, families: Vec<Missing>, indexing: bool) -> Self {
        Self { uid, families, indexing, consent: None }
    }

    pub fn inspect(&self) -> Value {
        json!({
            "families": self.families.iter().map(|m| json!({"family": m.family, "layers": m.layers, "source": m.source})).collect::<Vec<_>>(),
            "indexing": self.indexing,
            "consent": self.consent,
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
    let consent = app.ui.missing_fonts.as_mut().filter(|m| m.uid == uid).and_then(|m| m.consent.take());
    app.ui.missing_fonts = Some(Shown { consent, ..Shown::new(uid, families.clone(), false) });
    Some(families)
}

/// Text ▸ Missing fonts (`dialog.missingFonts`): the window for the file in front.
pub fn request(app: &mut FigApp) {
    let Ok(uid) = app.session.ds().map(|d| d.uid) else { return };
    if figcraft_text::installed_fonts_known() {
        list(app, uid);
    } else {
        app.ui.missing_fonts = Some(Shown::new(uid, Vec::new(), true));
    }
}

pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    finish_downloads(app, ctx);
    list_when_known(app);
    let Some(shown) = &app.ui.missing_fonts else { return };
    let t = Tokens::get(ctx);
    let mut open = true;
    let mut clicks = Clicks::default();
    crate::dialogs::window(tr("Missing fonts"), ctx).open(&mut open).show(ctx, |ui| clicks = body(ui, &t, shown, app.fonts_dir().is_some()));
    let close = !open || clicks.close;
    act(app, clicks);
    if close {
        app.ui.missing_fonts = None;
    }
}

/// Lists the fonts of a window that waits for the installed fonts to be indexed, and of a file
/// opened since, once they are; a window whose file closed goes.
fn list_when_known(app: &mut FigApp) {
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
}

/// What the user clicked in the window this frame.
#[derive(Default)]
struct Clicks {
    /// The family whose Download (or Try again) was clicked.
    download: Option<String>,
    /// The answer to the consent prompt.
    consent: Option<bool>,
    /// Install a font file from disk (the port's local flow, beside `font.download`).
    install: bool,
    close: bool,
}

fn body(ui: &mut egui::Ui, t: &Tokens, shown: &Shown, can_install: bool) -> Clicks {
    let mut clicks = Clicks::default();
    ui.set_width(380.0);
    // Installing is the first thing this window is for, so the button comes first.
    if can_install && crate::widgets::button(ui, tr("Install font…"), true, true).clicked() {
        clicks.install = true;
    }
    if can_install {
        ui.add_space(10.0);
    }
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
        let jobs = downloads();
        egui::ScrollArea::vertical().id_salt("missing_fonts").max_height(260.0).show(ui, |ui| {
            for m in &shown.families {
                let job = jobs.iter().find(|(f, _)| f.eq_ignore_ascii_case(&m.family)).map(|(_, d)| d);
                if row(ui, t, m, job) {
                    clicks.download = Some(m.family.clone());
                }
            }
        });
    }
    if shown.consent.is_some() {
        clicks.consent = consent_prompt(ui, t);
    }
    ui.add_space(12.0);
    clicks.close = crate::widgets::button(ui, tr("Close"), true, true).clicked();
    clicks
}

/// Download starts at once while downloading is on, else asks the user first; their Allow turns
/// it on (the only place that does) and starts it.
fn act(app: &mut FigApp, clicks: Clicks) {
    if clicks.install {
        install_dialog(app);
    }
    if let Some(family) = clicks.download {
        if downloads_allowed() {
            start(app, &family);
        } else if let Some(shown) = app.ui.missing_fonts.as_mut() {
            shown.consent = Some(family);
        }
    }
    let Some(allow) = clicks.consent else { return };
    let Some(family) = app.ui.missing_fonts.as_mut().and_then(|m| m.consent.take()) else { return };
    if allow {
        match allow_downloads() {
            Ok(()) => start(app, &family),
            Err(e) => app.show_error(e),
        }
    }
}

/// One missing family: its name, where the file uses it, and for a family the font store has,
/// where it comes from and its Download button (its progress while it downloads, and why it
/// failed). Returns whether Download was clicked.
fn row(ui: &mut egui::Ui, t: &Tokens, m: &Missing, job: Option<&Download>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(&m.family).strong());
        let used = if m.layers == 0 { tr("only in a text style or variable").to_string() } else { trn("{n} text layers", m.layers, &[]) };
        ui.label(RichText::new(used).color(t.text_secondary));
        let Some(source) = &m.source else { return };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match job {
                Some(Download::Running { done, total }) => {
                    let percent = if *total == 0 { 0 } else { done.saturating_mul(100) / total };
                    ui.label(RichText::new(trf("Downloading… {percent}%", &[("percent", &percent)])).color(t.text_secondary));
                }
                Some(Download::Installed) => {}
                Some(Download::Failed(_)) => clicked = crate::widgets::button(ui, tr("Try again"), false, true).clicked(),
                None => clicked = crate::widgets::button(ui, tr("Download"), false, true).clicked(),
            }
            ui.label(RichText::new(source).color(t.text_tertiary).font(regular(11.0)));
        });
    });
    if let Some(Download::Failed(error)) = job {
        ui.add(egui::Label::new(RichText::new(error).color(t.danger).font(regular(11.0))).wrap());
    }
    clicked
}

/// Asks the user to turn downloading fonts on (the setting every Craft app shares): `Some(true)`
/// when they allow it, `Some(false)` for Not now.
fn consent_prompt(ui: &mut egui::Ui, t: &Tokens) -> Option<bool> {
    ui.add_space(10.0);
    crate::widgets::separator(ui);
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Allow downloading fonts?")).strong());
    ui.add(
        egui::Label::new(
            RichText::new(tr(
                "Free fonts come from Google Fonts and Noto, through jsDelivr and GitHub. Each file is checked against the font index before it is installed into the font folder the Craft apps share. Nothing is downloaded until you allow it.",
            ))
            .color(t.text_secondary)
            .font(regular(11.0)),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if crate::widgets::button(ui, tr("Allow and download"), true, true).clicked() {
            Some(true)
        } else if crate::widgets::button(ui, tr("Not now"), false, true).clicked() {
            Some(false)
        } else {
            None
        }
    })
    .inner
}

fn start(app: &mut FigApp, family: &str) {
    app.run("font.download", json!({"family": family}));
}

/// A download that finished fits the open file's text to its font (`font.download` again) and
/// leaves the list; one under way repaints the window as it progresses.
fn finish_downloads(app: &mut FigApp, ctx: &egui::Context) {
    let jobs = downloads();
    for (family, _) in jobs.iter().filter(|(_, d)| *d == Download::Installed) {
        start(app, family);
        if let Some(uid) = app.ui.missing_fonts.as_ref().filter(|m| !m.indexing).map(|m| m.uid) {
            list(app, uid);
        }
    }
    if jobs.iter().any(|(_, d)| matches!(d, Download::Running { .. })) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
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
