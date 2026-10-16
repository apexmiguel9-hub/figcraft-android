//! FigCraft en Android: el crate, no el binario.
//!
//! Plantilla copiada de `vectorcraft-android/src/lib.rs`: el MISMO editor de
//! escritorio (`FigApp`), sin UI movil aparte. Este crate solo monta la ventana.
//!
//! Render: CPU (`tiny-skia`), la GPU solo presenta via egui-wgpu. Por eso se
//! fuerza OpenGL ES: Vulkan aborta en drivers moviles (medido en vectorcraft
//! en PowerVR `vulkan.mtk.so` -> `abort()` compilando el fragment shader).
//!
//! Gestos: no hay codigo aqui. egui 0.36 trae `zoom_delta()`, `rotation_delta()`,
//! `translation_delta()` y `multi_touch()`; winit 0.30 Android emite
//! `TouchPhase::Started/Moved` con id por dedo, y egui-winit lo agrupa.
//! 1 dedo = herramienta, 2 dedos = pan + zoom (igual que vectorcraft).

use figcraft_engine::Session;
use figcraft_ui_egui::FigApp;

/// El `eframe::App` del port. Reenvia a `FigApp`, que ya trae 100% de la UI.
pub struct App(pub FigApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput) {
        // Recorta `screen_rect`, no rellena `safe_area_insets`.
        // Medido en vectorcraft (2400x1080, 2.4375 px/pt): la ventana llega
        // edge-to-edge y winit 0.30 solo rellena safe_area en iOS.
        // Los paneles salen de `screen_rect`; los `Window` de `content_rect`.
        #[cfg(target_os = "android")]
        if let [l, t, r, b] = insets()
            && (l > 0.0 || t > 0.0 || r > 0.0 || b > 0.0)
            && let Some(pantalla) = _raw.screen_rect
        {
            _raw.screen_rect = Some(egui::Rect::from_min_max(pantalla.min + egui::vec2(l, t), pantalla.max - egui::vec2(r, b)));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

/// Construye el editor con servicios de escritorio degradados.
/// v1: `NoServices` (sin rfd/arboard). El browser `egui_file` + `permiso.rs`
/// de vectorcraft se enchufa aqui en el siguiente paso.
pub fn build(_cc: &eframe::CreationContext<'_>) -> std::result::Result<Box<dyn eframe::App>, String> {
    let mut session = Session::empty();
    // Primera ejecucion: arranca en un fichero nuevo, no en home vacio.
    if session.docs.is_empty() {
        let _ = session.execute("file.new", &serde_json::json!({}));
    }
    let app = FigApp::new(session, Box::new(figcraft_ui_egui::NoServices));
    Ok(Box::new(App(app)))
}

fn wgpu_solo_opengl() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = eframe::egui_wgpu::wgpu::Backends::GL;
    eframe::egui_wgpu::WgpuConfiguration { wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() }
}

fn window_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_min_inner_size([360.0, 480.0]).with_fullscreen(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_solo_opengl(),
        ..Default::default()
    }
}

/// Arranque en escritorio, para probar sin movil.
pub fn run() -> eframe::Result {
    eframe::run_native("FigCraft", window_options(), Box::new(|cc| build(cc).map_err(|e| e.into())))
}

/// Insets del sistema en puntos como [l, t, r, b].
/// v1: lee via Java en `permiso.rs` (siguiente paso). Aqui 0 para compilar
/// en escritorio y no romper el layout hasta enchufar JNI.
#[cfg(target_os = "android")]
fn insets() -> [f32; 4] {
    // TODO: permiso::insets() como en vectorcraft-android/src/permiso.rs
    [0.0, 0.0, 0.0, 0.0]
}

/// Punto de entrada en Android. Sin esto el `.so` sale VACIO (el linker tira
/// todo lo no alcanzable desde `#[no_mangle]`).
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    log::set_max_level(log::LevelFilter::Info);
    log::info!("nivel de log resuelto: {}", log::max_level());

    std::panic::set_hook(Box::new(|info| {
        log::error!("PANIC de Rust: {info}");
        eprintln!("PANIC de Rust: {info}");
    }));

    let options = eframe::NativeOptions { android_app: Some(app), ..window_options() };

    log::info!("figcraft-android: arrancando, version {}", env!("CARGO_PKG_VERSION"));

    let resultado = eframe::run_native("FigCraft", options, Box::new(|cc| build(cc).map_err(|e| e.into())));

    if let Err(e) = resultado {
        log::error!("figcraft-android: no se pudo arrancar: {e:?}");
    }
}
