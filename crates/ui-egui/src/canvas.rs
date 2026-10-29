//! The canvas: the rendered page plus selection, hover, guides, labels and text-editing overlays.

use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::Preedit;
use figcraft_engine::tools::{PointerEvent, PointerKind, ToolId, selection_box, selection_frame};
use figcraft_engine::vectoredit::VHit;
use figcraft_geom::Point;
use serde_json::json;

use crate::FigApp;
use crate::i18n::{tr, trf, trn};
use crate::render_worker::{Done, Job, Worker};
use crate::theme::{COMPONENT, Tokens, doc_c32, regular, semibold};

mod builder;
mod input;
mod outline;
mod overlays;
mod render;
mod rulers;

#[cfg(test)]
use builder::clean_poly;
use input::input;
pub use input::open_context_menu;
pub(crate) use outline::{TextOnScreen, node_quad};
use overlays::{accent, overlays};
use render::{JobKey, request_render, shows};
pub use rulers::fmt;
pub(crate) use rulers::{status_badge, status_label};
pub mod board_bar;
pub mod board_facilitate;
pub mod comments;
pub mod cursors;
pub mod frame_labels;
pub mod frame_stats;
pub mod guides;
pub mod handles;
pub mod live_text;
pub mod measure_overlay;
pub mod proto_canvas;
pub mod render_worker;
pub(crate) mod selection_spacing;
pub mod widget_bar;

pub struct CanvasState {
    pub rect: Rect,
    texture: Option<egui::TextureHandle>,
    tex_view: Option<figcraft_render::View>,
    /// The file ([`figcraft_engine::DocState::uid`]) and page the texture shows: it is drawn only
    /// on them.
    tex_of: Option<(u64, NodeId)>,
    /// The texture is a coarse preview (see [`crate::render_worker`]).
    pub tex_coarse: bool,
    /// The request the texture answers ([`crate::render_worker::Done::seq`]).
    tex_seq: u64,
    submitted: Option<JobKey>,
    /// The view of the last render asked for.
    want_view: Option<figcraft_render::View>,
    /// What the last canvas render had to do (shown by `ui.inspect`).
    pub render_work: Option<figcraft_render::Work>,
    /// From asking for a render to its preview, and its refined image, handed to egui as the
    /// canvas texture (ms; the texture is uploaded when that frame is painted).
    pub preview_latency_ms: Option<f64>,
    pub refine_latency_ms: Option<f64>,
    dragging: bool,
    /// The magnifier shown over the finger while a vector point or handle is dragged (touch).
    pub loupe: Option<Loupe>,
    space_pan: bool,
    middle_pan: Option<(Pos2, f64, f64)>,
    /// The zoom tool (Z held): zooming out (Alt) or in, and where its drag started.
    zoom_tool: Option<bool>,
    zoom_drag: Option<Pos2>,
    pub last_pointer: Option<Point>,
    /// The real pointer was over the canvas last frame (leaving it clears what it hovered).
    hovering: bool,
    /// What the pointer shows over the canvas (`ui.inspect`); none while it is elsewhere or
    /// something on the canvas with a cursor of its own has it (a ruler guide, a gradient handle).
    pub cursor: Option<crate::cursors::Shown>,
    preedit: Option<PreeditView>,
    /// The pen pressure last reported during the current press (pen and stylus touches; the
    /// winit backends and browsers that report it).
    pressure: Option<f32>,
    /// Fingers that were on the canvas last frame: two or more make a drag pan and zoom at
    /// once (`gestos_tactiles`).
    pub touch_count: usize,
    /// A finger held still on the canvas since it started: the point it started at and how far it
    /// has travelled since. Holding it still opens the context menu, the long press that stands
    /// for right-click on a touch screen (`pulsacion_larga`).
    ///
    /// MEDIDO que son **puntos recorridos acumulados** y no un instante: a finger resting does
    /// not produce events, so a clock would not advance.
    long_press: Option<(Pos2, f32)>,
    /// The taps on the canvas, for the double tap (`doble_toque`).
    toque: Toque,
}

/// The magnifier shown over the finger while a point or handle is dragged in vector edit mode.
///
/// It shows the same canvas texture the page is drawn from, at a few times its scale, so it costs
/// no extra render and includes whatever is under the point: a reference image, another layer, the
/// page's background. The point itself is marked with a crosshair at the centre.
#[derive(Clone, Copy, Debug)]
pub struct Loupe {
    /// The edited layer and the point or handle being dragged.
    id: NodeId,
    hit: VHit,
    /// The finger's screen position: the loupe is placed above it.
    at: Pos2,
}

/// A tap on the canvas, for the double tap (`doble_toque`).
#[derive(Default)]
struct Toque {
    /// Where and when the finger went down, while it is down.
    abajo: Option<(Pos2, f64)>,
    /// How far it has moved from `abajo.0`, in points.
    movido: f32,
    /// Where and when the last clean tap started and ended.
    ultimo: Option<(Pos2, f64)>,
    /// The second tap of a double tap: its whole touch is swallowed, so the tool sees no click
    /// or drag for it.
    tragar: bool,
}

/// What `doble_toque` found in this frame's touch events.
enum ToqueAhora {
    Nada,
    Doble(Pos2),
    /// The second tap of a double tap is still down: consume its frames.
    Tragado,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            rect: Rect::NOTHING,
            texture: None,
            tex_view: None,
            tex_of: None,
            tex_coarse: false,
            tex_seq: 0,
            submitted: None,
            want_view: None,
            render_work: None,
            preview_latency_ms: None,
            refine_latency_ms: None,
            dragging: false,
            loupe: None,
            space_pan: false,
            middle_pan: None,
            cursor: None,
            zoom_tool: None,
            zoom_drag: None,
            last_pointer: None,
            hovering: false,
            preedit: None,
            pressure: None,
            touch_count: 0,
            long_press: None,
            toque: Toque::default(),
        }
    }
}

/// What the canvas shows while an IME composition is in progress: the document with the
/// composition inserted at the caret (the real document only gets it on commit).
pub struct PreeditView {
    /// The document, composition and caret it was built from.
    key: (Arc<Document>, Preedit, usize),
    pub doc: Arc<Document>,
    /// The composition's char range, and the IME cursor, in `doc`'s edited text.
    pub range: (usize, usize),
    pub cursor: usize,
}

impl FigApp {
    fn preedit_key(&self) -> Option<(Arc<Document>, Preedit, usize)> {
        let ds = self.session.ds().ok()?;
        let te = ds.text_edit.as_ref()?;
        Some((ds.doc.clone(), te.preedit.clone()?, te.caret))
    }

    /// Rebuild the composition view when the document or composition changed.
    pub fn sync_preedit(&mut self) {
        if self.preedit_view().is_some() {
            return;
        }
        self.canvas.preedit = self.preedit_key().and_then(|key| {
            let (doc, range, cursor) = figcraft_engine::textedit::preedit_doc(&self.session)?;
            Some(PreeditView { key, doc: Arc::new(doc), range, cursor })
        });
    }

    /// The composition view, when it matches the current state.
    pub fn preedit_view(&self) -> Option<&PreeditView> {
        let p = self.canvas.preedit.as_ref()?;
        let (doc, pe, caret) = self.preedit_key()?;
        (Arc::ptr_eq(&p.key.0, &doc) && p.key.1 == pe && p.key.2 == caret).then_some(p)
    }
}

impl FigApp {
    /// Page → screen (logical points).
    pub fn to_screen(&self, x: f64, y: f64) -> Pos2 {
        let Ok(ds) = self.session.ds() else { return Pos2::ZERO };
        let (sx, sy) = ds.view.page_to_screen(x, y);
        pos2(self.canvas.rect.min.x + sx as f32, self.canvas.rect.min.y + sy as f32)
    }
    pub fn to_page(&self, p: Pos2) -> Point {
        let Ok(ds) = self.session.ds() else { return Point::ZERO };
        let (x, y) = ds.view.screen_to_page(f64::from(p.x - self.canvas.rect.min.x), f64::from(p.y - self.canvas.rect.min.y));
        Point::new(x, y)
    }
}

fn pointer_event(app: &FigApp, kind: PointerKind, pos: Pos2, m: &egui::Modifiers) -> PointerEvent {
    let p = app.to_page(pos);
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0);
    PointerEvent {
        kind,
        x: p.x,
        y: p.y,
        sx: f64::from(pos.x),
        sy: f64::from(pos.y),
        shift: m.shift,
        alt: m.alt,
        cmd: m.command,
        ctrl: m.ctrl,
        zoom,
        pressure: app.canvas.pressure,
    }
}

/// What a press at `pos` grabs in vector edit mode, if it is a point or a handle: the magnifier
/// follows those while they are dragged. The Move tool only: the other edit tools drag segments,
/// regions or the whole shape, where the point under the finger is not what is being placed.
pub(super) fn loupe_target(app: &FigApp, pos: Pos2) -> Option<Loupe> {
    let ds = app.session.ds().ok()?;
    let ve = ds.vector_edit.as_ref()?;
    if ve.tool != figcraft_engine::vectoredit::VTool::Move {
        return None;
    }
    let (id, hit) = figcraft_engine::vectoredit::hit_any(&ds.doc, ve, app.to_page(pos), figcraft_engine::vectoredit::hit_tol(ds.view.zoom))?;
    matches!(hit, VHit::Vertex { .. } | VHit::Handle { .. }).then_some(Loupe { id, hit, at: pos })
}

/// The page point the magnifier shows: the dragged point or handle in the document as it is now
/// (the preview during the drag, so snapping is included).
fn loupe_center(app: &FigApp, l: &Loupe) -> Option<Point> {
    let ds = app.session.ds().ok()?;
    let net = figcraft_engine::vectoredit::network(&ds.doc, l.id)?;
    let local = match l.hit {
        VHit::Vertex { index } => net.vertices.get(index)?.point(),
        VHit::Handle { seg, start } => {
            let s = net.segments.get(seg)?;
            let v = net.vertices.get(if start { s.a } else { s.b })?;
            let t = net.handle(seg, start)?;
            Point::new(v.x + t.0, v.y + t.1)
        }
        VHit::Segment { .. } => return None,
    };
    Some(ds.doc.world_transform(l.id) * local)
}

/// Side of the magnifier, in points.
const LOUPE_SIZE: f32 = 150.0;
/// How much the magnifier enlarges the canvas.
const LOUPE_MAG: f32 = 3.0;

/// Where the magnifier goes: above the finger, below it when there is no room, and inside the
/// canvas.
fn loupe_rect(at: Pos2, canvas: Rect, size: f32) -> Rect {
    let margin = 8.0;
    let gap = 28.0;
    let max_size = (canvas.width().min(canvas.height()) - 2.0 * margin).max(24.0);
    let size = size.min(max_size);
    let mut r = Rect::from_center_size(pos2(at.x, at.y - gap - size / 2.0), vec2(size, size));
    if r.top() < canvas.top() + margin {
        r = Rect::from_center_size(pos2(at.x, at.y + gap + size / 2.0), vec2(size, size));
    }
    let min = canvas.min + vec2(margin, margin);
    let max = canvas.max - vec2(margin, margin);
    let dx = if r.left() < min.x {
        min.x - r.left()
    } else if r.right() > max.x {
        max.x - r.right()
    } else {
        0.0
    };
    let dy = if r.top() < min.y {
        min.y - r.top()
    } else if r.bottom() > max.y {
        max.y - r.bottom()
    } else {
        0.0
    };
    r.translate(vec2(dx, dy))
}

/// The magnifier over the finger: the canvas texture around the dragged point at [`LOUPE_MAG`],
/// with a crosshair on the point. It uses the texture already on screen, so a reference image and
/// every other layer are included, and it costs no extra render.
fn draw_loupe(app: &FigApp, painter: &egui::Painter, rect: Rect, ppp: f32) {
    let Some(l) = app.canvas.loupe else { return };
    let Some(center) = loupe_center(app, &l) else { return };
    let shown = app.session.ds().map(|ds| (ds.uid, ds.page)).ok();
    let (Some(tex), Some(v), true) = (&app.canvas.texture, &app.canvas.tex_view, shown.is_some() && app.canvas.tex_of == shown) else {
        return;
    };
    let tl = app.to_screen(v.origin.0, v.origin.1);
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0);
    let pixel_preview = app.session.ds().ok().and_then(|ds| ds.view.pixel_preview_scale()) == Some(v.scale);
    let image_rect = presented_image_rect(tl, v, zoom, ppp, pixel_preview);
    let at = app.to_screen(center.x, center.y);
    let uv_center = ((at - image_rect.min) / image_rect.size()).to_pos2();
    let r = loupe_rect(l.at, rect, LOUPE_SIZE);
    let half = r.width() / (2.0 * LOUPE_MAG);
    let uv_half = vec2(half / image_rect.width(), half / image_rect.height());
    let uv = Rect::from_center_size(uv_center, uv_half * 2.0);
    let radius = CornerRadius::same(16);
    painter.rect_filled(r.translate(vec2(0.0, 3.0)).expand(2.0), radius, Color32::from_black_alpha(80));
    painter.add(egui::epaint::RectShape::filled(r, radius, Color32::WHITE).with_texture(tex.id(), uv));
    painter.rect_stroke(r, radius, Stroke::new(2.0, Color32::from_white_alpha(235)), StrokeKind::Inside);
    painter.rect_stroke(r.expand(1.0), radius, Stroke::new(1.0, Color32::from_black_alpha(90)), StrokeKind::Outside);
    let c = r.center();
    let shadow = Stroke::new(3.0, Color32::from_black_alpha(150));
    let light = Stroke::new(1.0, Color32::WHITE);
    for (a, b) in [(vec2(-9.0, 0.0), vec2(9.0, 0.0)), (vec2(0.0, -9.0), vec2(0.0, 9.0))] {
        painter.line_segment([c + a, c + b], shadow);
        painter.line_segment([c + a, c + b], light);
    }
    painter.circle_stroke(c, 6.0, shadow);
    painter.circle_stroke(c, 6.0, light);
}

/// Run one canvas pass: update the viewport size and first-fit zoom; handle pointer/key input
/// and accessibility; draw the drop overlay, guides, live pointer and preedit; request/poll
/// renders and upload the texture; draw the page background and last texture; then draw overlays,
/// labels, handles, comments, versions/branch/dev/live UI and the cursor.
pub fn show(app: &mut FigApp, ui: &mut Ui, worker: &mut Worker) {
    let rect = ui.max_rect();
    app.canvas.rect = rect;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let resp = ui.interact(rect, ui.scope_id().with("canvas"), Sense::click_and_drag());
    let key_box = app.ui.key_box;
    crate::a11y::canvas(&mut app.ui.nav, &resp, || key_box.map_or_else(|| describe(&app.session), describe_box));
    if let Ok(ds) = app.session.ds_mut()
        && ((ds.view.vw - f64::from(rect.width())).abs() > 0.5 || (ds.view.vh - f64::from(rect.height())).abs() > 0.5)
    {
        // Keep the centre; the first sizing after opening re-fits.
        let first = ds.view.vw == 1200.0 && ds.view.vh == 800.0;
        ds.view.vw = f64::from(rect.width());
        ds.view.vh = f64::from(rect.height());
        if first {
            ds.zoom_to_fit();
        }
    }
    let own_cursor = input(app, ui, &resp);
    // A browser names no dragged file until the drop, so documents there still say "import", or
    // in Dev Mode show the Dev Mode note, though they open when dropped.
    let documents = ctx.input(|i| i.raw.hovered_files.iter().all(|f| f.path.as_deref().is_some_and(crate::is_document)));
    let label = if documents {
        tr("Drop to open")
    } else if app.ui.right_tab == crate::RightTab::Inspect {
        tr("Dev Mode only inspects: switch to Design (Shift+D) to edit")
    } else {
        tr("Drop to import")
    };
    crate::drop_overlay(&ctx, ctx.content_rect(), label);
    crate::guides::end_released(&ctx);
    crate::live::track_pointer(app, &ctx);
    app.sync_preedit();
    // Retained rendering snaps the view outward and adds one pixel on each axis.
    let max_texture_side = (ctx.input(|i| i.max_texture_side).min(16_384) as u32).saturating_sub(1).max(1);
    let t = Tokens::get(&ctx);
    // Page background (also behind a stale/partial texture).
    let bg = app
        .session
        .ds()
        .ok()
        .and_then(|ds| match ds.doc.node(ds.page).map(|n| &n.kind) {
            Some(NodeKind::Page { background, .. }) => Some(doc_c32(&ctx, *background)),
            _ => None,
        })
        .unwrap_or(t.canvas);
    let dots = crate::board_bar::dots(app, bg, &t);
    request_render(app, worker, ppp, max_texture_side, dots);
    // Every finished render is shown, even when a newer one is already on its way: renders finish
    // in order, and an edit every frame (a drag) would otherwise never show until it stopped.
    let shown = app.session.ds().map(|ds| (ds.uid, ds.page)).ok();
    if let Some(done) = worker.poll()
        && shows(&app.canvas, &done, shown)
    {
        if !done.coarse || done.gesture {
            app.canvas.render_work = Some(done.work);
            app.ui.render_ms = done.ms;
        }
        // A pixel preview render is magnified with hard pixel edges.
        let filter = if app.session.ds().ok().and_then(|d| d.view.pixel_preview_scale()) == Some(done.view.scale) {
            egui::TextureOptions::NEAREST
        } else {
            egui::TextureOptions::LINEAR
        };
        match &mut app.canvas.texture {
            Some(t) => t.set(done.image, filter),
            None => app.canvas.texture = Some(ctx.load_texture("figcraft-canvas", done.image, filter)),
        }
        let latency = Some(done.at.elapsed().as_secs_f64() * 1000.0);
        if done.gesture {
            // Ni preview ni refine: es la imagen que sigue al dedo.
        } else if done.coarse {
            app.canvas.preview_latency_ms = latency;
        } else {
            app.canvas.refine_latency_ms = latency;
        }
        app.canvas.tex_view = Some(done.view);
        app.canvas.tex_of = Some((done.uid, done.page));
        app.canvas.tex_coarse = done.coarse;
        app.canvas.tex_seq = done.seq;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, bg);
    crate::theme::set_canvas_page(&ctx, bg);
    if let (Some(tex), Some(v), true) = (&app.canvas.texture, &app.canvas.tex_view, shown.is_some() && app.canvas.tex_of == shown) {
        // Re-project the last render onto the current view.
        let tl = app.to_screen(v.origin.0, v.origin.1);
        let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0);
        let pixel_preview = app.session.ds().ok().and_then(|ds| ds.view.pixel_preview_scale()) == Some(v.scale);
        let image_rect = presented_image_rect(tl, v, zoom, ppp, pixel_preview);
        painter.image(tex.id(), image_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    crate::dev_mode::refresh_measurements(app);
    app.session.tool.labels = overlays(app, &painter, &t);
    crate::frame_labels::rename_field(app, ui);
    // The zoom tool's area.
    if let (Some(start), Some(p)) = (app.canvas.zoom_drag, ctx.input(|i| i.pointer.hover_pos())) {
        let area = Rect::from_two_pos(start, p);
        if area.width() > 4.0 || area.height() > 4.0 {
            painter.rect_filled(area, CornerRadius::ZERO, accent(&painter).gamma_multiply(0.1));
            painter.rect_stroke(area, CornerRadius::ZERO, Stroke::new(1.0, accent(&painter)), StrokeKind::Inside);
        }
    }
    crate::dev_mode::draw(app, &painter);
    crate::handles::draw(app, &painter);
    crate::board_facilitate::badges(app, &painter);
    crate::board_bar::stamp_author(app, &painter, ctx.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)));
    crate::board_bar::show(app, &ctx);
    crate::board_facilitate::show(app, &ctx, rect);
    crate::widget_bar::show(app, &ctx);
    crate::selection_spacing::draw(app, &painter);
    crate::comments::draw(app, &painter);
    crate::live_text::draw(app, &painter);
    crate::live::draw(app, &painter, &ctx);
    crate::live::chat_box(app, &ctx);
    crate::live::spotlight_banner(app, &ctx, rect);
    crate::comments::popovers(app, &ctx);
    crate::versions::preview_banner(app, &ctx, rect);
    crate::branch_ui::banner(app, &ctx, rect);
    crate::dev_mode::focus_bar(app, &ctx, rect);
    draw_loupe(app, &painter, rect, ppp);
    // The cursor: the tool's, unless something on the canvas with its own has the pointer.
    app.canvas.cursor = None;
    if let (true, Some(at)) = (own_cursor && resp.hovered(), ctx.input(|i| i.pointer.hover_pos())) {
        let panning = app.canvas.space_pan || app.canvas.middle_pan.is_some();
        let shown = crate::cursors::on_canvas(panning, ctx.input(|i| i.pointer.any_down()), app.canvas.zoom_tool, app.session.tool.overlay.cursor);
        shown.show(&ctx, at);
        app.canvas.cursor = Some(shown);
    }
}

/// The image's screen rect; align native-density renders to device pixels to avoid linear filtering
/// blending neighbouring texels. Stale and pixel-preview renders retain their original projection.
pub(super) fn presented_image_rect(tl: Pos2, view: &figcraft_render::View, zoom: f64, ppp: f32, pixel_preview: bool) -> Rect {
    let w = (f64::from(view.width) / view.scale * zoom) as f32;
    let h = (f64::from(view.height) / view.scale * zoom) as f32;
    let native_density = !pixel_preview
        && ppp.is_finite()
        && ppp > 0.0
        && zoom.is_finite()
        && view.scale.is_finite()
        && view.scale > 0.0
        && (view.scale - zoom * f64::from(ppp)).abs() <= view.scale.abs().max(1.0) * 1e-9;
    let min = if native_density { pos2((tl.x * ppp).round() / ppp, (tl.y * ppp).round() / ppp) } else { tl };
    Rect::from_min_size(min, vec2(w, h))
}

/// What the canvas says to screen readers: the selected layer ("Rectangle 1, Rectangle, 120 by 80,
/// selected"), how many are selected, or the page with nothing selected.
pub fn describe(s: &figcraft_engine::Session) -> String {
    let Ok(ds) = s.ds() else { return String::new() };
    match ds.selection.as_slice() {
        [] => trf("{page}, nothing selected", &[("page", &ds.doc.node(ds.page).map(|p| p.name.as_str()).unwrap_or(tr("Page")))]),
        [id] => match ds.doc.node(*id) {
            Some(n) => {
                let kind = tr(n.kind.label());
                let name = if n.name.is_empty() { kind } else { n.name.as_str() };
                let (w, h) = (crate::widgets::fmt_num(n.width), crate::widgets::fmt_num(n.height));
                trf("{name}, {kind}, {w} by {h}, selected", &[("name", &name), ("kind", &kind), ("w", &w), ("h", &h)])
            }
            None => String::new(),
        },
        many => trn("{n} layers selected", many.len(), &[]),
    }
}

/// What the canvas says while a keyboard selection box shows: its size and place, and the keys.
fn describe_box([x0, y0, x1, y1]: [f64; 4]) -> String {
    let n = crate::widgets::fmt_num;
    let (w, h, x, y) = (n((x1 - x0).abs()), n((y1 - y0).abs()), n(x0.min(x1)), n(y0.min(y1)));
    let cmd = crate::toolbar::pretty_shortcut("Cmd");
    trf(
        "Selection box, {w} by {h} at {x}, {y}: arrows move it, Shift+arrows or {cmd}+arrows size it, Enter selects, Escape cancels",
        &[("w", &w), ("h", &h), ("x", &x), ("y", &y), ("cmd", &cmd)],
    )
}

#[cfg(test)]
mod builder_poly_tests;

#[cfg(test)]
mod loupe_tests;

#[cfg(test)]
mod zoom_preview_tests;
