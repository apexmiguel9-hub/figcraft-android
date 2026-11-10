//! FigCraft en Android: el crate, no el binario.
//!
//! Plantilla copiada de `vectorcraft-android/src/lib.rs`: el MISMO editor de escritorio
//! (`FigApp`), sin UI movil aparte. Este crate solo monta la ventana y conecta lo que Android
//! necesita.
//!
//! Render: CPU (`tiny-skia`), la GPU solo presenta via egui-wgpu. Por eso se fuerza OpenGL
//! ES: Vulkan aborta en drivers moviles (medido en vectorcraft en PowerVR `vulkan.mtk.so` ->
//! `abort()` compilando el fragment shader).
//!
//! Gestos: el arbitraje esta en `crates/ui-egui/src/canvas.rs` (`gestos_tactiles`):
//! 1 dedo = herramienta (egui emula raton con el primer dedo), 2 dedos = pan + zoom a la vez,
//! y pulsacion larga = clic derecho. Ver ahi por que el orden pan -> zoom importa.
//!
//! Bordes: `raw_input_hook` recorta `screen_rect` con los insets que lee Java, porque la
//! ventana llega edge-to-edge y `winit` 0.30 solo rellena `safe_area_insets` en iOS.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use figcraft_engine::Session;
use figcraft_ui_egui::FigApp;

#[cfg(target_os = "android")]
mod browser;
#[cfg(target_os = "android")]
mod permiso;

/// Fichero marcador del guarda anti-bucle de cierre (crash-loop).
///
/// Se crea justo antes de `eframe::run_native` y se borra en el primer frame que
/// pinta correctamente. Si al arrancar el marcador todavía está, la ejecucion
/// anterior murio antes del primer frame (p. ej. el driver Vulkan aborto al
/// crear el pipeline de egui), asi que esta vez se fuerza OpenGL ES.
const CRASH_MARKER: &str = ".figcraft_crash_marker";

/// Ruta del marcador de crash-loop dentro del directorio privado de datos de la app.
#[cfg(target_os = "android")]
fn crash_marker_path(app: &winit::platform::android::activity::AndroidApp) -> Option<std::path::PathBuf> {
    app.internal_data_path().map(|dir| dir.join(CRASH_MARKER))
}

/// Comprueba si el marcador de crash-loop esta presente y, en ese caso, lo borra y
/// devuelve `true` (la sesion anterior cerro de forma inesperada).
#[cfg(target_os = "android")]
fn hubo_crash_previo(app: &winit::platform::android::activity::AndroidApp) -> bool {
    let Some(path) = crash_marker_path(app) else { return false };
    if path.exists() {
        let _ = fs::remove_file(&path);
        log::warn!("figcraft-android: marcador de crash-loop previo; se fuerza OpenGL ES esta vez");
        return true;
    }
    false
}

/// Crea el marcador de crash-loop antes de lanzar el bucle de frames. Devuelve su ruta.
#[cfg(target_os = "android")]
fn crear_marcador_crash(app: &winit::platform::android::activity::AndroidApp) -> Option<PathBuf> {
    let Some(path) = crash_marker_path(app) else { return None };
    let _ = fs::File::create(&path);
    Some(path)
}

/// Borra el marcador de crash-loop: el primer frame se pudo pintar.
fn borrar_marcador_crash(app: &winit::platform::android::activity::AndroidApp) {
    let Some(path) = crash_marker_path(app) else { return };
    let _ = fs::remove_file(&path);
}

// MEDIDO, y el punto en el que el port dio la vuelta: con `NativeActivity` Android manda
// **teclas** al `InputConnection` de AOSP, `winit` las recibe y el texto llega a egui solo.
// No hace falta puente JNI: el teclado blando escribe por el camino normal de winit.
//
// Lo que se probo antes y por que se borro:
//
// 1. Un `BaseInputConnection` propio reinyectando por JNI, porque se creyo que `NativeActivity` no
//    tiene `InputConnection`. Ese supuesto es falso: es justo lo que usa vectorcraft. Ademas la
//    vista de 1x1 px robaba el foco y metia a Gboard en modo extractor.
// 2. `GameActivity` (androidx.games) + `AndroidApp::text_input_state()`, leyendo el `GameTextInput`
//    a mano. Con `GameActivity` Android manda texto compuesto y **se come las teclas**, asi que
//    winit no ve nada y el puente era obligatorio. Volviendo a `native-activity` sobra entero.
//
// La razon real de que el texto no se viera era otra: el `.so` de Android se compilaba sin
// craft-fonts y en Android no existe `/usr/share/fonts`, asi que no habia ninguna fuente para
// medir ni dibujar el texto del lienzo. Ver `.github/workflows/build-android-so.yml`.
//
// MEDIDO que el parche de `android-activity` que se llevo un rato tambien sobra: estaba solo para
// que `text_input_state()` no abortara con `slice::from_raw_parts(nulo, 0)`. Esa API ya no se
// llama. Ver el `Cargo.toml` para los datos que trajeron aqui (el APK de vectorcraft instalado en el
// movil tiene `Landroid/app/NativeActivity;` en sus `.dex`).

/// Ruta del marcador de crash-loop, establecida en `android_main` y leida en el primer frame.
static CRASH_MARKER_PATH: OnceLock<PathBuf> = OnceLock::new();

/// `true` una vez que se ha pintado el primer frame correctamente; se usa para borrar el
/// marcador de crash-loop solo la primera vez.
static PRIMER_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// El `eframe::App` del port. Reenvia a `FigApp`, que ya trae 100% de la UI.
pub struct App(pub FigApp);

impl App {
    /// PRUEBA: volca cada 3 s las metricas de rendimiento que `FigApp` ya mide.
    ///
    /// MEDIDO que hace falta: en el movil de pruebas figcraft va a 33-65 fps y vectorcraft a
    /// 70-120 en el mismo aparato, asi que hay que ver donde se va el tiempo en vez de suponer.
    /// Los numeros salen de [`FigApp::inspect`], que ya los tenia todos: `frameMs`, `renderMs`,
    /// `framesDrawn`, `repaintCauses` y, del lienzo, si el render fue completo o parcial, si es
    /// preview o refinado, y las dos latencias.
    ///
    /// `framesDrawn` es un acumulado, asi que el fps sale del delta entre muestras.
    fn diag_perf(&mut self) {
        #[cfg(target_os = "android")]
        {
            use std::sync::OnceLock;
            static INICIO: OnceLock<std::time::Instant> = OnceLock::new();
            static PREV: OnceLock<std::sync::Mutex<(u64, f64)>> = OnceLock::new();

            let t = INICIO.get_or_init(std::time::Instant::now).elapsed().as_secs_f64();
            let prev = PREV.get_or_init(|| std::sync::Mutex::new((0, 0.0)));
            let Ok(mut p) = prev.lock() else { return };
            if t - p.1 < 3.0 {
                return;
            }
            let i = self.0.inspect();
            let frames = i["framesDrawn"].as_u64().unwrap_or(0);
            let dt = t - p.1;
            let fps = if frames > p.0 { (frames - p.0) as f64 / dt } else { 0.0 };
            *p = (frames, t);
            let c = &i["canvas"];
            log::info!(
                "PERF fps={fps:.1} frameMs={} renderMs={} calidad={} trabajo={} latPreview={} latRefine={} causas={} knobs=[{}]",
                i["frameMs"],
                i["renderMs"],
                c["quality"],
                c["renderWork"],
                c["previewLatencyMs"],
                c["refineLatencyMs"],
                i["repaintCauses"],
                figcraft_ui_egui::knobs_summary()
            );
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.diag_perf();
        // MEDIDO que el browser va **antes** del editor: si acaba de recoger un fichero, lo
        // vuelca aqui y el motor lo ve en este mismo frame. Al reves habia un frame de
        // retraso, que en un movil de 120 fps se ve.
        #[cfg(target_os = "android")]
        browser::logic(ctx);
        // Upstream v0.12.1 split the old `FigApp::logic` into `background_tick` (deadlines,
        // control queues, autosave) and the frame itself, and the desktop app guards it.
        figcraft_ui_egui::windows::guard(&mut self.0, ctx, |fig| fig.background_tick(ctx));
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput) {
        // MEDIDO: con `NativeActivity` las teclas llegan por el `InputConnection` de AOSP y
        // las entrega `winit`. No se inyecta nada a mano: `Event::Text`/`Event::Ime` llegan
        // ya en `_raw.events`.

        // Recorta `screen_rect`, no rellena `safe_area_insets`.
        //
        // MEDIDO en el movil (Moto G56 5G): la ventana llega **edge-to-edge** —el viewport
        // medido es de 1080 x 2400 enteros—, y `winit` 0.30 rellena `safe_area_insets`
        // **solo en iOS**. Sin esto los paneles se dibujan debajo de la barra de estado y de
        // los botones de navegacion, que es justo lo que se veia.
        //
        // MEDIDO que hay que recortar `screen_rect` y **no** rellenar `safe_area_insets`, y el
        // motivo esta en el codigo de egui: `content_rect() = viewport_rect -
        // safe_area_insets` (`input_state/mod.rs:511`) lo usan los `Window`, mientras que los
        // paneles se colocan en `available_rect()`, que sale de **`screen_rect`**, y ahi los
        // insets no intervienen. Rellenarlos ademas los restaria **dos veces** a los
        // `Window`.
        #[cfg(target_os = "android")]
        if let [l, t, r, b] = permiso::insets()
            && (l > 0.0 || t > 0.0 || r > 0.0 || b > 0.0)
            && let Some(pantalla) = _raw.screen_rect
        {
            _raw.screen_rect = Some(egui::Rect::from_min_max(pantalla.min + egui::vec2(l, t), pantalla.max - egui::vec2(r, b)));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Primer frame que pinta bien: borrar el marcador de crash-loop para que el
        // proximo arranque use el backend normal (no el fallback de OpenGL).
        #[cfg(target_os = "android")]
        if !PRIMER_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
            if let Some(path) = CRASH_MARKER_PATH.get() {
                let _ = fs::remove_file(path);
                log::info!("figcraft-android: primer frame OK; marcador de crash-loop borrado");
            }
        }
        let ctx = ui.ctx().clone();
        figcraft_ui_egui::windows::guard(&mut self.0, &ctx, |fig| fig.ui(ui));
        // MEDIDO: el aviso va en una `Window` propia y **no** en el `Ui` del editor, porque
        // en el `Ui` sale pegado a la barra de estado, encima del texto de ayuda.
        //
        // MEDIDO que la `Window` solo se crea cuando hay algo que decir: antes se creaba
        // siempre, y aunque `aviso` no pintara nada egui dibujaba su marco —en el lienzo salia
        // un punto con sombra—. Un `Window` vacio no es un `Window` invisible.
        #[cfg(target_os = "android")]
        if browser::hay_aviso() {
            egui::Window::new("permiso de almacenamiento")
                .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 44.0))
                .collapsible(false)
                .resizable(false)
                .title_bar(false)
                .show(ui.ctx(), browser::aviso);
        }
    }
}

/// Construye el editor con los servicios del port.
///
/// MEDIDO que hace falta instalar las fuentes **aqui**, y no en el primer frame, porque si no
/// la app **aborta al arrancar** con un panic que no sale con backtrace util:
///
/// ```text
/// epaint-0.36.2/src/text/fonts.rs:1025:41:
/// FontFamily::Name("semibold") is not bound to any fonts
/// panic in a function that cannot unwind
/// Fatal signal 6 (SIGABRT) ... in tid ... (android_main)
/// ```
///
/// FigCraft usa esa familia para `TextStyle::Heading` (`theme.rs:167`) y la registra en
/// `theme::install_fonts`, que `FigApp::logic` llama en su primer frame
/// (`crates/ui-egui/src/lib.rs:1112`). El problema es el orden: el theme se aplica
/// (`theme.rs:1136`, que **nombra** la familia) antes de que ese frame llegue a instalar las
/// fuentes.
///
/// MEDIDO tambien que el panic no podria recuperarse aunque se quisiera: `android_main` es
/// `extern "C"`, donde el unwind esta prohibido, asi que cualquier panic aborta el proceso
/// entero. De ahi que el arreglo sea **preventivo** y no un `catch_unwind`.
pub fn build(cc: &eframe::CreationContext<'_>) -> std::result::Result<Box<dyn eframe::App>, String> {
    figcraft_ui_egui::theme::install_fonts(&cc.egui_ctx, false, 0);

    let mut session = Session::empty();
    // The preferences saved by the previous run (theme, language, panel widths...). Without
    // this every launch started from the defaults and the Settings dialog appeared not to save.
    #[cfg(target_os = "android")]
    if let Some(prefs) = browser::load_prefs() {
        session.prefs = prefs;
    }
    // `file.new` paints the first page with the interface's theme, and this runs before the
    // first UI frame: tell the session now so a saved light theme (or the dark default) doesn't
    // create the wrong page colour.
    let dark = match session.prefs.theme.as_str() {
        "light" => false,
        "dark" => true,
        _ => cc.egui_ctx.system_theme().unwrap_or(egui::Theme::Dark) == egui::Theme::Dark,
    };
    session.set_ui_dark(dark);
    // Primera ejecucion: arranca en un fichero nuevo, no en home vacio.
    if session.docs.is_empty() {
        let _ = session.execute("file.new", &serde_json::json!({}));
    }
    #[cfg(target_os = "android")]
    let services: Box<dyn figcraft_ui_egui::Services> = Box::new(browser::Android);
    // MEDIDO: en escritorio no hay browser ni permiso, asi que se usan servicios sin
    // dialogos nativos —el mismo camino que usa la build web— y cada hueco degrada a un
    // mensaje en vez de romper.
    #[cfg(not(target_os = "android"))]
    let services: Box<dyn figcraft_ui_egui::Services> = Box::new(figcraft_ui_egui::NoServices);

    // Imported fonts live with the app's private files and load before the first frame.
    #[cfg(target_os = "android")]
    let app = {
        // Android has no `/usr/share/fonts`: this marks the (empty) system scan done, so the
        // Missing fonts window stops waiting for it. Imported fonts live in the app's own folder.
        figcraft_text::scan_system_fonts();
        let mut app = FigApp::new(session, services);
        app.set_fonts_dir(browser::fonts_dir());
        app
    };
    #[cfg(not(target_os = "android"))]
    let app = FigApp::new(session, services);
    Ok(Box::new(App(app)))
}

/// Solo OpenGL ES.
///
/// MEDIDO por que, y por que no es "Vulkan con menos cosas". En el movil de pruebas (Moto G56
/// 5G, Android 16) wgpu encuentra los dos adaptadores y elige Vulkan, que en Android es el
/// backend primario, y al crear el primer pipeline el proceso muere. Los frames interiores
/// del tombstone no son de Rust, son del driver:
///
/// ```text
/// #04-06 vulkan.mtk.so   FragmentShaderCompileState::CompileUF()+1272
/// #02-03 libufwriter.so  BILParseStream()+164
/// #00      libc.so       abort()+156
/// ```
///
/// MEDIDO que el motor rasteriza en CPU (`tiny-skia`), asi que la GPU solo presenta pixeles
/// y no tiene nada que hacer con esto: quedarse con GL no cuesta nada al dibujo.
/// Backend de wgpu para Android. Se elige al arrancar la app (antes de crear la instancia
/// de wgpu), asi que no se puede cambiar en caliente como los demas knobs.
///
/// Variable de entorno `FIGCRAFT_WGPU_BACKEND`:
///   - `vulkan`: fuerza Vulkan (crash en PowerVR/MediaTek al crear el pipeline de egui).
///   - `gl`: fuerza OpenGL ES (default, estable en PowerVR).
///   - `auto`: deja que wgpu elija (tiende a Vulkan en Android).
fn wgpu_backend() -> eframe::egui_wgpu::wgpu::Backends {
    match std::env::var("FIGCRAFT_WGPU_BACKEND").ok().as_deref().unwrap_or("vulkan") {
        "vulkan" => {
            log::info!("wgpu backend: Vulkan (forzado por FIGCRAFT_WGPU_BACKEND)");
            eframe::egui_wgpu::wgpu::Backends::VULKAN
        }
        "gl" => {
            log::info!("wgpu backend: OpenGL ES");
            eframe::egui_wgpu::wgpu::Backends::GL
        }
        _ => {
            log::info!("wgpu backend: auto (wgpu elige)");
            eframe::egui_wgpu::wgpu::Backends::all()
        }
    }
}

/// Solo OpenGL ES (estable en PowerVR/MediaTek).
///
/// MEDIDO por que, y por que no es "Vulkan con menos cosas". En el movil de pruebas (Moto G56
/// 5G, Android 16) wgpu encuentra los dos adaptadores y elige Vulkan, que en Android es el
/// backend primario, y al crear el primer pipeline el proceso muere. Los frames interiores
/// del tombstone no son de Rust, son del driver:
///
/// ```text
/// #04-06 vulkan.mtk.so   FragmentShaderCompileState::CompileUF()+1272
/// #02-03 libufwriter.so  BILParseStream()+164
/// #00      libc.so       abort()+156
/// ```
///
/// MEDIDO que el motor rasteriza en CPU (`tiny-skia`), asi que la GPU solo presenta pixeles
/// y no tiene nada que hacer con esto: quedarse con GL no cuesta nada al dibujo.
/// Nombre legible del backend que se usara, para el log de arranque.
fn wgpu_backend_name(forzar_gl: bool) -> &'static str {
    if forzar_gl {
        "OpenGL ES (forzado por crash previo)"
    } else {
        match std::env::var("FIGCRAFT_WGPU_BACKEND").ok().as_deref().unwrap_or("vulkan") {
            "vulkan" => "Vulkan",
            "gl" => "OpenGL ES",
            _ => "auto",
        }
    }
}

/// Solo OpenGL ES (estable en PowerVR/MediaTek). Cuando `forzar_gl` es true ignora
/// `FIGCRAFT_WGPU_BACKEND` y usa solo OpenGL ES (fallback tras un crash previo).
fn wgpu_solo_opengl(forzar_gl: bool) -> eframe::egui_wgpu::WgpuConfiguration {
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = if forzar_gl {
        eframe::egui_wgpu::wgpu::Backends::GL
    } else {
        wgpu_backend()
    };
    eframe::egui_wgpu::WgpuConfiguration { wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() }
}

fn window_options(forzar_gl: bool) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_min_inner_size([360.0, 480.0]).with_fullscreen(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_solo_opengl(forzar_gl),
        ..Default::default()
    }
}

/// Arranque en escritorio, para probar sin movil.
pub fn run() -> eframe::Result {
    eframe::run_native("FigCraft", window_options(false), Box::new(|cc| build(cc).map_err(|e| e.into())))
}

/// Punto de entrada en Android.
///
/// MEDIDO, y fue el fallo que mas tiempo costo en el port de vectorcraft: **un `cdylib` de
/// Rust sin ningun `#[no_mangle]` se queda vacio.** El enlazador borra todo lo que no sea
/// alcanzable desde las exportaciones, y en Rust lo unico que se exporta es lo marcado con
/// `#[no_mangle]`. Sin eso el `.so` compila, Gradle lo empaqueta, la app instala y muere al
/// arrancar.
///
/// Lo que se vio era un `.so` de 5,8 MB con **566 KB de `.text` y 3 simbolos dinamicos**. El
/// tamano no lo delata: casi todo eran datos de depuracion comprimidos. "Es pequeno" y "esta
/// roto" se parecen, asi que el workflow comprueba la tabla de simbolos en vez de fiarse del
/// peso.
///
/// De ahi sale la forma exacta de `android_main`, que es **el patron del ejemplo oficial de
/// egui** (`eframe/examples/hello_android`): el `AndroidApp` **no se pasa como argumento
/// aparte**, va en `NativeOptions::android_app`, porque es lo que el `EventLoop` de winit
/// necesita para construirse.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    // MEDIDO que hace falta, y no es opcional: `log` **descarta todo en silencio hasta que se
    // llama a `set_max_level`**, porque su `MAX_LOG_LEVEL_FILTER` empieza en `Off`. Y
    // `android_logger` 0.14 solo hace esa llamada **si no hay ya un logger puesto**, que no es
    // una garantia: el orden de inicializacion dentro de un grafo de 400 crates no lo tiene
    // uno. Ponerlo aqui lo hace explicito y no condicional.
    log::set_max_level(log::LevelFilter::Info);
    log::info!("nivel de log resuelto: {}", log::max_level());

    // MEDIDO que hace falta, y de paso una leccion del otro repo: **en Android, un panic de
    // Rust es invisible.** El gancho por defecto escribe en `stderr`, y `stderr` no llega a
    // logcat. El sintoma es `Fatal signal 6 (SIGABRT) ... (android_main+428)`: se sabe que ha
    // panicado y no donde.
    std::panic::set_hook(Box::new(|info| {
        log::error!("PANIC de Rust: {info}");
        eprintln!("PANIC de Rust: {info}");
    }));

    // MEDIDO que hace falta antes de construir las opciones: el permiso y los insets necesitan
    // el `JavaVM`, que `android-activity` solo expone a traves del `AndroidApp`. `Services`
    // consulta el permiso en cuanto el usuario abre un fichero, y `raw_input_hook` pide los
    // insets en el primer frame.
    permiso::registrar(&app);

    // Preferencias: el directorio privado de la app es el equivalente a la carpeta de
    // configuracion de escritorio. Se registra antes de `build`, que las lee.
    if let Some(dir) = app.internal_data_path() {
        log::info!("figcraft-android: datos en {}", dir.display());
        browser::set_data_dir(dir);
    } else {
        log::warn!("figcraft-android: Android no dio un directorio de datos; las preferencias no se guardaran");
    }

    // Guarda anti-bucle de cierre: si la sesion anterior murio antes del primer frame
    // (p. ej. el driver Vulkan aborto al crear el pipeline de egui), esta vez se forza
    // OpenGL ES. El marcador se borra en el primer frame que pinta bien.
    let forzar_gl = hubo_crash_previo(&app);

    // El `AndroidApp` va DENTRO de las opciones, no como argumento aparte: es lo que el
    // `EventLoop` de winit necesita para construirse, y sin el se queda sin bucle de eventos.
    let options = eframe::NativeOptions { android_app: Some(app), ..window_options() };

    log::info!("figcraft-android: arrancando, version {}", env!("CARGO_PKG_VERSION"));
    log::info!("figcraft-android: backend wgpu = {:?}, forzar_gl = {}", wgpu_backend_name(forzar_gl), forzar_gl);

    // Crear el marcador ANTES de run_native: si no llega al primer frame, perdura.
    let marker_path = crear_marcador_crash(&app);
    let _ = CRASH_MARKER_PATH.set(marker_path);

    let resultado = eframe::run_native("FigCraft", options, Box::new(|cc| build(cc).map_err(|e| e.into())));

    // MEDIDO por que esto NO es un `panic!`: el proyecto prohibe panicos en codigo que se
    // reparte. `run_native` devuelve `Err` si no puede crear la ventana o el contexto de
    // render, y un abort aqui sale como "Fatal signal 6", que no dice nada de por que.
    if let Err(e) = resultado {
        log::error!("figcraft-android: no se pudo arrancar: {e:?}");
    }
}
