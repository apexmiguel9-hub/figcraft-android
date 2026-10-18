//! Lo unico que queda de JNI: el permiso de almacenamiento y los **insets del sistema**.
//!
//! Port directo de `vectorcraft-android/src/permiso.rs` (mismo `NativeActivity`, mismo
//! handshake). Lo unico que cambia es el paquete: `ai.storyteller.figcraft`.
//!
//! SAF se ha ido: el browser es nuestro y se dibuja en egui (ver el modulo
//! [`crate::browser`]). De la parte de Java solo quedan dos cosas: pedir
//! `MANAGE_EXTERNAL_STORAGE`, que es lo que hace falta para que `std::fs` alcance
//! `/storage/emulated/0/`, y **leer los insets del sistema**, que `winit` 0.30 solo rellena
//! en iOS y sin ellos `egui` dibuja debajo de la barra de estado y de los botones de
//! navegacion. Ver [`insets`].
//!
//! ## Por que un appop y no un permiso normal
//!
//! `MANAGE_EXTERNAL_STORAGE` **no se concede con un dialogo**. Es un appop, y hay que
//! abrir `Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION`; el usuario lo activa
//! a mano. MEDIDO en el propio paquete de ArmourPaint, que es el unico modo de verlo:
//!
//! ```text
//! requested permissions:
//!   android.permission.MANAGE_EXTERNAL_STORAGE
//! install permissions:
//!   android.permission.MANAGE_EXTERNAL_STORAGE: granted=false, flags=[USER_SET]
//! appops:
//!   MANAGE_EXTERNAL_STORAGE: allow
//! ```
//!
//! Ojo al par de lineas: `granted=false` **no** significa que este denegado. Los appops
//! no salen en la lista de permisos concedidos; lo que decide es `appops: allow`. Y es
//! justo el motivo de que haga falta un modulo aparte en vez de un `checkSelfPermission`.
//!
//! ## Que pasa si el usuario dice que no
//!
//! **No se rompe nada.** Sin el permiso el navegador sigue funcionando para lo que es
//! nuestro —el directorio de la app, que es suyo— y solo no alcanza
//! `/storage/emulated/0/`. MEDIDO el porque:
//!
//! ```text
//! $ adb shell run-as ai.storyteller.figcraft ls /sdcard
//! ls: /sdcard: Permission denied
//! ```
//!
//! Y el otro lado, con un build de test de ArmourPaint **sin ningun** permiso de
//! almacenamiento: su browser muestra la **rejilla vacia** en `/Download` — los nombres
//! de las carpetas de primer nivel si se ven, su contenido no.
//!
//! Por eso esto es un aviso con dos botones y no un dialogo modal: se puede saltar y la
//! app sigue sirviendo.
//!
//! ## El puente JNI, y por que hay que tomarlo y no buscarlo
//!
//! MEDIDO: `Env::find_class` y `Env::load_class` fallan desde el hilo nativo de
//! `android-activity` con `NoClassDefFound`, y `LoaderContext::FromObject` tambien
//! fallo. La quinta via, que funciona, es que **`MainActivity.onCreate` nos pase su
//! propia clase** llamando a [`Java_ai_storyteller_figcraft_MainActivity_nativeListo`];
//! a partir de ahi hay una referencia global y no hace falta buscar nada.
//!
//! MEDIDO tambien que hace falta `System.loadLibrary("vectorcraft_android")` explicito
//! en el `onCreate`. Sin eso **ninguna** nativa de la clase resolvia, aunque la
//! libreria estuviera cargada y Rust arrancando, con `UnsatisfiedLinkError`:
//! `android.app.NativeActivity` carga la `.so` por su cuenta leyendo el meta-data
//! `android.app.lib_name`, pero lo hace por el camino **nativo** de ANativeActivity, que
//! no se le comunica a ART, y el `dlsym` de ART busca entre las librerias que el
//! `ClassLoader` tiene registradas.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use jni::Env;
use jni::objects::{JObject, JString};
use jni::sys::JavaVM;

/// El `JavaVM`, del `AndroidApp` de `android-activity`.
///
/// MEDIDO: `winit::platform::android::activity::AndroidApp` reexporta
/// `android_activity::AndroidApp`, que da `vm_as_ptr()`. El grafo ya trae `jni` como
/// dependencia de `android-activity`, asi que anadirla aqui como directa no amplia nada.
static VM: AtomicPtr<JavaVM> = AtomicPtr::new(std::ptr::null_mut());

/// La clase de Java, tal y como la pasa `MainActivity.onCreate`.
///
/// Es el puntero de una referencia **global** creada en `nativeListo`. Ojo: un puntero no
/// es una referencia, y por eso el `Global` se filtra con `mem::forget` — sin eso la
/// referencia se borra y lo que queda aqui apunta a memoria liberada. Lo dice el `abort`
/// de Android, textual: `jobject is an invalid global reference … deleted reference`.
static CLASE_JAVA: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Ultimo valor de `Environment.isExternalStorageManager()`.
///
/// MEDIDO que hace falta cachearlo: `concedido()` se llama al construir el browser y en
/// cada frame del aviso, y una llamada JNI por frame no es algo que se deje a la ligera.
/// Se refresca solo cuando el usuario ha ido a Ajustes a cambiarlo.
static CONCEDIDO: AtomicBool = AtomicBool::new(false);

/// El usuario ya dijo que no. No se vuelve a preguntar.
static DESCARTADO: AtomicBool = AtomicBool::new(false);

/// Frames que quedan mirando el permiso, porque el usuario se ha ido a Ajustes.
static MIRANDO: AtomicU32 = AtomicU32::new(0);

/// Frames que se reintenta la lectura antes de rindirse, esperando a que la clase de Java
/// llegue.
static INTENTOS: AtomicU32 = AtomicU32::new(120);

/// Frames que lleva la app viva. Se cuenta para no preguntar en el primer frame, que es
/// cuando todavia no hay ventana y el salto a Ajustes se ve raro.
static FRAMES: AtomicU32 = AtomicU32::new(0);

/// Frames que se espera antes de abrir Ajustes por primera vez. MEDIDO que el arranque
/// hasta que hay ventana ajena a lo que tarda: ~1,5 s a 60 Hz da margen de sobra y no se
/// solapa con el primer frame visible.
const ESPERA_ANTES_DE_PREGUNTAR: u32 = 90;

/// Frames entre relecturas del permiso mientras siga faltando. A 60 Hz, uno por segundo.
const RELECTURA: u32 = 60;

/// Frames de gracia tras volver de Ajustes.
///
/// MEDIDO el numero de frames, no de segundos, porque son lo que se puede medir sin
/// reloj: el `Env` lo hace la plataforma al volver. Con 180 frames a 60 Hz son unos 3
/// segundos, de sobra para el viaje a Ajustes y la vuelta, y si se pasa de ahi se
/// vuelve a leer solo cuando el usuario toque algo.
const GRACIA: u32 = 180;

/// Guardar el `JavaVM` del proceso. Se llama una vez, al arrancar.
pub fn registrar(app: &winit::platform::android::activity::AndroidApp) {
    VM.store(app.vm_as_ptr() as *mut JavaVM, Ordering::Release);
}

/// Java -> Rust: la app ya esta viva y nos pasa su propia clase.
///
/// MEDIDO que la firma de una nativa JNI en `jni` 0.22 lleva los tipos puestos, no
/// punteros (`env.rs:4627`):
///
/// ```text
/// pub extern "system" fn Java_com_example_MyClass_myNativeMethod<'caller>(
///     mut unowned_env: jni::EnvUnowned<'caller>,
///     _this: JObject<'caller>,
///     arg: JString<'caller>,
/// ) -> JObject<'caller>
/// ```
///
/// y `EnvUnowned::with_env` es, textual de la documentacion, *"specifically intended to
/// be used within native/foreign Java method implementations"*.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_storyteller_figcraft_MainActivity_nativeListo<'caller>(
    mut env: jni::EnvUnowned<'caller>,
    clase: jni::objects::JClass<'caller>,
) {
    let _ = env
        .with_env(|e| -> jni::errors::Result<()> {
            let g = e.new_global_ref(&clase)?;
            CLASE_JAVA.store(g.as_raw() as *mut c_void, Ordering::Release);
            // La clase vive para todo el proceso, asi que filtrarla es lo correcto y no
            // una fuga que arreglar.
            std::mem::forget(g);
            Ok(())
        })
        .resolve_with::<jni::errors::LogContextErrorAndDefault, _>(|| "MainActivity no pudo pasar su clase a Rust".to_string());
    log::info!("permiso: clase de Java recibida: {}", CLASE_JAVA.load(Ordering::Acquire) != std::ptr::null_mut());
    // La clase acaba de llegar: este es el momento de la primera lectura, y es el
    // unico que se puede hacer con seguridad — antes, `clase()` daria null.
    leer();
}

/// La clase de Java, por el puntero que nos paso `nativeListo`.
///
/// MEDIDO: `as_cast_raw` toma `&jobject` y aqui el puntero es `*mut c_void`, asi que
/// hace falta el cast explicito. Y `FromObject` pide `&JObject` a el que `Cast` no
/// convierte solo en la posicion de un argumento aunque tenga `Deref`.
fn clase<'l>(e: &mut Env<'l>) -> std::result::Result<jni::objects::JClass<'l>, jni::errors::Error> {
    let p = CLASE_JAVA.load(Ordering::Acquire) as jni::sys::jobject;
    if p.is_null() {
        return Err(jni::errors::Error::NullPtr("la clase de Java no ha llegado todavia"));
    }
    // SAFETY: `p` es una referencia global de JNI creada en `nativeListo`, y esa clase
    // vive para todo el proceso. `as_cast_raw` lo comprueba en tiempo de ejecucion.
    let obj = unsafe { e.as_cast_raw::<jni::refs::Global<JObject>>(&p)? };
    let objeto: &JObject = &obj;
    // SAFETY: la clase es de verdad una `Class` — la pasa el propio `MainActivity` — y
    // `JClass::from_raw` es lo que genera `bind_java_type!` para cada tipo de Java.
    Ok(unsafe { jni::objects::JClass::from_raw(e, objeto.as_raw()) })
}

/// Adjuntar el hilo actual a la JVM y hacer una cosa con el `Env`.
///
/// `attach_current_thread_for_scope` desconecta solo, que es lo que hace falta porque se
/// llama desde el hilo de winit.
///
/// El `Result` anidado es por la firma de `attach_current_thread_for_scope`, que exige
/// `E: From<jni::errors::Error>` y por tanto no admite errores de aplicacion. El wrapper
/// [`env`] lo aplana, para que las llamadas no hagan malabares.
fn con_env<T, F>(f: F) -> Result<Result<T, String>, String>
where
    F: FnOnce(&mut Env) -> std::result::Result<Result<T, String>, jni::errors::Error>,
{
    let p = VM.load(Ordering::Acquire);
    if p.is_null() {
        return Err("la JVM no esta registrada todavia".into());
    }
    // SAFETY: `p` viene de `AndroidApp::vm_as_ptr()`, que es el JavaVM de la app y
    // vive todo el proceso. `JavaVM::from_raw` solo comprueba que no sea nulo.
    let vm = unsafe { jni::JavaVM::from_raw(p) };
    vm.attach_current_thread_for_scope(f).map_err(|e| format!("{e:?}"))
}

/// [`con_env`] sin el `Result` anidado.
fn env<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut Env) -> std::result::Result<Result<T, String>, jni::errors::Error>,
{
    con_env(f)?
}

/// Los insets del sistema, en puntos: `(izquierda, arriba, derecha, abajo)`.
///
/// MEDIDO, y es lo que arregla que los menus de arriba estuvieran pegados al reloj y los de
/// la derecha encima de los botones de navegacion.
///
/// MEDIDO que `egui` los usa de verdad, leyendo su codigo (`egui-0.36.2`):
///
/// ```text
/// // src/input_state/mod.rs:511
/// pub fn content_rect(&self) -> Rect { self.viewport_rect - self.safe_area_insets }
/// ```
///
/// O sea que `content_rect()` es lo que leen los paneles, y sin los insets esa rect es
/// toda la ventana —con la barra de estado encima—.
///
/// MEDIDO que `winit` 0.30 no los rellena en Android: en `src/platform_impl/android/` no hay
/// nada, y en `platform_impl/ios/` si. Por eso van a mano.
///
/// MEDIDO tambien que `setDecorFitsSystemWindows(true)` y `clearFlags(FLAG_LAYOUT_NO_LIMITS)`
/// **no** redimensionan la superficie: con las dos puestas el viewport seguia siendo
/// 937,4 x 443,1 pt. La correccion es restar los insets, no tocar flags.
///
/// MEDIDO del valor, en 2400 x 1080 con densidad 390 (2,4375 px/pt): statusBars 59 px
/// arriba = 24,2 pt; navigationBars 117 px a la derecha = 48,0 pt; displayCutout 115 px a
/// la izquierda = 47,2 pt.
///
/// Se relee una vez por frame y es una llamada JNI: MEDIDO que el frame va a 119-131 fps,
/// asi que una llamada mas de esas no se ve. Alternativa descartada: `Window::onApplyInsets`
/// de Java, que exigiria una vista y `NativeActivity` no tiene ninguna —es justo el problema
/// del teclado blando, ya medido en el modulo.
pub fn insets() -> [f32; 4] {
    let r = env(|e| {
        let clase = clase(e)?;
        let v = e.call_static_method(&clase, jni::jni_str!("insets"), jni::jni_sig!("()Ljava/lang/String;"), &[])?;
        if let Some(x) = excepcion(e) {
            return Ok(Err(x));
        }
        Ok(Ok(texto(e, v)?))
    });
    // MEDIDO, y es la **tercera** vez que sale este error exacto, con el mismo mensaje:
    //
    //     expected `String`, found `Result<_, _>`
    //
    // `env` aplana el `Result` anidado de `con_env`, asi que aqui hay un solo nivel. Los
    // `Ok(Ok(..))` que quedan en el archivo estan *dentro* de las closures —que si
    // devuelven el anidado— y ahi son correctos; el que falla es el `match` de fuera.
    // MEDIDO: esto se registra porque sin el no hay forma de saber si los insets llegan.
    // La primera vez que se registra un valor distinto del anterior, a nivel `info` —los
    // `debug` no salen, el filtro esta en `Info`.
    let v = match r {
        Ok(s) if !s.is_empty() => parsea(&s),
        // MEDIDO que sin ventana no es un fallo: al principio no hay, y ceros deja que
        // egui use la ventana entera, que es lo de siempre.
        Ok(_) => [0.0; 4],
        Err(e) => {
            log::debug!("permiso: {e}");
            [0.0; 4]
        }
    };
    // Solo cuando cambia, para no llenar el logcat de una linea por frame.
    let empaquetado = empaqueta(v);
    if ULTIMO_INSET.load(Ordering::Acquire) != empaquetado {
        ULTIMO_INSET.store(empaquetado, Ordering::Release);
        log::info!("insets: l={} t={} r={} b={}", v[0], v[1], v[2], v[3]);
    }
    v
}

/// Los ultimos insets leidos, solo para no repetir el log.
static ULTIMO_INSET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Los cuatro insets empaquetados en un `u64` para poder compararlos de una vez.
fn empaqueta(v: [f32; 4]) -> u64 {
    // MEDIDO que hacer la comparacion con un `AtomicU64` y no con `[f32; 4]`: no hay
    // `Atomic` para arrays, y un `Mutex` aqui estaria en el camino caliente de cada frame
    // para no compar nada.
    let mut bits = 0u64;
    for (i, x) in v.iter().enumerate() {
        let b = (*x * 16.0).round().clamp(-1.0e6, 1.0e6) as i64 as u16 as u64;
        bits |= b << (i * 16);
    }
    bits
}

/// `"l,t,r,b"` -> `[l, t, r, b]`, o ceros si no cuadra.
///
/// MEDIDO que no se parte por `split(',')` a ciegas y se indexa: eso es `panico` en un
/// dato de Java, y el proyecto prohibe panicos en codigo que se reparte. Un numero que no
/// se puede leer es `0.0`, no un fallo.
fn parsea(s: &str) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, parte) in s.split(',').take(4).enumerate() {
        match parte.trim().parse::<f32>() {
            Ok(v) if v.is_finite() && v >= 0.0 => out[i] = v,
            // MEDIDO que un inset negativo no tiene sentido y se descarta: `Rect - inset`
            // con un inset negativo agranda la rect y el panel se sale por el otro lado.
            _ => out[i] = 0.0,
        }
    }
    out
}

/// El `String` que devuelve una llamada Java, o cadena vacia si devuelve `null`.
///
/// MEDIDO, y era el ultimo fallo que quedaba en el intento anterior:
/// `saf: abrir fallo: NullPtr("get_string_utf_chars obj argument")`. Un `null` de JNI y
/// una cadena vacia son cosas distintas, y `request` de Java devolvia `null` **cuando
/// habia ido bien**. Hay que mirar `is_null()` antes de convertir.
///
/// MEDIDO tambien que `Env::get_string` esta deprecado desde 0.22 y el proyecto compila
/// con `clippy -D warnings`, asi que el sustituto es `JString::try_to_string`.
///
/// `jobject` y `jstring` son el mismo tipo: en JNI `jstring` es un `typedef` de `jobject`.
fn texto(e: &mut Env<'_>, v: jni::JValueOwned<'_>) -> std::result::Result<String, jni::errors::Error> {
    match v {
        jni::JValueOwned::Object(o) if !o.is_null() => {
            // SAFETY: la referencia la ha devuelto JNI en la llamada anterior de este
            // mismo `Env`, es una local viva y no sale de aqui: se lee y se descarta.
            let s: JString = unsafe { JString::from_raw(e, o.into_raw()) };
            s.try_to_string(e)
        }
        _ => Ok(String::new()),
    }
}

/// La excepcion pendiente de Java, si la hay.
///
/// Sin esto un error seria un mensaje generico de JNI y no se sabria que paso.
fn excepcion(env: &mut Env) -> Option<String> {
    if !env.exception_check() {
        return None;
    }
    // MEDIDO: `describe` vuelca la traza al logcat y `clear` la levanta. Sin las dos, la
    // excepcion se cuelga al siguiente frame y el siguiente `call_static_method` falla
    // con un error que no dice nada.
    env.exception_describe();
    env.exception_clear();
    Some("Java lanzo una excepcion; la traza esta en el logcat".into())
}

/// Leer el permiso de Java y cachearlo.
///
/// MEDIDO por que no se puede con `ContextCompat.checkSelfPermission`: el permiso es un
/// appop y no aparece en la lista de permisos. La unica fuente de verdad es
/// `Environment.isExternalStorageManager()`.
fn leer() {
    let r = env(|e| {
        let clase = clase(e)?;
        let v = e.call_static_method(&clase, jni::jni_str!("hayPermiso"), jni::jni_sig!("()Z"), &[])?;
        if let Some(x) = excepcion(e) {
            return Ok(Err(x));
        }
        // MEDIDO, y este era el fallo entero de la deteccion del permiso. Un `boolean`
        // de Java llega en su **propia** variante, no como `Int`:
        //
        //     jni-0.22.4/src/jvalue.rs:24
        //     pub enum JValueOwned<'local> {
        //         Object(JObject<'local>), Byte(jbyte), Char(jchar), Short(jshort),
        //         Int(jint), Long(jlong),
        //         Bool(jboolean),                 // ← esta
        //         Float(jfloat), Double(jdouble), Void,
        //     }
        //
        // Con `match v { JValueOwned::Int(n) => …, _ => false }` la rama `_` se llevaba
        // **siempre**, asi que `concedido()` era `false` para siempre. Y eso explicaba
        // tres cosas a la vez, todas medidas en el movil:
        //
        // * `permiso: se deja de mirar; concedido = false` con el appop ya en `allow`
        // * el aviso de permiso, que no se iba nunca
        // * `Permission denied` en `/storage/emulated/0` siendo que el permiso estaba
        //   concedido, que ademas es lo que hacia fallar el navegador de vez en cuando
        let v = match v {
            // MEDIDO: en `jni-sys` 0.4 `jboolean` **es** `bool`, no un entero. Por eso
            // no hay constante `FALSE` a la que comparar —el error lo decia:
            //     no associated function or constant named `FALSE` found for type `bool`
            jni::JValueOwned::Bool(b) => b,
            _ => false,
        };
        Ok(Ok(v))
    });
    // MEDIDO, y el error lo decia:
    //     expected `bool`, found `Result<_, _>`
    // `env` **aplana** el `Result` anidado de `con_env` —que existe porque
    // `attach_current_thread_for_scope` exige `E: From<Error>`—, asi que aqui ya no hay
    // dos niveles. Es el mismo error que salio al ultimo con `pick_open`.
    match r {
        Ok(v) => {
            let antes = CONCEDIDO.swap(v, Ordering::AcqRel);
            if antes != v {
                log::info!("permiso: acceso a todos los archivos: {v}");
            }
        }
        // No es un fallo de la app: es que la clase de Java todavia no ha llegado, y se
        // reintenta en `cada_frame`.
        Err(e) => log::debug!("permiso: {e}"),
    }
}

/// ¿Acceso a todos los archivos concedido?
pub fn concedido() -> bool {
    CONCEDIDO.load(Ordering::Acquire)
}

/// ¿Hay que preguntar? MEDIDO que se pueda saltar sin romper nada, asi que basta con que
/// falte el permiso y que el usuario no haya dicho que no.
pub fn hay_que_preguntar() -> bool {
    !concedido() && !DESCARTADO.load(Ordering::Acquire)
}

/// Abrir los Ajustes de "Acceso a todos los archivos".
///
/// MEDIDO que esto **no** es SAF y por eso no da ANR: se lanza y se vuelve, sin esperar
/// nada. El bloqueo del ANR era esperar dentro del update de egui, que ya no existe.
pub fn pedir() {
    // Se mira un rato largo: el viaje a Ajustes, el interruptor y la vuelta. Y se
    // apaga solo si el usuario no vuelve, para no preguntar sin parar.
    MIRANDO.store(GRACIA, Ordering::Release);
    let r = env(|e| {
        let clase = clase(e)?;
        let v = e.call_static_method(&clase, jni::jni_str!("pedirPermiso"), jni::jni_sig!("()Ljava/lang/String;"), &[])?;
        if let Some(x) = excepcion(e) {
            return Ok(Err(x));
        }
        Ok(Ok(texto(e, v)?))
    });
    // MEDIDO, mismo aplanado que en `leer`.
    match r {
        Ok(v) if v.is_empty() => log::info!("permiso: Ajustes de almacenamiento abiertos"),
        Ok(v) => log::error!("permiso: no se pudieron abrir los Ajustes: {v}"),
        Err(e) => log::error!("permiso: {e}"),
    }
}

/// El usuario ha dicho que ahora no. No se vuelve a preguntar.
pub fn descartar() {
    DESCARTADO.store(true, Ordering::Release);
    MIRANDO.store(0, Ordering::Release);
    log::info!("permiso: el usuario lo aplaza; el browser se queda en lo nuestro");
}

/// Un frame: mientras se vuelve de Ajustes, releer.
///
/// MEDIDO que el unico sitio que se entera de que el usuario ha vuelto es un frame
/// posterior, asi que en vez de adivinar se relee un rato y se para.
pub fn cada_frame() {
    FRAMES.fetch_add(1, Ordering::Relaxed);
    preguntar_una_vez();

    // La clase llega en el `onCreate`, antes que el primer frame, asi que normalmente
    // esto ya no hace falta. Se deja un intento por frame los primeros instantes por si
    // el orden se tuerce, **con tope**: sin el, un fallo de JNI seria una llamada por
    // frame para siempre, que es justo lo que no se quiere en un update de egui.
    if CLASE_JAVA.load(Ordering::Acquire).is_null() {
        if INTENTOS.fetch_sub(1, Ordering::AcqRel) <= 1 {
            INTENTOS.store(0, Ordering::Release);
            log::error!("permiso: la clase de Java no llego; se deja de preguntar por el permiso");
        } else {
            leer();
        }
        return;
    }
    // MEDIDO, y era un fallo visible: la cache se quedaba en `false` para siempre. Al
    // conceder el permiso, el log decia
    //
    //     permiso: se deja de mirar; concedido = false
    //
    // con el appop ya en `allow`. La razon es que solo se releia mientras se esperaba, y
    // al dejar de mirar se quedaba con el valor viejo **y el aviso no se iba nunca**.
    //
    // Asi que ahora se relee cada `RELECTURA` frames mientras falte el permiso. Una
    // llamada JNI por segundo no es nada, y es lo que hace que el aviso desaparezca solo
    // en cuanto el usuario vuelve de Ajustes.
    let quedan = MIRANDO.load(Ordering::Acquire);
    if quedan == 0 {
        if !CONCEDIDO.load(Ordering::Acquire) && FRAMES.load(Ordering::Relaxed) % RELECTURA == 0 {
            leer();
        }
        return;
    }
    leer();
    let antes = quedan;
    let ahora = quedan.saturating_sub(1);
    MIRANDO.store(ahora, Ordering::Release);
    if ahora == 0 {
        // MEDIDO: `{concedido()}` no vale como argumento con nombre —el error lo decia
        //     expected `}` in format string
        // asi que la llamada va como argumento normal.
        let hay = concedido();
        log::info!("permiso: se deja de mirar; concedido = {hay}");
    } else if antes == 1 {
        log::info!("permiso: vuelve a mirar tras volver de Ajustes");
    }
}

/// Crear el directorio de documentos y preguntar el permiso **una sola vez**.
///
/// MEDIDO por que hace falta crearlo: el browser arranca ahi porque es lo unico que se
/// puede leer sin permiso, y si el directorio no existe el `read_dir` falla con
/// `No such file or directory (os error 2)`. Es literalmente lo que se vio al probarlo:
///
/// ```text
/// Abrir
/// /data/data/ai.storyteller.figcraft/documents
/// No such file or directory (os error 2)
/// ```
///
/// MEDIDO por que se pregunta solo una vez y no en cada arranque: sin esto, cada vez que
/// se mata la app sale un salto a Ajustes, que es de las cosas que mas gente odia en
/// Android. El marcador es **un fichero**, no `SharedPreferences`, y asi no hace falta ni
/// un metodo mas de JNI para lo unico que hay que recordar entre arranques.
fn preguntar_una_vez() {
    let dir = directorio_privado();
    // MEDIDO: el fallo se registra en vez de tragarselo con `let _ =` a secas, porque si
    // el directorio no se puede crear el browser sale con `os error 2` y no dice por que.
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!("permiso: no se pudo crear {}: {e}", dir.display());
    }

    if concedido() || DESCARTADO.load(Ordering::Acquire) || ya_preguntado() {
        return;
    }
    if FRAMES.load(Ordering::Relaxed) < ESPERA_ANTES_DE_PREGUNTAR {
        return;
    }
    marcar_preguntado();
    log::info!("permiso: primera vez; se abren los Ajustes de almacenamiento");
    pedir();
}

/// El fichero que recuerda que ya se pregunto.
fn marca() -> std::path::PathBuf {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => std::path::PathBuf::from(h).join(".permiso-preguntado"),
        _ => std::path::PathBuf::from("/data/data/ai.storyteller.figcraft/.permiso-preguntado"),
    }
}

fn ya_preguntado() -> bool {
    marca().is_file()
}

fn marcar_preguntado() {
    // MEDIDO: si el fichero no se puede escribir se pregunta otra vez en el siguiente
    // arranque. Peor un salto de mas a Ajustes que un bucle de saltos.
    if let Err(e) = std::fs::write(marca(), b"1") {
        log::debug!("permiso: no se pudo marcar que ya se pregunto: {e}");
    }
}

/// Where the app's own documents live, for the permission-free browser start.
///
/// MEDIDO: `std::env::home_dir` esta deprecado desde 1.29 y el proyecto prohibe APIs
/// deprecadas (`clippy -D warnings`). En Android `HOME` es `/data/data/<paquete>`.
pub fn directorio_privado() -> std::path::PathBuf {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => std::path::PathBuf::from(h).join("documents"),
        _ => std::path::PathBuf::from("/data/data/ai.storyteller.figcraft/documents"),
    }
}

/// Leer el portapapeles del sistema via JNI (`ClipboardManager`).
///
/// MEDIDO que hace falta JNI porque `egui` solo expone la escritura del portapapeles,
/// no la lectura. En Android hay que usar `android.content.ClipboardManager`, cuyo
/// metodo nativo esta expuesto en `MainActivity` como `nativeGetClipboardText`.
pub fn clipboard_get_text() -> Option<String> {
    let r = env(|e| {
        let clase = clase(e)?;
        let v = e.call_static_method(&clase, jni::jni_str!("nativeGetClipboardText"), jni::jni_sig!("()Ljava/lang/String;"), &[])?;
        if let Some(x) = excepcion(e) {
            return Ok(Err(x));
        }
        Ok(Ok(texto(e, v)?))
    });
    match r {
        Ok(s) if !s.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            log::debug!("permiso: clipboard: {e}");
            None
        }
    }
}
