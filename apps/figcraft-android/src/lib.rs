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

use figcraft_engine::Session;
use figcraft_ui_egui::FigApp;

#[cfg(target_os = "android")]
mod browser;
#[cfg(target_os = "android")]
mod permiso;

/// El texto que llega del teclado blando, entre frames.
///
/// MEDIDO que hace falta una cola y no un `String`: Java->Rust ocurre en el hilo de Java y el
/// update de egui corre en el hilo de `android_main`. Un `static Mutex<String>` con `mem::take`
/// en el update resuelve la carrera sin ningun condvar ni hilo bloqueado, que es el mismo
/// patron que usa el browser (`browser.rs`, la casilla `RESPUESTA`).
#[cfg(target_os = "android")]
static TECLAS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Java -> Rust: una pulsacion del teclado blando.
///
/// MEDIDO, y es el bug que arreglo: con `NativeActivity` el teclado blando se muestra pero las
/// teclas **no llegan** a egui, porque `NativeActivity` de AOSP no tiene vista y por tanto no
/// tiene `InputConnection` (`MainActivity.java` lo explica). Escribir texto en el lienzo era
/// imposible.
///
/// MEDIDO que se inyecta como `egui::Event::Text`, que es lo que `shortcuts::text_keys` lee y
/// manda a `figcraft_engine::textedit::insert`, o sea el mismo camino que el teclado fisico.
///
/// MEDIDO el `"\u0000"` de `onWindowFocusChanged`: es la senal de "el teclado se ha ido", y
/// suelta el foco del `TextEdit` del puente para que no se quede fantasma.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_figcraft_MainActivity_nativeTecla<'caller>(
    mut env: jni::EnvUnowned<'caller>,
    _class: jni::objects::JClass<'caller>,
    texto: jni::objects::JString<'caller>,
) {
    // MEDIDO: en `jni` 0.22 una nativa recibe `EnvUnowned`, y el `Env` de verdad sale de
    // `with_env` (`permiso.rs:150` lo usa igual). `JString::try_to_string` pide `&mut Env`.
    // MEDIDO: el tipo de error hay que nombrarlo. `with_env` pide `E: From<jni::errors::Error>` y
    // con `Ok(())` hay dos `impl` que encajan (`jni` y `android_activity`), asi que Rust dice
    // "type annotations needed for `EnvOutcome<'_, (), _>`". Se anota la closure, que es lo que
    // hace `permiso.rs`.
    let _ = env.with_env(|e| -> jni::errors::Result<()> {
        let Ok(s) = texto.try_to_string(e) else {
            return Ok(());
        };
        if let Ok(mut cola) = TECLAS.lock() {
            cola.push(s);
        }
        Ok(())
    });
}

/// El `eframe::App` del port. Reenvia a `FigApp`, que ya trae 100% de la UI.
pub struct App(pub FigApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // MEDIDO que el browser va **antes** del editor: si acaba de recoger un fichero, lo
        // vuelca aqui y el motor lo ve en este mismo frame. Al reves habia un frame de
        // retraso, que en un movil de 120 fps se ve.
        #[cfg(target_os = "android")]
        browser::logic(ctx);
        self.0.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput) {
        // MEDIDO, y es el bug que arreglo: con `NativeActivity` el teclado blando se muestra pero
        // sus teclas **no llegan** a egui. El motivo es que `NativeActivity` de AOSP no tiene
        // vista, luego no tiene `InputConnection`, y el IME no tiene contra quien enviar el texto.
        //
        // Java las recoge en `MainActivity.nativeTecla` y las deja en la cola `TECLAS`; aqui se
        // inyectan como `Event::Text`, que es exactamente lo que haria el teclado fisico, asi que
        // el camino de escritura es el de siempre (`shortcuts::text_keys` ->
        // `figcraft_engine::textedit::insert`) y no hay que tocar el motor.
        #[cfg(target_os = "android")]
        if let Ok(mut cola) = TECLAS.lock() {
            for t in cola.drain(..) {
                if t == "\u{0}" {
                    // Senal de "el teclado se fue": suelta el foco del puente (ver
                    // `keyboard_bridge_release`).
                    _ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("figcraft_android_teclado_campo")));
                } else if !t.is_empty() {
                    // MEDIDO: `deleteSurroundingText` de Java llega como una cadena de `\b`, y en
                    // egui el borrado es un `Key::Backspace`, no texto.
                    if t.chars().all(|c| c == '\u{8}') {
                        for _ in 0..t.chars().count() {
                            _raw.events.push(egui::Event::Key {
                                key: egui::Key::Backspace,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                    } else {
                        _raw.events.push(egui::Event::Text(t));
                    }
                }
            }
        }
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
        self.0.ui(ui);
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
fn wgpu_solo_opengl() -> eframe::egui_wgpu::WgpuConfiguration {
    // `WgpuSetupCreateNew` no es `#[non_exhaustive]`, pero en vez de sintaxis de actualizacion
    // se parte de su propio constructor y se cambia el campo. Es lo que menos puede romperse
    // al actualizar egui.
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

    // El `AndroidApp` va DENTRO de las opciones, no como argumento aparte: es lo que el
    // `EventLoop` de winit necesita para construirse, y sin el se queda sin bucle de eventos.
    let options = eframe::NativeOptions { android_app: Some(app), ..window_options() };

    log::info!("figcraft-android: arrancando, version {}", env!("CARGO_PKG_VERSION"));

    let resultado = eframe::run_native("FigCraft", options, Box::new(|cc| build(cc).map_err(|e| e.into())));

    // MEDIDO por que esto NO es un `panic!`: el proyecto prohibe panicos en codigo que se
    // reparte. `run_native` devuelve `Err` si no puede crear la ventana o el contexto de
    // render, y un abort aqui sale como "Fatal signal 6", que no dice nada de por que.
    if let Err(e) = resultado {
        log::error!("figcraft-android: no se pudo arrancar: {e:?}");
    }
}
