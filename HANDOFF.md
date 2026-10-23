# FigCraft en Android — TRASPASO de sesión

> **Lee esto primero si eres un agente nuevo en esta sesión.** Todo el contexto está aquí.
> Última actualización: 2026-10-23, commit `538ad16a`.
> Las secciones de 2026-10-20 son el estado anterior; la sección de 2026-10-23 manda.

---

## Estado al 2026-10-23 — menús táctiles y preferencias

- **Instalado en el Moto**: APK de **`538ad16a`** (CI `39903444700` verde; APK `39903999864`).
- **Menú `@`**: el deadlock de `released_outside` (`30ad2665`) y el nivel `At` del submenú
  (`5be88c3f`) están corregidos. Verificado en el móvil: File/Preferences abren su submenú y
  "Preferences..." abre el diálogo.
- **Long press**: la suelta que termina el gesto ya no cierra el menú contextual. `facfdd0c`
  añade `context_menu_ignore_release`: se ignora la primera suelta (la del gesto que abrió el
  menú) y se vuelve a cerrar con la siguiente. Verificado 4/4 en el móvil con un desplazamiento
  de 7 px arriba-izquierda al soltar, más la prueba headless
  `layers_keyboard_tests::a_long_press_release_does_not_close_the_context_menu`.
- **Preferences**: el toggle se autoguarda sin pulsar Save; `state.json` cambia y el valor
  sobrevive a `am force-stop` + relanzar. El bloque de autoguardado sigue tras `frame` (el
  diálogo se dibuja dentro); el intento de moverlo dentro de `frame` se revirtió (`538ad16a`).
- **Cierre fuera**: tocar el lienzo con el menú `@` abierto lo cierra sin congelar ni abortar.
- **Ojo/candado**: siguen visibles en la fila de la capa seleccionada.
- **Pruebas**: CI `39903444700` verde; se añadió
  `windows_qa_tests::a_preference_toggle_is_written_without_the_save_button`; el test de File
  hace scroll al final para encontrar Open recent; los `eprintln!` de diagnóstico se quitaron.
- **Siguiente**: verificar el selector de color Custom (altura/scroll) y el long press en la
  fila de capas; después decidir si se portan las piezas de 0.8.0 (Lote 1: `#867`, `#817` +
  JNI clipboard, `#900`, `#861`; `#865` opcional; luego `#832`).

---

## Estado al 2026-10-20 — táctil: nodos vs cuadritos

- **Rutas**: las `/root/...` que nombra este doc ya no existen; el clon de trabajo es
  **`/tmp/opencode/repos/figcraft-android`** (y `/tmp/opencode/repos/vectorcraft-android`).
- **Instalado en el Moto**: APK de **`8a496de`** (encima de `487e170` + `7f79ad0`).
- **Doble toque** (`7f79ad0`) entra al editor de nodos; log `figcraft_ui_egui::can..` → `DOBLE TOQUE x,y`.
- **Nodo manda** (`487e170`): en el editor de nodos el círculo gana a la esquina de la caja; la caja
  solo responde fuera del nodo (tolerancia táctil 12 u ≈ 29 px en `vectoredit::pointer::hit_tol`).
- **Cuadritos fantasma ocultos** (`8a496de`): en el editor de nodos la caja ya **no dibuja cuadritos**
  (solo contorno + medida). El dueño confirmó: los que no servían son los del **selector/box**; los
  que mandan son los **círculos** (nodos). En selección normal los cuadritos siguen y funcionan.
- **Si mañana hace falta el cuadrito visible** para escalar puntos (p. ej. al seleccionar la arista
  entre 2 nodos): plan listo = dibujar **por esquina** solo las no tapadas por nodo/manecilla con un
  `Session::vector_edit_box_live()` que reutilice `vectoredit::hit()` + `hit_tol()`. No implementado.
- **Tool Direct Select**: descartada por ahora; solo se retomaría si se cambia el método de entrada
  al editor (el doble toque ya funciona).
- **Móvil**: `adb connect 192.168.100.51:43263` (el puerto cambia; `adb devices`). Capturas 2400×1080.
  `gh` siempre con `--repo apexmiguel9-hub/figcraft-android`.
- **Cola siguiente**: subida parcial de textura (`ImageDelta`), caché del glass por (forma, tamaño,
  escala), tap de prototipos "on click" en táctil, texto invisible al escribir, menú contextual sin
  scroll, diálogo de Preferences grande.

---

## 0. Lo primero: qué se borró y hay que rehacer

El dueño va a **rootear el Moto G56 5G**, y **eso borra los datos de la app instalada**.
Esto significa:

| Antes | Después del root |
|---|---|
| `ai.storyteller.figcraft` instalado y funcionando | **Desinstalado** — hay que reinstalar |
| adb por TCP `192.168.100.51:32959` | **IP y puerto cambiados** — hay que redescubrir |
| Los ficheros del usuario en la app | **Borrados** |

**Nada de esto afecta al repo**: el código está en GitHub, no en el móvil. Solo hay que
reinstalar el APK cuando el rooting termine.

### Cómo redescubrir el móvil

```bash
adb kill-server && adb start-server
adb devices -l
# si sale (IP):PUERTO → ese es el móvil
# si no sale nada: en el móvil, Depuración USB → "Dispositivo por TCP/IP", o
#   adb connect <ip-del-movil>:5555
```

Si el rooting cambia el modo de depuración, quizá haya que activar
*Ajustes → Acerca de → Pulsar 7 veces "número de compilación"* → *Opciones de desarrollador →
Depuración USB*.

### Reinstalar la app

```bash
# 1. Bajar el APK (ver §5; `gh run download` NO funciona, hay que usar la API)
# 2. Instalar
adb install -r app-debug.apk
# 3. Si falla con INSTALL_FAILED_UPDATE_INCOMPATIBLE (firma distinta):
adb uninstall ai.storyteller.figcraft && adb install app-debug.apk
# 4. Lanzar y ver que vive
adb logcat -c && adb shell am start -n ai.storyteller.figcraft/.MainActivity
sleep 5 && adb shell pidof ai.storyteller.figcraft   # debe imprimir un PID
```

---

## 1. Dónde está cada cosa en esta máquina

| Ruta | Qué es | ¿Se edita? |
|---|---|---|
| **`/root/figcraft-android`** | **El fork. Aquí se trabaja.** Upstream `storytold/figcraft` + el port de Android | ✅ **SÍ** |
| `/root/figcraft` | Copia **prístina** del upstream, sin modificar | ❌ **NO** — es solo referencia |
| `/root/vectorcraft-android` | El port de vectorcraft, **la plantilla** que se copió | ❌ Solo leer (`PORT.md` tiene las lecciones) |
| `/tmp/opencode/watch-android-build.sh` | Script que vigila un workflow y avisa con bell | — |
| `/tmp/opencode/apk/app-debug.apk` | Último APK que se instaló (puede estar viejo) | — |

### Remotes de `/root/figcraft-android`

```
origin    -> https://github.com/apexmiguel9-hub/figcraft-android.git   (este fork)
upstream  -> https://github.com/storytold/figcraft.git                 (el original)
```

**Para traer cambios de upstream:**
```bash
cd /root/figcraft-android
git fetch upstream && git merge upstream/main    # NUNCA rebase
```

---

## 2. Qué es este proyecto (en una pantalla)

Port de **FigCraft** (clon de Figma, 100 % Rust, del study de `storytold`) a **Android**, usando
la plantilla del port de **VectorCraft** a Android que ya tenía el dueño.

* **Repo**: `apexmiguel9-hub/figcraft-android`
* **Base**: `storytold/figcraft` **v0.6.0** (alpha, upstream dice ~53 % "ready for real work")
* **Mismo stack que los dos**: `eframe 0.36` + `egui 0.36` + `winit 0.30` + `wgpu 30.0.1`
* **Los dos rasterizan en CPU** (`vello_cpu` en vectorcraft, `tiny-skia` en figcraft), la GPU solo
  presenta. Por eso se espera **120 fps** en el móvil.
* **Tamaño del port**: ~1.500 líneas sobre las 215.422 de figcraft = **0,7 %**
* **Propietario**: los tres repos (`vectorcraft-android`, `figcraft-android`, y el trabajo de
  drawable/Compose) son del **dueño**, no de `storytold`. Upstream no los quiere.

---

## 3. Estado al dejar esta sesión

### Funciona (verificado en el Moto antes del rooting)

| | |
|---|---|
| `.so` arm64-v8a compila | ✅ 5-12 min en frío |
| APK debug se monta e instala | ✅ 1-2 min |
| **Arranca y dibuja la UI completa** | ✅ rail, panel de capas, canvas, panel derecho, pestañas, Share |
| Fuentes sin panic | ✅ (arreglado, ver §4.1) |
| 120 fps | ⏳ **sin medir** |

Los tres workflows: `build-android-so` ✅, `build-apk` ✅, `CI`/`packaging-lint` ✅ (en el
último commit que compiló bien).

### ⚠️ Lo único que NO se sabe: el estado del último commit

El commit `b782b67` ("TouchPhase::Move, no Moved") se subió y quedó un workflow corriendo, pero
**no se vio el resultado**. Es un cambio de una línea, pero **no está verificado**.

**Primera tarea de la nueva sesión: mirar si el último pushselleró.**

```bash
gh run list --repo apexmiguel9-hub/figcraft-android --limit 5
```

### ⚠️ PENDIENTE CRÍTICO: la API de `egui_file` no está verificada

**Esto es lo más importante que queda.** `browser.rs` tiene ~50 líneas escritas **a ciegas**
contra `egui_file 0.28`, un crate que:

1. **no está descargado** en esta máquina (`cargo fetch` se intent�� pero se interrumpió)
2. **no se pudo compilar** en el CI todavía, porque el primer error fueron las APIs de egui
   (que están antes en el orden de compilación)

Los métodos que **supongo** que existen y **no he comprobado**:
`FileDialog::new`, `.title()`, `.pick_folder()`, `.show_new_folder()`, `.add_file_filters()`,
`.default_filename()`, `.initial_path()`, `.open()`, `.show()`, `.update()`, `.path()`,
`.directory()`, y el enum `Eleccion` con su variante `Aceptada`.

**Cómo verificarlo** (descargar el fuente y leerlo, como se hizo con `egui_file/src/lib.rs:231`):
```bash
cd /root/figcraft-android
export PATH="$HOME/.cargo/bin:$PATH"
cargo fetch                    # baja el .crate de egui_file 0.28.0
D=$(find /root/.cargo/registry/src -maxdepth 2 -name "egui_file-0.28*" -type d | head -1)
grep -n "pub fn " "$D/src/lib.rs"       # la API real
```

Si algo no existe, corregir en `apps/figcraft-android/src/browser.rs`. Nota: el
`add_file_filters` con `&'static [(&str, &[&str])]` es el punto más sospechoso, porque
`egui_file` suele tomar `&[(&str, &[&str])]` con lifetimes distintos.

---

## 4. Los fallos que ya costaron tiempo (no repetirlos)

### 4.1 `FontFamily::Name("semibold") is not bound to any fonts` → abort al arrancar

**El peor de todos.** La app compilaba, instalaba y moría al abrir, sin decir por qué.

```text
epaint-0.36.2/src/text/fonts.rs:1025:41:
FontFamily::Name("semibold") is not bound to any fonts
panic in a function that cannot unwind
Fatal signal 6 (SIGABRT) ... in tid ... (android_main)
```

**Causa**: figcraft usa `FontFamily::Name("semibold")` para `TextStyle::Heading`
(`crates/ui-egui/src/theme.rs:167`) y la registra en `theme::install_fonts`, que
`FigApp::logic` llama **en su primer frame** (`crates/ui-egui/src/lib.rs:1112`). Pero el theme se
aplica **antes** (`theme.rs:1136`), nombrando una familia que todavía no existe.

**Por qué aborta y no se recupera**: `android_main` es `extern "C"`, donde el unwind está
prohibido. Cualquier panic tumba el proceso entero.

**Arreglo** (commit `432d9dd`): `theme::install_fonts(&cc.egui_ctx, false, 0)` en `build()`, que
corre antes de cualquier frame. Preventivo: no hay `catch_unwind` que valga aquí.

### 4.2 La API de egui 0.36 — 3 ciclos de build perdidos

**Lección: leer el código de egui, no adivinar.** Aquí está la API real, verificada:

**`MultiTouchInfo`** (`egui-0.36.2/src/input_state/touch_state.rs`):
```rust
pub start_time: f64, pub start_pos: Pos2, pub center_pos: Pos2,
pub num_touches: usize, pub zoom_delta: f32,      // ← CAMPO, no método
pub rotation_delta: f32, pub translation_delta: Vec2, pub force: f32,
```
* **`zoom_delta` es un campo**, no un método. (Error: *"no method named zoom_delta"*)
* **NO existe `max_movement`.** (Error: *"no field max_movement"*)

**Y el hallazgo que cambió el diseño:**
> `MultiTouchInfo` **solo se crea con dos o más dedos** — *"Value is ≥ 2 since for a single touch
> no MultiTouchInfo is created"* (`touch_state.rs:20`).

Es decir: la rama de gestos táctiles cubre **exactamente** los 2+ dedos (no era un
`if num_touches >= 2` por comodidad), y **la pulsación larga no puede medirse con
`MultiTouchInfo`** porque con un dedo no llega nada. Se mide con `Event::Touch`.

**`TouchPhase`** (`egui-0.36.2/src/data/input/touch.rs:16`) — **`Start` / `Move` / `End` / `Cancel`**:
* No `Started`, no `Moved`, no `Ended`, no `Cancelled`. (Dos ciclos perdidos por esto.)

### 4.3 `no encuentro llvm-strip en el NDK`

El workflow buscaba las herramientas de LLVM solo en `toolchains/llvm/prebuilt`, y la estructura
del NDK varía según `sdkmanager`. Arreglado con un helper `find_ndk_tool()` en cada paso que lo
usa (los pasos de un workflow son **shells independientes**: una función definida en un paso no
existe en el siguiente).

### 4.4 `cannot update the lock file ... --locked`

Añadir dependencias sin regenerar el lock. `cargo generate-lockfile` y subirlo.

### 4.5 Clippy y fmt

* `unused variable: raw` en `raw_input_hook` → `_raw` (el `#[cfg(android)]` interior no lo usa en
  escritorio).
* **`cargo fmt` SIEMPRE antes de commitear.** Reindenta de formas que no coinciden con lo que se
  escribe a mano, y `cargo fmt -- --check` es un gate del CI.

### 4.6 shellcheck en los workflows

El `packaging-lint` de `storytold/craft-common-actions` corre shellcheck con `-D warnings`.
Falla por `SC2086` (variables sin comillas) y `SC2012` (`ls` donde pide `find`).

---

## 5. Cómo compilar y desplegar

### Local: qué funciona y qué NO

| | |
|---|---|
| `cargo fmt` | ✅ **funciona** (no necesita compilar) |
| `cargo check` / `cargo build` | ❌ **NO funciona**: Termux/proot, `winit` no soporta esta plataforma |
| `cargo generate-lockfile` | ✅ funciona |

**Consecuencia**: no se puede verificar nada localmente. Cada error cuesta un ciclo de CI de
~5 min. **Leer el código de las dependencias en `/root/.cargo/registry/src/`** es la única forma
de no fallar a ciegas.

### CI

```bash
gh workflow run build-android-so.yml   # compila la .so  (~5-12 min)
gh workflow run build-apk.yml          # monta el APK     (~1-2 min, se dispara solo tras el .so)
```

**Bajar el APK** — `gh run download` **NO funciona**, hay que usar la API:
```bash
A=$(gh api "repos/apexmiguel9-hub/figcraft-android/actions/runs/$RUN/artifacts" \
      --jq '.artifacts[]|select(.name=="figcraft-android-debug")|.id')
gh api "repos/apexmiguel9-hub/figcraft-android/actions/artifacts/$A/zip" > a.zip
unzip -o -q a.zip    # -> app-debug.apk
```

**Vigilar un build:**
```bash
/tmp/opencode/watch-android-build.sh apexmiguel9-hub/figcraft-android build-android-so.yml 30
```
Sale con código 0 si es success, 1 si failure, y avisa con bell.

### Los tres workflows

| Workflow | Qué hace | Min |
|---|---|---|
| `build-android-so` | `cargo ndk` arm64-v8a, quita solo la tabla DWARF, valida `android_main` + `.text` > 5 MB + `.eh_frame` | 5-12 |
| `build-apk` | baja la última `.so` buena → `jniLibs` → `assembleDebug` → valida `.so` dentro + `.symtab` + `MainActivity` + `aapt` | 1-2 |
| `CI` / `packaging-lint` | fmt, clippy, tests, wasm / shellcheck + actionlint | 2-3 |

**Por qué están separados** (regla que costó sangre en vectorcraft): la `.so` son ~400 crates y
tarda 15 min; Gradle tarda 3. `javac` corre en el del APK y su código **no está en la `.so`**, así
que el del `.so` dio verde dos veces seguidas con el Java roto.

**Aviso importante**: el workflow del APK busca la última `.so` **con éxito**, no la de su commit.
**El APK puede llevar una `.so` vieja**. Comprobar con `strings` lo que se acaba de instalar.

---

## 6. Qué hay implementado (mapa de ficheros)

```
apps/figcraft-android/
  Cargo.toml     lib cdylib `figcraft_android` + bin escritorio
  src/lib.rs     android_main, GLES solo, insets en raw_input_hook, browser+permiso
  src/main.rs    el mismo run() en escritorio, para depurar sin móvil
  src/permiso.rs JNI: nativeListo, hayPermiso, pedirPermiso, insets   (port de vectorcraft)
  src/browser.rs egui_file + Services con pick_open/pick_save         (NUEVO, sin verificar)
  android/       proyecto Gradle, MainActivity, manifest, iconos
  PORT.md        mapa del port (más corto que este documento)

crates/ui-egui/src/canvas.rs   ÚNICO fichero compartido con upstream que se toca:
  - gestos_tactiles()   2 dedos = pan + zoom, con el orden pan→zoom
  - pulsacion_larga()   pulsación larga = clic derecho (menú contextual)
  - gestures_mouse()    zoom de trackpad / rueda, separado para no aplicarlo dos veces
  - CanvasState: + touch_count, + long_press

.github/workflows/build-android-so.yml, build-apk.yml
```

### Los gestos, y por qué el orden pan→zoom importa

**Un dedo = herramienta** (egui-winit emula ratón con el primer dedo → clic, doble clic, arrastre).
**Dos dedos = pan + zoom a la vez**. **Pulsación larga = clic derecho** (el menú contextual es
inalcanzable en táctil si no).

El orden importa y **no es cosmético**: en `vectorcraft` este bug costó lo más caro del port. Con
dos dedos, el zoom iba bien y **el pan no**, y el pan solo aparecía si dejabas los dedos quietos un
segundo. Motivo: el ancla del zoom se calculaba **antes** del pan, así que `before − after` traía un
`−d` que el `+d` del pan cancelaba exactamente.

Arreglo (estructural, no parche): **pan primero, ancla recalculada después, zoom después.**

---

## 7. Los bugs del APK y del navegador (medidos en vectorcraft, aplicables aquí)

* **Abrir cualquier segunda Activity da ANR**: el handshake de `android-activity` bloquea el hilo
  principal de Java hasta que el hilo del bucle confirma la pausa, y ese hilo corre el update de
  egui. Por eso el browser es **propio, pintado en egui**, y no SAF.
* **El `NativeActivity` carga la `.so` por su cuenta** pero por el camino nativo de
  `ANativeActivity`, que **no se le comunica a ART**: sin `System.loadLibrary` explícito en el
  `static {}`, ninguna nativa de la clase resuelve.
* **Las cinco vías de JNI**: `find_class`, `load_class` con `LoaderContext::None` y
  `FromObject` fallan con `NoClassDefFound` desde el hilo nativo. **La que funciona es la quinta**:
  que `onCreate` pase su propia clase a `nativeListo`.
* **`granted=false` no significa denegado**: `MANAGE_EXTERNAL_STORAGE` es un **appop**, no un
  permiso normal. Lo que decide es `appops: allow`.
* **Recortar `screen_rect`, NO rellenar `safe_area_insets`**: `content_rect()` lo usan los
  `Window`; los paneles salen de `available_rect()`, que viene de `screen_rect`. Rellenarlos
  además se lo restaría dos veces a los `Window`.
* **`egui_file` pide doble toque para entrar en una carpeta** (`lib.rs:813` selecciona, `:827`
  entra), y en un dedo solo se registra si los dos caen dentro del tiempo de egui. O sea: "a veces
  sí, a veces no". Hay que arreglarlo **sin tocar el crate**.
* **`selection()` viene vacío en un diálogo de guardar**: solo devuelve ficheros *marcados*. Para
  guardar hay que usar `path()`, que sí es la ruta completa.
* **Un `cdylib` de Rust sin `#[no_mangle]` sale VACÍO** (5,8 MB con 566 KB de `.text`). El enlazador
  borra lo no alcanzable desde las exportaciones. El CI comprueba la tabla de símbolos por eso.

---

## 8. Hoja de ruta

### Fase 1 — que sea usable con el dedo

1. **Verificar que el último commit compiló** (§3).
2. **Verificar la API de `egui_file`** (§3) — bloqueante, nada de browser funciona hasta ahí.
3. **Probar los gestos con un dedo en el móvil**: pan de 2 dedos, zoom, pulsación larga → menú,
   doble tap → editor de nodos.
   *Dato*: en PC el modo vector se entra con **doble clic** (`crates/engine/src/tools.rs:1844` →
   `vectoredit::begin_many`), y requiere que la capa **ya esté seleccionada**. Con el ratón
   emulado por egui, el doble tap debería llegar solo; falta comprobarlo.
4. **Teclado blando — bloqueante.** Con `native-activity` **no aparece** en Android moderno
   (`NativeActivity` no tiene vista para el `InputMethodManager`). Sin teclado no se escribe texto,
   y un editor de diseño sin texto no sirve.
   ```toml
   # Cargo.toml
   eframe = { ..., features = ["game-activity"] }     # en vez de "android-native-activity"
   android-activity = { version = "0.6", features = ["game-activity"] }
   ```
   ```gradle
   // app/build.gradle
   implementation 'androidx.games:games-activity:4.4.0'
   ```
   ```java
   // MainActivity: extends GameActivity en vez de NativeActivity
   ```
5. **Medir fps** y meter un log como el de vectorcraft (`render 5,4 ms · ui 1,5 ms · 124 fps`).
6. **Portapapeles de lectura**: `clipboard_get_text()` devuelve `None`. Se resuelve con JNI
   (`android.content.ClipboardManager`).

### Fase 2 — que se sienta como una app de móvil

7. **Paneles como hojas**: hoy los paneles laterales (240 pt) se comen media pantalla. En vertical
   deberían ser hojas a pantalla completa con swipe para cerrar, y el rail, un bottom bar.
8. **Barra de herramientas adaptativa**: los 11 botones no caben en 393 pt de ancho en vertical.
9. **Swipe en el panel de capas** para reordenar, y swipe horizontal en las pestañas.
10. **Zoom de los paneles**: a densidad 2,5 un control de 4 pt son 10 px, que no se tocan.

### Fase 3 — lo que pidió el dueño (no está en upstream)

Orden por rentabilidad: **11 → 10 → 9**, porque el 11 usa el 10.

9. **Export a `.xml` drawable** (VectorDrawable) — **4-8 h**
   Sin librerías externas: es XML puro, y `figcraft_svg::try_export_svg` ya calcula el `pathData`.
   Añadir `ExportFormat::VectorDrawable` a `crates/doc/src/props.rs:1229` +
   `file.exportVectorDrawable`.
   *Límite*: solo iconos y vectores. No lleva layout, ni blur, ni auto layout; el texto va a
   outlines y los degradados son limitados.

10. **UI a código Compose Kotlin** — **3-6 h**
    Ya existe `CodeLang::Compose` en `crates/engine/src/codegen.rs:987` (auto layout → `Column`/
    `Row`, shape → `Box`, instancias → calls), que se usa con `inspect.code {language: compose}`.
    Falta el paso de **exportar el frame entero**, con variables a `Color.kt`/`Theme`.

11. **Plantilla Android completa en un `.zip`** — **8-15 h**
    Lo que más renta: `MainActivity.kt` + un `.kt` por frame + `res/drawable/*.xml` (del VD) +
    `res/values/colors.xml`/`themes.xml` (de variables) + `AndroidManifest.xml`, empaquetado con
    `zip` (ya es dependencia del workspace). Listo para abrir en Android Studio y compilar.
    *La lógica* (viewmodels, estados, `onClick` reales) es otra historia: v1 solo
    `onClick { /* TODO */ }`.

### Fase 4 — sincronizar con upstream

12. Upstream está en **v0.6.0 y en alpha**, así que se mueve. El `PORT.md` de vectorcraft tiene el
    mapa de los conflictos que caen siempre (`crates/ui-egui/src/canvas.rs`,
    `crates/ui-egui/src/dialogs/*.rs`, `Cargo.lock`) y la lección: **siempre `git merge`, nunca
    rebase**, y `Cargo.lock` **se regenera con un workflow, nunca a mano**.

---

## 9. Cosas del entorno que sorprenden

* **No hay display.** Es un contenedor. adb sí funciona (el móvil va por TCP), pero no hay
  navegador ni nada gráfico.
* **`adb` está instalado** pero el móvil hay que conectarlo por TCP, no por USB.
* **No hay `cargo check` funcional.** Solo `cargo fmt` y `cargo generate-lockfile`.
* **El workspace prohíbe `panic`, `unwrap`, `expect`, `todo`, `unimplemented`, `unreachable`** con
  clippy `-D warnings`. Salen como error si aparecen en código de distribución.
* **`unsafe_code = "forbid"`** en el workspace, y por eso `apps/figcraft-android/Cargo.toml`
  tiene `[lints] workspace = true` **comentado**: `#[unsafe(no_mangle)]` en `android_main` lo
  necesita. Es el patrón del ejemplo oficial de egui (`eframe/examples/hello_android`).

---

## 10. Preguntas de arquitectura que siguen abiertas

* **¿Dónde va la lógica de UI en un proyecto que tiene `.fig` y export a drawable?** El
  `EXPORT_FORMAT` es el punto: hoy `ExportFormat` son png/jpg/svg/pdf/webp, y `file.export` es
  donde se metería `VectorDrawable` y el `.zip` de Android.
* **El `.zip` de plantilla Android necesita `zip` con las entradas correctas** (`res/drawable/…`,
  `app/src/main/java/…`). `zip` ya está en el workspace (`Cargo.toml`), pero hay que comprobar si
  soporta lo que hace falta o si hace falta `zip` con más features.
* **Los 400 crates compilan para Android, pero `tiny-skia` necesita SIMD** y en aarch64 va por
  NEON. No se ha comprobado si el render es igual de rápido en Android que en PC.

---

**Resumen para la nueva sesión:** el repo está en GitHub, el código del port está escrito pero
**`browser.rs` nunca se ha compilado** (API de `egui_file` sin verificar), los gestos están
escritos pero **nunca se han probado con un dedo**, y **el móvil se reinstala desde cero**
porque el dueño lo va a rootear. Lo primero: comprobar el último CI y verificar `egui_file`.