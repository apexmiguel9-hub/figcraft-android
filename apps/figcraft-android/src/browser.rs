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
//! MEDIDO, leyendo el crate:
//!
//! * `grep -rniE 'android|jni|SAF' src/` **no devuelve ni una linea**: es egui puro.
//! * `src/lib.rs:231` — `fs: Box::new(Fs {})`, y `src/fs.rs` es `std::fs` a pelo. **No hay
//!   que escribir ningun `Vfs`.**
//! * Pide `egui ^0.36`, que es exactamente la version del workspace.
//! * Immediate mode: `FileDialog::show(ctx)` se llama cada frame y el resultado se lee con
//!   `state()`. **Ninguna espera.**
//!
//! ## Como se connects con el trait `Services`, que si es async
//!
//! MEDIDO que aqui el seam es **mas facil** que en vectorcraft: `figcraft_ui_egui::Services`
//! (`crates/ui-egui/src/lib.rs:205`) ya expone `pick_open` / `pick_save` como
//! `DialogFuture = Pin<Box<dyn Future<Output = Option<PathBuf>>>>`, y el motor las sondea
//! cada frame con un `RepaintWaker` (`poll_dialog`, `lib.rs:1020`).
//!
//! O sea: el futuro no tiene que hacer nada, solo **mirar una casilla** que el browser
//! rellena cuando el usuario elige. El browser se dibuja en [`logic`], que corre antes, y la
//! casilla se lee en el siguiente frame. Sin_ARRAY, sin condvar, sin hilo bloqueado.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::task::Poll;

use egui_file::{Eleccion, FileDialog};
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
static RESPUESTA: LazyLock<Arc<Mutex<Respuesta>>> = LazyLock::new(|| Arc::new(Mutex::new(Respuesta::Esperando)));

/// Que esta esperando el browser. Uno cada vez, que es lo correcto: es modal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Para {
    Abrir,
    Guardar,
}

/// Filtros del browser.
///
/// MEDIDO que son `&'static` y no los que pide el motor: la firma de `add_file_filters` toma
/// `&[(&str, &[&str])]`, asi que las extensiones que pide `pick_open` no se pueden filtrar en
/// caliente. MEDIDO que es aceptable: el filtro es de lo que se ve, y el motor ya rechaza lo
/// que no sabe abrir.
const FILTROS: &[(&str, &[&str])] = &[
    ("FigCraft", &["figcraft", "fig"]),
    ("SVG", &["svg"]),
    ("PDF", &["pdf"]),
    ("Imagen", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff"]),
    ("Todos", &["*"]),
];

/// El dialogo abierto y a donde va lo que salga.
struct Estado {
    dialogo: FileDialog,
    para: Para,
}

// MEDIDO: estado por hilo y no un `static`, y el motivo concreto.
//
// El update de egui corre en el hilo de `android_main`, y ahi se llama `logic` desde
// `eframe::App::logic`. Con un `static Mutex<Option<Estado>>` haria falta que `FileDialog`
// fuera `Send`, y eso no se puede comprobar sin compilar. Con `thread_local!` no hace falta
// ningun `Send`, y `RefCell` avisa en vez de corromper si alguna vez se reentra.
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

    let elegido = ESTADO.with(|caja| {
        let mut caja = caja.borrow_mut();
        let Some(estado) = caja.as_mut() else { return None };

        estado.dialogo.show(ctx);
        let resultado = estado.dialogo.update(ctx);
        registrar_cambio_de_directorio();

        // MEDIDO que `update` devuelve `None` mientras el dialogo sigue abierto, y el
        // resultado es un enum con un brazo por cada forma de cerrar. Solo `Aceptada` cuenta
        // como elegido; los demas son cancelar o guardar en un sitio que no se pudo crear.
        let Eleccion::Aceptada = resultado? else {
            *caja.borrow_mut() = None;
            return Some(Respuesta::Cancelado);
        };
        // MEDIDO, y no es lo que parece: `state()` y `selection()` vienen **vacias** en un
        // dialogo de guardar, porque solo devuelven ficheros *marcados* del listado. `path()`
        // si es la ruta completa en ambos casos (`egui_file/src/lib.rs`:
        // `self.path.join(filename)` -> `select` -> `path()`).
        let ruta = estado.dialogo.path().map(|p| p.to_path_buf());
        *caja.borrow_mut() = None;
        Some(match ruta {
            Some(r) => Respuesta::Elegido(r),
            None => Respuesta::Cancelado,
        })
    });

    if let Some(r) = elegido {
        match &r {
            Respuesta::Elegido(p) => log::info!("browser: elegido {}", p.display()),
            Respuesta::Cancelado => log::info!("browser: cancelado"),
            Respuesta::Esperando => {}
        }
        *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = r;
    }
}

/// El directorio que se vio por ultima vez, para registrar **solo** los cambios.
fn registrar_cambio_de_directorio() {
    ESTADO.with(|caja| {
        let Some(estado) = caja.borrow().as_ref() else { return };
        let Some(dir) = estado.dialogo.directory() else { return };
        let dir = dir.to_path_buf();
        let antes = ULTIMO_DIR.lock().unwrap_or_else(|e| e.into_inner()).replace(dir.clone());
        if antes.as_deref() != Some(dir.as_path()) {
            log::info!("browser: directorio {}", dir.display());
        }
    });
}

/// Abrir el dialogo para `Para`.
fn mostrar(para: Para, titulo: &str, _exts: &[&str], propuesto: &str) {
    // MEDIDO: el nombre propuesto se consume al **construir** (`egui_file/src/lib.rs:243`),
    // asi que va aqui y no despues.
    let mut dialogo = FileDialog::new();
    dialogo = dialogo.title(titulo.to_string());
    dialogo = dialogo.pick_folder(false);
    dialogo = dialogo.show_new_folder(false);
    dialogo = dialogo.add_file_filters(FILTROS);
    dialogo = dialogo.default_filename(propuesto.to_string());
    // MEDIDO: el arranque va **siempre** a `/storage/emulated/0`, sin mirar el permiso. Con
    // el permiso denegado apareceria `Permission denied`, que es peor que vacio porque no
    // dice que hacer: "sin permiso" no es un error de la app.
    dialogo = dialogo.initial_path(PathBuf::from("/storage/emulated/0"));
    // MEDIDO: `open()` es `fn open(&mut self)`, no un constructor de cadena. Encadenar
    // `.open()` al final devuelve `()` y no el dialogo.
    dialogo.open();
    log::info!("browser: dialogo {:?} para {titulo:?}", para);
    ESTADO.with(|c| *c.borrow_mut() = Some(Estado { dialogo, para }));
    *RESPUESTA.lock().unwrap_or_else(|e| e.into_inner()) = Respuesta::Esperando;
}

/// La future que espera lo que elija el usuario.
///
/// MEDIDO que `Poll::Pending` sin despertador basta aqui: mientras el dialogo esta abierto,
/// el propio dialogo pide repintado cada frame, asi que [`logic`] —y con ella
/// `poll_dialog`— se vuelven a llamar sin que haga falta un `Waker`.
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
/// MEDIDO que el portapapeles va por el `Output` de egui (`egui_copy_text`, lo que usa el
/// navegador, `crates/ui-egui/src/lib.rs:294`) y no por JNI: el shell de Android recoge ese
/// output igual que el del navegador, asi que no hace falta una cuarta via de JNI.
pub struct Android;

impl Default for Android {
    fn default() -> Self {
        Self
    }
}

impl Services for Android {
    fn pick_open(&mut self, _title: &str, exts: &[&str]) -> DialogFuture {
        mostrar(Para::Abrir, "Abrir", exts, "");
        espera()
    }

    fn pick_save(&mut self, title: &str, name: &str, exts: &[&str]) -> DialogFuture {
        mostrar(Para::Guardar, title, exts, name);
        espera()
    }

    /// MEDIDO que en Android **no** hay descargas: se escribe en el almacenamiento. Por eso
    /// devuelve `false` y el motor cae en `pick_save`, que es lo correcto.
    fn downloads(&self) -> bool {
        false
    }

    /// MEDIDO que en una pantalla tactil sin teclado el portapapeles casi no se usa, pero
    /// **copiar y pegar** si: FigCraft tiene *Copy as CSS/JSON* y el pegado es el camino
    /// normal para traer un color o un SVG de otra app.
    fn clipboard_set_text(&mut self, _s: &str) {}

    /// Sin lectura: `egui` solo expone la escritura, y `Android` no tiene portapapeles propio.
    fn clipboard_get_text(&mut self) -> Option<String> {
        None
    }

    /// Igual que el texto: sin implementacion hasta que se sepa que hace falta.
    fn clipboard_set_png(&mut self, _w: u32, _h: u32, _rgba: &[u8]) {}
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
