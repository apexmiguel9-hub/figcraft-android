use std::time::Instant;

use super::{FigApp, Point, PointerKind, Pos2, Rect, Toque, ToqueAhora, Ui, json, loupe_target, pointer_event};

/// What a hover was found in: the event (where, the modifiers, the zoom) and what decides what it
/// hits (the file and page, their revision, the selection, the tool and modes). The same again finds
/// the same, so it isn't sent again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HoverKey {
    event: figcraft_engine::tools::PointerEvent,
    doc: Option<(u64, figcraft_engine::doc::NodeId, u64)>,
    selection: Vec<figcraft_engine::doc::NodeId>,
    tool: figcraft_engine::ToolId,
    modes: [bool; 3],
}

impl HoverKey {
    fn of(app: &FigApp, event: figcraft_engine::tools::PointerEvent) -> Self {
        let ds = app.session.ds().ok();
        HoverKey {
            event,
            doc: ds.map(|d| (d.uid, d.page, d.revision)),
            selection: ds.map(|d| d.selection.clone()).unwrap_or_default(),
            tool: app.session.tool.tool,
            modes: [app.session.tool.dev_mode, ds.is_some_and(|d| d.vector_edit.is_some()), ds.is_some_and(|d| d.text_edit.is_some())],
        }
    }
}

/// The pointer left the canvas (or something over it took the pointer): nothing is hovered there.
fn left(app: &mut FigApp) {
    app.canvas.last_hover = None;
    app.session.pointer_left();
}

/// The zoom tool while Z is held (Alt: zoom out): a click zooms a step about the pointer, a drag
/// zooms to the dragged area. Takes the pointer from the other tools while it's active.
fn zoom_tool(app: &mut FigApp, ui: &Ui, resp: &egui::Response, pos: Pos2, mods: egui::Modifiers, typing: bool) -> bool {
    let held = !typing && !mods.command && ui.input(|i| i.key_down(egui::Key::Z));
    app.canvas.zoom_tool = held.then_some(mods.alt);
    let Some(start) = app.canvas.zoom_drag.or_else(|| (held && resp.hovered() && ui.input(|i| i.pointer.primary_pressed())).then_some(pos)) else {
        return held;
    };
    app.canvas.zoom_drag = Some(start);
    let area = Rect::from_two_pos(start, pos);
    if ui.input(|i| i.pointer.primary_down()) {
        return true;
    }
    app.canvas.zoom_drag = None;
    let (a, b) = (app.to_page(area.min), app.to_page(area.max));
    let params = if area.width() > 4.0 && area.height() > 4.0 {
        json!({"x": a.x, "y": a.y, "width": b.x - a.x, "height": b.y - a.y})
    } else {
        let p = app.to_page(pos);
        json!({"x": p.x, "y": p.y, "out": mods.alt})
    };
    app.run("view.zoomAt", params);
    true
}

/// The wheel's scroll and the zoom for this frame. With Preferences ▸ Scroll wheel zoom on, a
/// mouse wheel zooms and Cmd/Ctrl+wheel scrolls ([`swap_wheel`]); Shift+wheel still scrolls
/// sideways, and trackpads, pinches and touch screens are left as they are.
fn wheel_zoom(app: &mut FigApp, ctx: &egui::Context, zoom: f32, shift: bool) -> (egui::Vec2, f32) {
    let (wheel, latest, pinch) = ctx.input(|i| {
        let latest = i.events.iter().rev().find_map(|e| match e {
            egui::Event::MouseWheel { unit, source, .. } => Some(is_mouse_wheel(*source, *unit)),
            _ => None,
        });
        let pinch = i.multi_touch().is_some() || i.events.iter().any(|e| matches!(e, egui::Event::Zoom(_)));
        (i.smooth_scroll_delta, latest, pinch)
    });
    let speed = ctx.options(|o| o.input_options.scroll_zoom_speed);
    // egui spreads a wheel step over a few frames: the kind holds until the next scroll event.
    if let Some(mouse) = latest {
        app.canvas.wheel_is_mouse = mouse;
    }
    if !app.session.prefs.scroll_wheel_zoom || !app.canvas.wheel_is_mouse || pinch {
        return (wheel, zoom);
    }
    swap_wheel(wheel, zoom, shift, speed)
}

/// Whether a scroll event came from a mouse wheel. Only macOS says which device scrolled; elsewhere
/// a wheel scrolls by lines and a trackpad by points, which is the best guess there is.
pub(crate) fn is_mouse_wheel(source: egui::MouseWheelSource, unit: egui::MouseWheelUnit) -> bool {
    match source {
        egui::MouseWheelSource::Wheel => true,
        egui::MouseWheelSource::Unknown => unit != egui::MouseWheelUnit::Point,
        _ => false,
    }
}

/// Scroll wheel zoom on a mouse wheel's frame: egui reads Cmd/Ctrl+wheel as `zoom`, which becomes
/// a scroll down (across with Shift: `ViewState::scroll`), and a plain
/// `wheel` scroll becomes a zoom at egui's own Cmd/Ctrl+wheel rate (`speed`). Shift+wheel is
/// left a sideways scroll.
pub(crate) fn swap_wheel(wheel: egui::Vec2, zoom: f32, shift: bool, speed: f32) -> (egui::Vec2, f32) {
    if zoom > 0.0 && (zoom - 1.0).abs() > 1e-6 && speed > 0.0 {
        return (egui::vec2(0.0, zoom.ln() / speed), 1.0);
    }
    if !shift && wheel != egui::Vec2::ZERO {
        return (egui::Vec2::ZERO, (speed * (wheel.x + wheel.y)).exp());
    }
    (wheel, zoom)
}

fn import_dropped(app: &mut FigApp, ctx: &egui::Context, hover_pos: Option<egui::Pos2>) {
    let dropped = ctx.input(|i| i.raw.dropped_files.clone());
    let mut refused = false;
    // The first image goes to the colour picker that shows an image; the rest to the canvas.
    let mut well = crate::colorpicker::image_drop_target(app, ctx).map(|(_, params)| params);
    for f in dropped {
        let path = f.path().to_path_buf();
        if crate::is_document(&path) {
            crate::pending_open::open_dropped(app, &f);
            continue;
        }
        if refused || crate::menus::dev_mode_refuses(app, "file.import") {
            refused = true;
            continue;
        }
        let pt = hover_pos.map(|h| app.to_page(h)).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped.png".into());
        let (command, mut params) = match well.take() {
            Some(params) => ("image.replace", params),
            None => ("file.import", json!({"name": name, "x": pt.x, "y": pt.y})),
        };
        let read = if path.exists() {
            params["path"] = json!(path.to_string_lossy());
            params.as_object_mut().map(|o| o.remove("name"));
            None
        } else {
            Some(crate::dropped_bytes(&f, &name))
        };
        let Some(target) =
            app.session.ds().ok().map(|doc| crate::pending_open::ImportTarget { uid: doc.uid, page: doc.page, name: doc.doc.name.clone() })
        else {
            continue;
        };
        app.ui.pending_opens.push(crate::pending_open::PendingOpen { label: name, params, command, target: Some(target), read });
    }
}

/// Zoom (pinch de trackpad, Cmd/Ctrl+rueda) about the pointer; plain scroll pans.
///
/// MEDIDO en Android: separado de [`gestos_tactiles`] a proposito, para que el zoom de dos
/// dedos se aplique **una sola vez**. Este ve el raton y el trackpad; los dedos se resuelven
/// antes, en `gestos_tactiles`, que devuelve `true` cuando hay dos.
fn gestures_mouse(app: &mut FigApp, ui: &mut Ui, resp: &egui::Response, pos: Pos2, mods: egui::Modifiers) -> bool {
    let ctx = ui.ctx().clone();
    let (scroll, zoom_delta) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
    let (scroll, zoom_delta) = wheel_zoom(app, &ctx, zoom_delta, mods.shift);
    if !resp.hovered() {
        return false;
    }
    if (zoom_delta - 1.0).abs() > 1e-4 {
        let anchor = app.to_page(pos);
        if let Ok(ds) = app.session.ds_mut() {
            let z = ds.view.zoom * f64::from(zoom_delta);
            ds.view.set_zoom_at(z, (anchor.x, anchor.y));
            ds.revision += 1;
        }
    } else if scroll != egui::Vec2::ZERO
        && let Ok(ds) = app.session.ds_mut()
    {
        ds.view.scroll(f64::from(scroll.x), f64::from(scroll.y), mods.shift);
        ds.revision += 1;
    }
    false
}

/// Touch gestures on the canvas: **one finger is the tool, two fingers pan and zoom at once**,
/// and a finger held still opens the context menu (the long press that stands for a
/// right-click on a touch screen).
///
/// MEDIDO que hace falta decidirlo aqui, y no en cada panel: `egui-winit` **emula el raton
/// con un dedo** (`egui-winit-0.36.2/src/lib.rs:904`, *"emit PointerButton resp. PointerMoved
/// events to emulate mouse"*), y **solo para el primer puntero** (`:902`). Por eso un dedo ya
/// produce clic, doble clic y arrastre —que es justo lo que queremos para las herramientas— y
/// el segundo dedo **no** se emula: queda libre para hacer lo que en raton haria el boton
/// central, que es desplazar.
///
/// ## El orden pan y luego zoom, y por que importa
///
/// MEDIDO en el port de vectorcraft (su `PORT.md` §8), donde este bug costo lo mas caro del
/// port: con dos dedos el **zoom iba bien y el pan no**, y el pan solo aparecia si se dejaban
/// los dedos quietos un segundo y luego se deslizaban.
///
/// El motivo es que `before` y `after` asumian el mismo centro, y el pan lo rompia un frame
/// antes: `before − after` traia un `−d` que el `+d` del pan cancelaba exactamente, dejando
/// solo la correccion de zoom.
///
/// Aqui el arreglo es estructural, no un parche: **el punto de anclaje se calcula despues del
/// pan**, en este orden:
///
/// 1. pan con `translation_delta`
/// 2. `app.to_page(pos)` **despues** de mover la vista
/// 3. `set_zoom_at` con ese ancla ya fresca
///
/// Asi el pan nunca puede cancelarse, porque el zoom no sabe que hubo pan.
///
/// ## La pulsacion larga
///
/// MEDIDO que hace falta: sin ella no hay **clic derecho** en un tactil, y el menu contextual
/// del lienzo y de las filas del panel de capas quedan inalcanzables. egui no trae
/// pulsacion larga por su cuenta, asi que se mide aqui: un dedo quieto mas de
/// [`LONG_PRESS_SECS`] sin pasar de [`LONG_PRESS_MOVEMENT_PX`] abre el menu y se come el
/// gesto, para que el dedo al soltar no clickee tambien.
fn gestos_tactiles(app: &mut FigApp, ctx: &egui::Context, resp: &egui::Response, pos: Pos2) -> bool {
    if !resp.hovered() {
        return false;
    }

    // MEDIDO leyendo `egui-0.36.2/src/input_state/touch_state.rs:20`: `MultiTouchInfo` **solo
    // se crea con dos o mas dedos** — *"Value is ≥ 2 since for a single touch no
    // `MultiTouchInfo` is created"*—. Asi que la rama de aqui no es "dos dedos o mas" por
    // convenience: es **todo** lo que egui sabe de un gesto multiple, y con un dedo no llega
    // nada. Por eso la pulsacion larga se mide aparte, con `Event::Touch`.
    let Some(mt) = ctx.multi_touch() else {
        app.canvas.touch_count = 0;
        return false;
    };
    app.canvas.touch_count = mt.num_touches;

    // Dos dedos: pan y zoom a la vez, y el gesto se come el frame para que la herramienta no
    // vea un arrastre de un dedo que en realidad son dos.
    app.canvas.long_press = None;
    // MEDIDO: con dos dedos, `egui` tambien emula el raton del primer puntero
    // (`egui-winit/src/lib.rs:904`), asi que sin esto la herramienta dejaria un punto
    // seleccionado y un arrastre a medias.
    if app.canvas.dragging && !ctx.input(|i| i.pointer.primary_down()) {
        app.canvas.dragging = false;
    }

    let d = mt.translation_delta;
    // 1. El pan, primero.
    if d != egui::Vec2::ZERO
        && let Ok(ds) = app.session.ds_mut()
    {
        ds.view.scroll(f64::from(d.x), f64::from(d.y), false);
        ds.revision += 1;
    }
    // 2. El ancla, **despues** del pan.
    let anchor = app.to_page(pos);
    // 3. El zoom, con el ancla fresca. MEDIDO: en 0.36 `zoom_delta` es un **campo**, no un
    // metodo (`touch_state.rs:36`).
    let z = mt.zoom_delta;
    if (z - 1.0).abs() > 1e-4
        && let Ok(ds) = app.session.ds_mut()
    {
        let nueva = ds.view.zoom * f64::from(z);
        ds.view.set_zoom_at(nueva, (anchor.x, anchor.y));
        ds.revision += 1;
    }
    true
}

/// The second tap of a double tap, measured on `Event::Touch`.
///
/// MEDIDO por que no vale el doble clic de egui: lo decide en la **suelta**, y pide dos sueltas
/// en menos de `max_double_click_delay` = 0,3 s y a menos de `max_click_dist` = 6 px
/// (`egui-0.36.2/src/input_state/mod.rs:1165`). Un dedo tarda 400-600 ms entre toques y se mueve
/// 10-20 px, asi que en el movil el doble toque **no llegaba nunca**: por eso el editor de nodos
/// (doble toque en un objeto) era inalcanzable con el dedo. Aqui se mide el toque de verdad, con
/// margenes de dedo, y se traga el segundo toque entero para que la herramienta no vea ademas un
/// clic o un arrastre.
fn doble_toque(app: &mut FigApp, ctx: &egui::Context) -> ToqueAhora {
    // Dos dedos son pan y zoom, no un doble toque.
    if ctx.multi_touch().is_some() {
        app.canvas.toque = Toque::default();
        return ToqueAhora::Nada;
    }
    let ahora = ctx.input(|i| i.time);
    let toques: Vec<(egui::TouchPhase, Pos2)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Touch { phase, pos, .. } => Some((*phase, *pos)),
                _ => None,
            })
            .collect()
    });
    let mut doble = None;
    let mut tragado = false;
    for (phase, pos) in toques {
        match phase {
            egui::TouchPhase::Start => {
                app.canvas.toque.abajo = Some((pos, ahora));
                app.canvas.toque.movido = 0.0;
                if let Some((p0, t0)) = app.canvas.toque.ultimo
                    && ahora - t0 <= DOBLE_TOQUE_SECS
                    && pos.distance(p0) <= DOBLE_TOQUE_DIST
                {
                    doble = Some(pos);
                    // A third tap does not make a triple: the pair is spent.
                    app.canvas.toque.ultimo = None;
                    app.canvas.toque.abajo = None;
                    app.canvas.toque.tragar = true;
                }
            }
            egui::TouchPhase::Move => {
                if let Some((p0, _)) = app.canvas.toque.abajo {
                    app.canvas.toque.movido = app.canvas.toque.movido.max(p0.distance(pos));
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if app.canvas.toque.tragar {
                    // The second tap ends: swallowed, and the next one starts clean.
                    app.canvas.toque.tragar = false;
                    tragado = true;
                } else if let Some((p0, t0)) = app.canvas.toque.abajo {
                    // Only a short, still touch is half of a double tap; a drag or a hold is not.
                    app.canvas.toque.ultimo =
                        (phase == egui::TouchPhase::End && app.canvas.toque.movido <= TOQUE_MOVIMIENTO_MAX && ahora - t0 <= TOQUE_SECS_MAX)
                            .then_some((p0, ahora));
                }
                app.canvas.toque.abajo = None;
            }
        }
    }
    if let Some(p) = doble {
        return ToqueAhora::Doble(p);
    }
    if tragado || app.canvas.toque.tragar {
        return ToqueAhora::Tragado;
    }
    ToqueAhora::Nada
}

/// How far apart two taps can land (in points) and still be a double tap.
///
/// MEDIDO que son mas que los 6 px de egui y menos que un dedo de separacion: en el Moto (ppp
/// 2,44) son unos 78 px, el mismo sitio para un pulgar, y menos que dos objetos distintos.
const DOBLE_TOQUE_DIST: f32 = 32.0;

/// How long after the first tap ends the second one can start (seconds).
///
/// MEDIDO: egui pide 0,3 s entre **sueltas**; un doble toque humano ronda 400-600 ms entre
/// toques, y con 0,55 s el gesto entra sin abrir la puerta a dos toques seguidos sin querer.
const DOBLE_TOQUE_SECS: f64 = 0.55;

/// A tap that moves more than this (points) is a drag, not half of a double tap.
const TOQUE_MOVIMIENTO_MAX: f32 = 16.0;

/// A tap held longer than this (seconds) is a hold, not half of a double tap.
const TOQUE_SECS_MAX: f64 = 0.7;

/// The long press that stands for a right-click on a touch screen, and the flick/swipe to drag.
///
/// MEDIDO por que hace falta y no sale de egui: en un tactil no hay boton derecho, asi que
/// **el menu contextual es inalcanzable** —ni en el lienzo ni en las filas del panel de capas—
/// y con el se van media app y la mitad de los atajos.
///
/// MEDIDO que se mide con `Event::Touch` y no con `MultiTouchInfo`, y el motivo esta en el
/// codigo de egui: `MultiTouchInfo` **solo existe con dos o mas dedos**
/// (`touch_state.rs:20`), asi que para un dedo no hay nada que mirar ahi. Los eventos de toque
/// si llegan de uno en uno, con `TouchPhase::Start` / `Move` / `End` / `Cancel`.
///
/// MEDIDO que el umbral se cuenta **en eventos y no en tiempo**: los eventos tocan cuando
/// tocan, y un dedo quieto **no genera eventos**, asi que un reloj no avanzaria. Lo que se
/// cuenta son los `Moved` con el dedo lejos del punto de partida.
///
/// Returns true when the press was consumed (the menu opened), so the click is not also
/// fired when the finger lifts.
fn pulsacion_larga(app: &mut FigApp, ctx: &egui::Context) -> bool {
    if ctx.multi_touch().is_some() {
        // Dos dedos: eso es pan y zoom, no una pulsacion larga.
        app.canvas.long_press = None;
        return false;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        app.canvas.long_press = None;
        return false;
    };

    // MEDIDO: egui-winit emula el raton del primer dedo, asi que con un dedo
    // `i.pointer.any_pressed()` es true desde el primer frame. El `Event::Touch` es lo que
    // distingue el dedo de un raton de verdad.
    let toques: Vec<egui::Event> = ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Touch { .. })).cloned().collect());
    if toques.is_empty() {
        // Sin eventos de toque este frame: si no hay ni un dedo pulsado, se limpia.
        if !ctx.input(|i| i.pointer.any_down()) {
            app.canvas.long_press = None;
        }
        return false;
    }

    let mut sigue = false;
    for e in &toques {
        match e {
            egui::Event::Touch { phase: egui::TouchPhase::Start, pos: p, .. } => {
                app.canvas.long_press = Some((*p, 0.0));
                sigue = true;
            }
            egui::Event::Touch { phase: egui::TouchPhase::Move, pos: p, .. } => {
                sigue = true;
                if let Some((p0, movidos)) = app.canvas.long_press.as_mut() {
                    *movidos += (*p - *p0).length();
                }
            }
            egui::Event::Touch { phase: egui::TouchPhase::End, .. } | egui::Event::Touch { phase: egui::TouchPhase::Cancel, .. } => {
                app.canvas.long_press = None;
            }
            _ => {}
        }
    }
    if !sigue {
        return false;
    }

    // MEDIDO el criterio, y por que son estos dos numeros: el recorrido acumulado mientras el
    // dedo sigue apoyado, y un minimo de eventos. Un dedo que se mueve menos de
    // `LONG_PRESS_MOVEMENT_PX` puntos en total y ha generado al menos `LONG_PRESS_EVENTS`
    // eventos se considera quieto. Con 12 pt (unos 30 px en un movil de densidad 2,5) un dedo
    // tembloroso no dispara el menu; con 2, lo haria casi siempre.
    let Some((p0, movido)) = app.canvas.long_press else { return false };
    let eventos = ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Touch { .. })).count());
    if movido <= LONG_PRESS_MOVEMENT_PX && eventos >= LONG_PRESS_EVENTS {
        let dentro = app.canvas.rect.contains(pos) && pos.distance(p0) <= LONG_PRESS_MOVEMENT_PX;
        if dentro {
            // MEDIDO: se limpia **antes** de abrir el menu, para que al soltar el dedo no se
            // dispare tambien el clic. Y `dragging` a false para que el dedo que levanta no
            // termine un arrastre que nunca existio.
            app.canvas.long_press = None;
            app.canvas.dragging = false;
            app.canvas.loupe = None;
            // A long press is not half of a double tap.
            app.canvas.toque = Toque::default();
            open_context_menu(app, pos, false);
            return true;
        }
    }
    false
}

/// Cuanto puede recorrer el dedo (en puntos, en total) sin que deje de contar como pulsacion
/// larga.
///
/// MEDIDO que el valor es en **puntos**, no en pixeles: en un movil de densidad 2,5 son unos
/// 30 px, que es el margen que se deja para que el dedo no se vaya nunca del sitio.
const LONG_PRESS_MOVEMENT_PX: f32 = 12.0;

/// Cuantos eventos de toque hacen falta para dar por pulsacion larga.
///
/// MEDIDO que hace falta un minimo y no basta con "el dedo no se ha movido": en el frame
/// siguiente al toque egui sintetiza varios eventos del raton emulado, y con `>= 1` el menu
/// saltaria en el frame en que el dedo toca, antes de que el usuario haya pensado nada. Con
/// 4 eventos, el menu aparece tras un instante real de quietud.
const LONG_PRESS_EVENTS: usize = 4;

/// Put the magnifier away when no finger is on the canvas or a pinch starts, even if the release
/// happens off the canvas.
fn clear_loupe_if_done(app: &mut FigApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.pointer.primary_down()) || ctx.multi_touch().is_some() {
        app.canvas.loupe = None;
    }
}

/// Show the magnifier when a point or handle is grabbed on a touch screen and move it with the
/// finger. A mouse has a cursor and doesn't hide the point.
fn update_loupe(app: &mut FigApp, ctx: &egui::Context, kind: Option<PointerKind>, pos: Pos2, primary_down: bool) {
    match kind {
        Some(PointerKind::Down) if primary_down => {
            let touch = cfg!(target_os = "android") || ctx.input(|i| i.any_touches());
            app.canvas.loupe = if touch { loupe_target(app, pos) } else { None };
        }
        Some(PointerKind::Move) => {
            if let Some(l) = &mut app.canvas.loupe {
                l.at = pos;
            }
        }
        _ => {}
    }
}

/// The pointer and keys on the canvas. False when something on it with a cursor of its own has
/// the pointer (a comment pin, a ruler guide, a gradient or padding handle): the tool's cursor
/// then doesn't show.
pub(super) fn input(app: &mut FigApp, ui: &mut Ui, resp: &egui::Response) -> bool {
    if crate::proto_canvas::input(app, ui, resp) {
        // A prototype gesture is a press on the canvas too: the Pages list gives up the keyboard.
        app.ui.pages_focus = None;
        return true;
    }
    let ctx = ui.ctx().clone();
    clear_loupe_if_done(app, &ctx);
    // Un handle (gradiente, recorte, padding, blur) que ya está arrastrando tiene que ver la
    // suelta **antes** del corte por `hover_pos`. En tactil, egui-winit manda `PointerGone` al
    // levantar el dedo y `hover_pos` queda en `None`; sin esto, el drag se quedaba abierto y el
    // siguiente toque lo aplicaba en otro sitio.
    let handle_pos = ctx.input(|i| i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()));
    if let Some(hp) = handle_pos
        && crate::handles::is_dragging(&ctx)
        && crate::handles::input(app, &ctx, hp)
    {
        return false;
    }
    // El doble toque se mide **antes** del corte por `hover_pos`: al levantar el dedo
    // egui-winit manda `PointerGone` y `hover_pos` es `None`, y es justo la suelta la que arma
    // el toque. Debajo del corte no se armaba nunca y el doble toque no llegaba.
    match doble_toque(app, &ctx) {
        ToqueAhora::Doble(p) => {
            let mods = ctx.input(|i| i.modifiers);
            log::info!("DOBLE TOQUE {:.0},{:.0}", p.x, p.y);
            let ev = pointer_event(app, PointerKind::DoubleClick, p, &mods);
            if let Err(e) = app.session.pointer(&ev) {
                app.toast(e.to_string());
            }
            app.canvas.dragging = false;
            return true;
        }
        ToqueAhora::Tragado => return true,
        ToqueAhora::Nada => {}
    }
    // MEDIDO que aqui **no** se leen `smooth_scroll_delta` ni `zoom_delta`: los usa
    // `gestures_mouse`, que los toma de su propia lectura, para que el zoom de dos dedos se
    // aplique una sola vez y no aqui y alla.
    // `interact_pos` y no `hover_pos`: en tactil, al soltar, `hover_pos` es `None` pero
    // `interact_pos` conserva el punto del toque. Los dos son iguales con raton.
    let (mods, pointer_pos, space) =
        ctx.input(|i| (i.modifiers, i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()), i.key_down(egui::Key::Space)));
    let editing_text = app.session.ds().map(|d| d.text_edit.is_some()).unwrap_or(false);
    let typing_elsewhere = crate::a11y::field_focused(&ctx);
    app.canvas.space_pan = space && !editing_text && !typing_elsewhere;
    if !app.canvas.dragging && crate::selection_spacing::input(app, &ctx, pointer_pos, resp.hovered(), app.canvas.space_pan) {
        return false;
    }
    // Dropped files import onto the canvas, behind a card while they do ([`crate::pending_open`]);
    // a dropped document opens as a file of its own, as on the home screen. Dev Mode imports
    // nothing into the design, and says so once for the whole drop.
    import_dropped(app, &ctx, pointer_pos);
    let Some(pos) = pointer_pos.filter(|p| app.canvas.rect.contains(*p)) else {
        // A drag released off the canvas, or off the window, ends where the pointer was last seen.
        if app.canvas.dragging && !ctx.input(|i| i.pointer.primary_down()) {
            app.canvas.dragging = false;
            if let Some(at) = pointer_pos.or_else(|| app.canvas.last_pointer.map(|p| app.to_screen(p.x, p.y))) {
                send(app, PointerKind::Up, at, &mods);
            }
        }
        // Leaving the canvas: nothing is hovered there any more.
        if !app.canvas.dragging && std::mem::take(&mut app.canvas.hovering) {
            left(app);
        }
        // A pan whose button was released off the canvas is over.
        if !ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle) || i.pointer.primary_down()) {
            app.canvas.middle_pan = None;
        }
        // So is a handle drag.
        crate::handles::end_released_drag(app, &ctx);
        return true;
    };
    // A press on the canvas gives the keyboard back to it (arrows nudge again).
    if resp.hovered() && ctx.input(|i| i.pointer.any_pressed()) {
        app.ui.layers_focus = false;
        app.ui.pages_focus = None;
        app.ui.key_box = None;
    }
    // Zoom y desplazamiento del raton y del trackpad; los dedos se resuelven en
    // `gestos_tactiles`, que va justo despues.
    if gestures_mouse(app, ui, resp, pos, mods) {
        return true;
    }
    if gestos_tactiles(app, &ctx, resp, pos) {
        return true;
    }
    if zoom_tool(app, ui, resp, pos, mods, editing_text || typing_elsewhere) {
        return true;
    }

    // MEDIDO que va **despues** de los gestos de dos dedos y del zoom, y no antes: la
    // pulsacion larga es de **un** dedo, asi que con dos dedos ya se ha resuelto el frame y
    // aqui solo queda el dedo suelto.
    if pulsacion_larga(app, &ctx) {
        return true;
    }
    // Middle-button / space panning.
    let middle_down = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
    if (middle_down || (app.canvas.space_pan && ctx.input(|i| i.pointer.primary_down()))) && resp.hovered() || app.canvas.middle_pan.is_some() {
        if let Ok(ds) = app.session.ds_mut() {
            match app.canvas.middle_pan {
                None => app.canvas.middle_pan = Some((pos, ds.view.cx, ds.view.cy)),
                Some((p0, cx, cy)) => {
                    let z = ds.view.zoom.max(1e-6);
                    ds.view.cx = cx - f64::from(pos.x - p0.x) / z;
                    ds.view.cy = cy - f64::from(pos.y - p0.y) / z;
                    ds.revision += 1;
                }
            }
        }
        if !middle_down && !ctx.input(|i| i.pointer.primary_down()) {
            app.canvas.middle_pan = None;
        }
        return true;
    }
    // A handle drag whose button went up ends first, wherever on the canvas the pointer is: over a
    // ruler or a comment pin, which take the pointer below, or over something drawn above the
    // canvas (the colour picker, a card). The handle had the press, so it has the release too.
    if crate::handles::end_released_drag(app, &ctx) {
        return false;
    }
    if !app.canvas.dragging && (resp.hovered() || app.ui.comments.dragging()) && crate::comments::input(app, &ctx, pos) {
        return false;
    }
    if !app.canvas.dragging && resp.hovered() && crate::guides::input(app, &ctx, pos) {
        return false;
    }
    if !app.canvas.dragging && resp.hovered() && crate::handles::input(app, &ctx, pos) {
        return false;
    }
    if resp.secondary_clicked() {
        open_context_menu(app, pos, mods.command);
        return true;
    }
    // A third quick click is counted as a triple, not a double (a click elsewhere in between still
    // leaves its time in the sequence): it means the same here — edit, enter, open — unless a
    // vector or text is being edited, where a triple already has its own meaning. A triple on
    // empty canvas still means a double in vector edit mode: the double-click that entered edit
    // mode counts on, so leaving it right away would otherwise take a fourth click or a pause.
    let ds = app.session.ds().ok();
    let editing = ds.is_some_and(|d| d.vector_edit.is_some() || d.text_edit.is_some());
    // Only a triple click asks (it walks the parts' render bounds).
    let empty_canvas = || {
        ds.is_some_and(|d| {
            let Some(ve) = d.vector_edit.as_ref().filter(|v| v.anchor.is_none() && d.text_edit.is_none()) else {
                return false;
            };
            let p = app.to_page(pos);
            let tol = 6.0 / d.view.zoom.max(1e-6);
            ve.parts().iter().all(|part| d.doc.world_render_bounds(part.id).is_none_or(|b| !b.inflate(tol * 4.0, tol * 4.0).contains(p)))
        })
    };
    if resp.double_clicked() || (resp.triple_clicked() && (!editing || empty_canvas())) {
        let ev = pointer_event(app, PointerKind::DoubleClick, pos, &mods);
        if let Err(e) = app.session.pointer(&ev) {
            app.toast(e.to_string());
        }
        app.canvas.dragging = false;
        return true;
    }
    let primary_pressed = ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    let primary_down = ctx.input(|i| i.pointer.primary_down());
    let primary_released = ctx.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
    // The press and the release carry their own positions: both in one frame (a quick click or a
    // slow frame) would otherwise send the press from the hover position the pointer ended at.
    let (press_pos, release_pos) = ctx.input(|i| {
        let (mut press, mut release) = (None, None);
        for e in &i.events {
            if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = e {
                if *pressed {
                    press = Some(*pos);
                } else {
                    release = Some(*pos);
                }
            }
        }
        (press, release)
    });
    let kind = if primary_pressed && resp.hovered() {
        app.canvas.dragging = true;
        app.ui.context_menu = None;
        app.ui.tool_flyout = None;
        // Clicking the canvas takes focus from panel fields.
        ctx.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        app.canvas.last_input_instant = Some(Instant::now());
        Some(PointerKind::Down)
    } else if app.canvas.dragging && primary_released {
        app.canvas.dragging = false;
        Some(PointerKind::Up)
    } else if app.canvas.dragging && primary_down {
        app.canvas.last_input_instant = Some(Instant::now());
        Some(PointerKind::Move)
    } else if resp.hovered() {
        app.canvas.hovering = true;
        Some(PointerKind::Hover)
    } else {
        // Something over the canvas took the pointer.
        if std::mem::take(&mut app.canvas.hovering) {
            left(app);
        }
        None
    };
    // Pen pressure comes with touch events (a pen on Windows, a stylus in a browser); a mouse,
    // macOS and Linux tablets report none and draw at the stroke weight.
    let force = ctx.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force: Some(f), .. } if f.is_finite() && *f > 0.0 => Some(f.clamp(0.0, 1.0)),
            _ => None,
        })
    });
    if force.is_some() {
        app.canvas.pressure = force;
    }
    update_loupe(app, &ctx, kind, pos, primary_down);
    if let Some(k) = kind {
        let at = if matches!(k, PointerKind::Down) { press_pos.unwrap_or(pos) } else { pos };
        send(app, k, at, &mods);
        // The pointer also let go within this frame (a quick click, or a slow frame): the tools get
        // the release too, so the press's gesture ends — a quick click with a shape tool still
        // draws and returns to Move, and a double-click right after drawing finds the Move tool.
        if matches!(k, PointerKind::Down) && primary_released {
            app.canvas.dragging = false;
            send(app, PointerKind::Up, release_pos.unwrap_or(pos), &mods);
        }
    }
    true
}

/// Pointer event `kind` at screen point `pos`, to the tools.
fn send(app: &mut FigApp, kind: PointerKind, pos: Pos2, m: &egui::Modifiers) {
    let ev = pointer_event(app, kind, pos, m);
    // A hover where nothing changed since the last one finds what that one found (the state is
    // taken after it ran: what it set, such as the hovered layer, is part of it).
    let hover = kind == PointerKind::Hover;
    if hover && app.canvas.last_hover.as_ref().is_some_and(|k| *k == HoverKey::of(app, ev.clone())) {
        return;
    }
    if kind == PointerKind::Up {
        app.canvas.pressure = None;
    }
    app.canvas.last_pointer = Some(Point::new(ev.x, ev.y));
    if let Err(e) = app.session.pointer(&ev) {
        app.toast(e.to_string());
    }
    app.canvas.last_hover = hover.then(|| HoverKey::of(app, ev));
    app.handle_ui_requests();
}

/// Right-click at screen `pos`: select what's under the pointer (`deep`: the nested layer), then
/// open the context menu there. A frame's name label counts as the frame, as on a press, so a
/// label drawn over its section picks the frame, not the section behind it. A layer that is
/// already selected keeps the whole selection.
pub fn open_context_menu(app: &mut FigApp, pos: Pos2, deep: bool) {
    app.canvas.loupe = None;
    let p = app.to_page(pos);
    if let Ok(ds) = app.session.ds() {
        let (doc, page, sel, zoom) = (ds.doc.clone(), ds.page, ds.selection.clone(), ds.view.zoom.max(1e-6));
        // A frame's name label is the frame, as on a press, even where it sits over a section.
        let label = figcraft_engine::tools::label_at(&doc, page, &app.session.tool.labels, p);
        let hit = app.session.hit(&doc, page, p, 2.0 / zoom);
        let target = label.or_else(|| hit.map(|leaf| figcraft_doc::geometry::select_target(&doc, leaf, &sel, deep)));
        if let Some(target) = target
            && !sel.contains(&target)
        {
            app.session.set_selection(vec![target]);
        }
    }
    app.ui.context_menu = Some((pos, app.session.selection()));
    app.ui.context_menu_shown = false;
}
