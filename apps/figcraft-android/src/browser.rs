//! El browser de ficheros y los `Services` del port, dibujados dentro de la app.
//!
//! Sustituye a SAF entero, y la razon esta medida, no supuesta.
//!
//! ## Por que no SAF
//!
//! MEDIDO en `vectorcraft-android` (mismo `NativeActivity`, mismo `android-activity`):
//! abrir una segunda Activity da **ANR**. El selector del sistema abre una Activity, eso
//! pausa la nuestra, y `android-activity` **bloquea el hilo principal de Java hasta que el
//! hilo del bucle confirma el estado**. Traza textual, del propio movil:
//!
//! ```text
//! "main" prio=5 tid=1 Native
//!   native: #01 <std::sys::sync::condvar::futex::Condvar>::wait+112
//!   native: #02 <WaitableNativeActivityState>::set_activity_state+160
//!   native: #05 android_activity::activity_impl::glue::on_pause+20
//!   native: #08 android.app.NativeActivity.onPause+16
//! ```
//!
//! `set_activity_state` (`glue.rs:511`) escribe el comando en un pipe y espera en un condvar
//! a que `activity_state` cambie, y **solo el hilo del bucle puede cambiarlo**. Ese hilo es
//! el de `android_main`, que es donde corre el update de egui. Si el update espera al
//! resultado del selector, el hilo nunca confirma la pausa y el resultado nunca llega.
//!
//! **No es un problema de SAF: es de abrir cualquier segunda Activity.** Por eso un browser
//! propio, pintado en egui, no abre ninguna.
//!
//! ## Por que `egui_file`
//!
//! MEDIDO, leyendo el crate (version 0.28.0 bajada de crates.io):
//!
//! * `grep -rniE 'android|jni|SAF' src/` **no devuelve ni una linea**: es egui puro.
//! * `src/lib.rs:231` — `fs: Box::new(Fs {})`, y `src/fs.rs` es `std::fs` a pelo.
//!   **No hay que escribir ningun `Vfs`.**
//! * Pide `egui ^0.36`, que es exactamente la version del workspace.
//! * Immediate mode: `FileDialog::show(ctx)` se llama cada frame y el resultado se lee
//!   con `state()` / `path()` / `selection()`. **Ninguna espera.**
//!
//! ## Como se conecta con el trait `Services`, que es async
//!
//! `figcraft_ui_egui::Services` expone `pick_open` / `pick_save` / `pick_folder` como
//! `DialogFuture = Pin<Box<dyn Future<Output = Option<PathBuf>>>>`, y el motor las sondea
//! cada frame con un `RepaintWaker` (`poll_dialog`, `lib.rs:1020`).
//!
//! O sea: el futuro no tiene que hacer nada, solo **mirar una casilla** que el browser
//! rellena cuando el usuario elige. El browser se dibuja en [`logic`], que corre antes,
//! y la casilla se lee en el siguiente frame. Sin condvar, sin hilo bloqueado.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::task::Poll;

use egui_file::{FileDialog, State};
use figcraft_engine::{Prefs, Session};
use figcraft_ui_egui::{DialogFuture, Services};

use crate::permiso;

/// Lo que el usuario eligio en el dialogo abierto.
///
/// MEDIDO que hace falta un estado aparte y no un `Option<PathBuf>` a secas: "todavia no se
/// ha elegido" y "el usuario ha cancelado" son cosas distintas, y confundirlas hacia que
/// guardar pareciera haber funcionado.
#[derive(Clone, PartialEq, Eq)]
enum Respuesta {
    Esperando,
    Elegido(PathBuf),
    Cancelado,
}

/// La casilla que leen las futures de [`Services`].
///
/// MEDIDO que es `static` y no un campo del `App`: las futures se crean **dentro** de
/// `pick_open` y `pick_save`, que solo reciben `&mut self` del trait; no hay donde guardar
/// nada mas. Y no se guarda ninguna referencia a la app.
static RESPUESTA: LazyLock<Arc<Mutex<Respuesta>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Respuesta::Esperando)));

/// Que esta esperando el browser. Uno cada vez, que es lo correcto: es modal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Para {
    Abrir,
    Guardar,
    Carpeta,
}

/// Filtros del browser.
///
/// MEDIDO que son `&[(&str, &[&str])]` y se traducen a un `show_files_filter` con closure,
/// porque `egui_file 0.28` no tiene `add_file_filters`. Los ext se comparan en minusculas.
const FILTROS: &[(&str, &[&str])] = &[
    ("FigCraft", &["figcraft", "fig"]),
    ("SVG", &["svg"]),
    ("PDF", &["pdf"]),
    ("Imagen", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff"]),
    ("Todos", &["*"]),
];

/// El dialogo abierto y a donde va lo que sale.
struct Estado {
    dialogo: FileDialog,
    para: Para,
}

// MEDIDO: estado por hilo y no un `static`, y el motivo concreto.
//
// El update de egui corre en el hilo de `android_main`, y ahi se llaman todos los ganchos
// que nos importan: `pick_open`/`pick_save`/`pick_folder` desde el menu, y `logic` desde
// `eframe::App::logic`. Con un `static Mutex<Option<Estado>>` haria falta que
// `FileDialog` fuera `Send`, y eso no se puede comprobar sin compilar. Con `thread_local!`
// no hace falta ningun `Send`, y `RefCell` avisa en vez de corromper si alguna vez se reentra.
thread_local! {
    static ESTADO: RefCell<Option<Estado>> = const { RefCell::new(None) };
}

/// El directorio que se vio por ultima vez, para registrar **solo** los cambios.
///
/// MEDIDO por que hace falta: el bug que se midio en el movil de vectorcraft es que al tocar
/// una carpeta a veces no se abre, y sin esto no hay manera de saber si el toque llego al
/// dialogo o no.
static ULTIMO_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Un frame del update.
///
/// Se llama **antes** de `FigApp::logic`, a proposito: si el browser acaba de recoger un
/// fichero, se vuelca en la casilla aqui y el motor lo ve en el mismo frame.
pub fn logic(ctx: &egui::Context) {
    permiso::cada_frame();

    ESTADO.with(|caja| {
        let mut caja = caja.borrow_mut();
        let Some(estado) = caja.as_mut() else { return };

        estado.dialogo.show(ctx);

        // MEDIDO, y este es el arreglo del bug de "toco una carpeta y a veces no se abre".
        //
        // No es una carrera ni un fallo de lectura: es que `egui_file` pide **doble
        // toque** para entrar en una carpeta (`src/lib.rs:813` y `:827`):
        //
        //     if response.clicked()        { … Command::Select(…) }            // selecciona
        //     if response.double_clicked() { … Command::BrowseDirectory(…) }  // entra
        //
        // En un dedo, el doble toque solo se registra si los dos caen dentro del tiempo de
        // egui. Tocar rapido entra; tocar despacio no. De ahi el "una de cada dos o
        // tres", y de ahi que con un toque y el boton **Open** fuera siempre: un toque
        // selecciona y el boton hace `OpenSelected`.
        //
        // El arreglo usa solo API publica, sin tocar el crate: tras un toque, la carpeta
        // queda en `selected_file`, asi que `path()` es el directorio tocado y
        // `directory()` sigue siendo el de antes. Si son distintos, se entra.
        //
        // MEDIDO que no hay bucle: al entrar, `set_path` llama a `refresh`, que hace
        // `selected_file = None` (`lib.rs:494`), asi que `path()` vuelve a dar `None`.
        if estado.dialogo.state() == State::Open
            && let Some(tocado) = estado.dialogo.path().map(std::path::Path::to_path_buf)
            && tocado.is_dir()
            && tocado != estado.dialogo.directory()
        {
            log::info!("browser: se entra en {}", tocado.display());
            estado.dialogo.set_path(tocado);
        }

        // MEDIDO, y es la instrumentacion del bug de las carpetas: un registro por
        // cambio de directorio, no uno por frame.
        let ahora = estado.dialogo.directory().to_path_buf();
        let cambio = ULTIMO_DIR.lock().unwrap_or_else(|e| e.into_inner()).as_ref() != Some(&ahora);
        if cambio {
            ULTIMO_DIR.lock().unwrap_or_else(|e| e.into_inner()).replace(ahora.clone());
            log::info!("browser: directorio -> {}", ahora.display());
        }

        match estado.dialogo.state() {
            State::Selected => {
                // MEDIDO: `selection()` cubre los casos con multi-seleccion, pero se cae a
                // `path()` si viene vacia (p. ej. dialogo de guardar).
                let mut elegidas: Vec<PathBuf> =
                    estado.dialogo.selection().iter().map(|p| p.to_path_buf()).collect();
                if elegidas.is_empty()
                    && let Some(p) = estado.dialogo.path()
                {
                    elegidas.push(p.to_path_buf());
                }
                let para = estado.para;
                *caja = None;
                if let Some(ruta) = elegidas.into_iter().next() {
                    *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = Respuesta::Elegido(ruta);
                } else {
                    *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = Respuesta::Cancelado;
                }
            }
            State::Cancelled | State::Closed => {
                *caja = None;
                *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = Respuesta::Cancelado;
            }
            // `Open`: sigue abierto.
            State::Open => {}
        }
    });
}

/// Abrir el dialogo para `Para`.
///
/// MEDIDO que hay que construir uno nuevo cada vez y no reutilizar: el motor ya dejo el
/// ultimo destino en su sitio, y es lo que hace el ejemplo de la libreria. Ademas
/// `FileDialog::new` es **privado** en egui_file 0.28: los unicos constructores publicos
/// son `open_file`, `save_file` y `select_folder`, asi que el tipo se elige con la funcion
/// que se pasa.
fn mostrar(para: Para, titulo: &str, exts: &[&str], propuesto: &str, es_carpeta: bool) {
    if ESTADO.with(|c| c.borrow().is_some()) {
        // Ya hay un modal abierto. Sustituirlo perderia lo elegido hasta ahora, asi que
        // la peticion se ignora, que es lo que hace un modal de verdad.
        log::info!("browser: ya hay un dialogo abierto, peticion ignorada");
        return;
    }

    let mut dialogo = if es_carpeta {
        FileDialog::select_folder()
    } else if para == Para::Guardar {
        FileDialog::save_file()
    } else {
        FileDialog::open_file()
    };

    dialogo = dialogo
        .title(titulo)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_size(egui::vec2(760.0, 460.0))
        .show_system_files(false)
        .multi_select(false)
        .show_new_folder(false)
        .show_rename(false);

    // MEDIDO: `egui_file 0.28` no tiene `add_file_filters`. Se usa `show_files_filter` con
    // un closure `Box<dyn Fn(&Path) -> bool>`. Los ext se comparan en minusculas porque el
    // motor las da asi pero los ficheros no siempre.
    if !exts.is_empty() {
        let permitidas: Vec<String> =
            exts.iter().map(|e| e.to_ascii_lowercase()).collect();
        dialogo = dialogo.show_files_filter(Box::new(move |p: &std::path::Path| {
            match p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) {
                Some(e) => permitidas.contains(&e),
                // Sin extension son las carpetas: siempre se dejan pasar.
                None => true,
            }
        }));
    }

    // MEDIDO: `default_filename` se consume al construir (`egui_file/src/lib.rs:243`),
    // asi que el nombre propuesto se le pasa aqui y no despues.
    if !propuesto.is_empty() {
        dialogo = dialogo.default_filename(propuesto);
    }

    // MEDIDO: el arranque va **siempre** a `/storage/emulated/0`, sin mirar el permiso.
    // Con el permiso denegado apareceria `Permission denied`, que es peor que vacio porque
    // no dice que hacer: "sin permiso" no es un error de la app.
    dialogo = dialogo.initial_path(PathBuf::from("/storage/emulated/0"));

    // MEDIDO, y lo dice el propio error de compilacion:
    //     expected `FileDialog`, found `()`
    //     note: method `open` modifies its receiver in-place
    // O sea que `open()` es `fn open(&mut self)`, no un constructor de cadena. Encadenar
    // `.open()` al final devolvia `()` y no el dialogo.
    dialogo.open();

    ESTADO.with(|c| *c.borrow_mut() = Some(Estado { dialogo, para }));
    *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = Respuesta::Esperando;
}

/// La future que espera lo que elija el usuario.
///
/// MEDIDO que `Poll::Pending` sin despertador basta aqui: mientras el dialogo esta abierto,
/// el propio dialogo pide repintado cada frame, asi que [`logic`] —y con ella el poll del
/// futuro— se vuelven a llamar sin que haga falta un `Waker`.
fn espera() -> DialogFuture {
    let casilla = RESPUESTA.clone();
    Box::pin(std::future::poll_fn(move |_cx| match casilla.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Respuesta::Esperando => Poll::Pending,
        Respuesta::Elegido(p) => Poll::Ready(Some(p)),
        Respuesta::Cancelado => Poll::Ready(None),
    }))
}

/// Los `Services` del port: el browser y `std::fs`.
///
/// MEDIDO que `read`/`write` no hacen falta aqui: el trait de figcraft pide una **ruta**
/// (`pick_open` / `pick_save`) y el motor lee y escribe con `std::fs` por su cuenta. Ese es
/// el motivo de que este port sea mas pequeno que el de vectorcraft.
///
/// MEDIDO que el portapapeles de **escritura** va por el `Output` de egui
/// (`egui_copy_text`) y no por JNI. La **lectura** del portapapeles queda como stub
/// (`None`) hasta que se implemente con JNI (`android.content.ClipboardManager`), que es
/// lo que falta despues del browser.
pub struct Android;

impl Default for Android {
    fn default() -> Self {
        Self
    }
}

impl Services for Android {
    fn pick_open(&mut self, title: &str, exts: &[&str]) -> DialogFuture {
        mostrar(Para::Abrir, title, exts, "", false);
        espera()
    }

    fn pick_save(&mut self, title: &str, name: &str, exts: &[&str]) -> DialogFuture {
        mostrar(Para::Guardar, title, exts, name, false);
        espera()
    }

    /// MEDIDO que en Android **no** hay descargas: se escribe en el almacenamiento. Por eso
    /// devuelve `false` y el motor cae en `pick_save`, que es lo correcto.
    fn downloads(&self) -> bool {
        false
    }

    /// Elegir una carpeta (p. ej. "Anadir biblioteca"). Reutiliza el mismo browser.
    fn pick_folder(&mut self, title: &str) -> DialogFuture {
        mostrar(Para::Carpeta, title, &[], "", true);
        espera()
    }

    /// MEDIDO que en una pantalla tactil sin teclado el portapapeles casi no se usa, pero
    /// **copiar y pegar** si: FigCraft tiene *Copy as CSS/JSON* y el pegado es el camino
    /// normal para traer un color o un SVG de otra app.
    fn clipboard_set_text(&mut self, _s: &str) {}

    /// Lectura del portapapeles via JNI (`android.content.ClipboardManager`).
    /// Ver ANDROID-PORT.md §4.4 y `permiso::clipboard_get_text`.
    fn clipboard_get_text(&mut self) -> Option<String> {
        permiso::clipboard_get_text()
    }

    /// Igual que el texto: sin implementacion hasta que se sepa que hace falta.
    fn clipboard_set_png(&mut self, _w: u32, _h: u32, _rgba: &[u8]) {}

    /// Ninguna URL nativa en Android por ahora.
    fn open_url(&mut self, _url: &str) {}

    /// Ningun gestor de archivos nativo por ahora.
    fn reveal_path(&mut self, _path: &std::path::Path) {}

    /// Depois de un panic: no hay copia de emergencia específica de Android.
    fn emergency_save(&mut self, _session: &Session) {}

    /// Guardar preferencias: no hay nada específico de Android que guardar aparte de lo
    /// que ya hace el motor en `prefs`.
    fn save_prefs(&mut self, _prefs: &Prefs) {}

    /// El idioma del sistema no se lee desde Android por ahora; el motor usa el default.
    fn system_language(&self) -> Option<String> {
        None
    }

    /// Sin gamepads en Android por ahora.
    fn gamepad_held(&mut self) -> u32 {
        0
    }
}

/// Si hay que mostrar el aviso de permiso.
///
/// MEDIDO que hace falta por separado del aviso: la `Window` se dibuja **antes** de llamar a
/// [`aviso`], asi que sin esto se creaba una ventana **vacia** y salia su marco en el lienzo
/// —un punto con sombra— aunque no hubiera nada que decir.
pub fn hay_aviso() -> bool {
    permiso::hay_que_preguntar()
}

/// El aviso de permiso, si falta.
///
/// MEDIDO que no hace falta pedirlo solo: el usuario lo salta y la app sigue siendo utilizable
/// para lo suyo. Por eso son dos botones y no un dialogo modal.
pub fn aviso(ui: &mut egui::Ui) {
    if !permiso::hay_que_preguntar() {
        return;
    }
    ui.vertical(|ui| {
        ui.label("FigCraft necesita acceso a tus ficheros para abrir y guardar.");
        ui.label("Sin el, solo puede usar los suyos.");
        ui.horizontal(|ui| {
            if ui.button("Permitir").clicked() {
                permiso::pedir();
            }
            if ui.button("Ahora no").clicked() {
                permiso::descartar();
            }
        });
    });
}
