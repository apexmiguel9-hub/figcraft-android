# FigCraft en Android — estado, fallos y hoja de ruta

Fork de [`storytold/figcraft`](https://github.com/storytold/figcraft) v0.6.0 con la plantilla de
port de Android copiada de [`apexmiguel9-hub/vectorcraft-android`](https://github.com/apexmiguel9-hub/vectorcraft-android)
(que es a su vez un fork de `storytold/vectorcraft`, base `v0.7.0`).

Plantilla: `vectorcraft-android/PORT.md`. Mapa de ficheros: [`apps/figcraft-android/PORT.md`](apps/figcraft-android/PORT.md).

---

## 1. Dónde está

| | |
|---|---|
| Repo | `apexmiguel9-hub/figcraft-android` |
| Base | `storytold/figcraft` v0.6.0 (alpha, ~53% ready for real work) |
| Upstream | `git remote rename origin upstream` → `https://github.com/storytold/figcraft.git` |
| Plantilla | `apexmiguel9-hub/vectorcraft-android` (mismo dueño, mismo stack) |
| Dispositivo de pruebas | **Moto G56 5G**, Android 16, 1080×2400, adb por TCP `192.168.100.51:32959` |

**Mismo stack que los dos proyectos** (`eframe 0.36` + `egui 0.36` + `winit 0.30` + `wgpu 30.0.1`), y
los dos rasterizan en **CPU** (`vello_cpu` en vectorcraft, `tiny-skia` en figcraft), con la GPU
solo presentando. Por eso se espera el mismo comportamiento: **120 fps**.

Tamaño: el port son **~1.500 líneas** (350 de Rust + Gradle/Java/manifest) sobre las **215.422** de
figcraft — un **0,7 %**.

---

## 2. Estado: funciona

| | |
|---|---|
| `.so` arm64-v8a | ✅ compila, ~5-12 min en frío |
| APK debug | ✅ se monta y se instala |
| Arranca en el Moto | ✅ **la UI completa se ve** |
| 120 fps | ⏳ sin medir todavía |

Captura del móvil (todo el editor: rail de iconos, panel de capas, canvas con su barra de
herramientas, panel derecho con Design/Prototype/Export, barra de pestañas y Share):

```
ai.storyteller.figcraft/.MainActivity  →  vivo, sin panic
```

---

## 3. Fallos que ya están resueltos

Merecen estar escritos porque **cada uno costó un ciclo de build de 10 minutos**, y ninguno
era visible leyendo el código.

### 3.1 `FontFamily::Name("semibold") is not bound to any fonts` → abort al arrancar

**El peor de todos**: la app compilaba, instalaba, y moría al abrir sin decir por qué.

```text
epaint-0.36.2/src/text/fonts.rs:1025:41:
FontFamily::Name("semibold") is not bound to any fonts
panic in a function that cannot unwind
Fatal signal 6 (SIGABRT) ... in tid ... (android_main)
```

**Causa**: figcraft usa `FontFamily::Name("semibold")` para `TextStyle::Heading`
(`crates/ui-egui/src/theme.rs:167`), y la registra en `theme::install_fonts`, que
`FigApp::logic` llama **en su primer frame** (`crates/ui-egui/src/lib.rs:1112`). Pero el theme se
aplica **antes** (`theme.rs:1136`), nombrando una familia que todavía no existe.

**Por qué aborta y no se recupera**: `android_main` es `extern "C"`, donde el unwind está
prohibido. Cualquier panic tumba el proceso entero.

**Arreglo** (commit `432d9dd`): llamar `theme::install_fonts(&cc.egui_ctx, false, 0)` en
`build()`, que corre antes de cualquier frame y elimina la carrera. Es **preventivo**: no hay
`catch_unwind` que valga aquí.

### 3.2 `no encuentro llvm-strip en el NDK`

El workflow buscaba las herramientas de LLVM solo en `toolchains/llvm/prebuilt`, y la estructura
del NDK varía según la versión de `sdkmanager`. Arreglado con un helper `find_ndk_tool()` que
prueba varias rutas y hace `find` en todo el NDK, definido en cada paso que lo usa (los pasos de
un workflow son shells independientes).

### 3.3 `no puedo actualizar el lock file ... --locked`

Añadir dependencias al port sin regenerar `Cargo.lock`. Se regenera con `cargo generate-lockfile`
y se sube.

### 3.4 Clippy y fmt

* `unused variable: raw` en `raw_input_hook` (el `#[cfg(target_os = "android")]` interior no lo
  usa en escritorio) → `_raw`.
* `cargo fmt` reformatea de forma distinta a como se escribe a mano. **Siempre `cargo fmt` antes
  de commitear**; local funciona (no necesita compilar).

### 3.5 shellcheck en los workflows

El `packaging-lint` de `storytold/craft-common-actions` corre shellcheck con `-D warnings`.
Fallaba por `SC2086` (variables sin comillas) y `SC2012` (`ls` donde pide `find`).

---

## 4. Fallos abiertos

### 4.1 API de `MultiTouchInfo` en egui 0.36 — **corregido, sin verificar en el móvil**

Tres errores, todos por no haber leído el código de egui:

| Error | Realidad (`egui-0.36.2/src/input_state/touch_state.rs`) |
|---|---|
| `no method named zoom_delta` | Es un **campo**, no un método (`:36`) |
| `no field max_movement` | **No existe** en 0.36 |
| `expected Option<Pos2>, found Pos2` | Firma equivocada |

Y el hallazgo que **cambió el diseño**:

> `MultiTouchInfo` **solo se crea con dos o más dedos** — *"Value is ≥ 2 since for a single touch
> no `MultiTouchInfo` is created"* (`touch_state.rs:20`).

O sea: la rama de gestos táctiles cubre exactamente los 2+ dedos (no era un `if num_touches >= 2`
por comodidad), y **la pulsación larga no puede medirse con `MultiTouchInfo`** porque con un dedo
no llega nada. Se mide con `Event::Touch`.

Además `TouchPhase` es `Start` / `Move` / `End` / `Cancel` (`data/input/touch.rs:16`), **no**
`Started` / `Moved` / `Ended` / `Cancelled`.

### 4.2 Sin medir en el móvil

Todo lo siguiente está **escrito pero nunca probado con un dedo**:

* pan de dos dedos
* zoom de dos dedos
* pulsación larga → menú contextual
* doble tap → editor de nodos
* browser (abrir/guardar)
* permiso (el diálogo de Ajustes)
* insets (que la UI no monte sobre la barra de estado)

### 4.3 Teclado blando

`native-activity` **no** muestra el teclado en Android moderno: `NativeActivity` no tiene vista
para el `InputMethodManager`. Sin teclado no se puede escribir texto, que en un editor de diseño
es crítico (nombres de capas, contenido de texto).

**Arreglo** (el del port de vectorcraft, sin probar aquí):
```toml
# Cargo.toml
eframe = { ..., features = ["game-activity"] }   # en vez de "android-native-activity"
android-activity = { version = "0.6", features = ["game-activity"] }
```
```gradle
// app/build.gradle
implementation 'androidx.games:games-activity:4.4.0'
```
```java
// MainActivity: extends GameActivity en vez de NativeActivity
```

### 4.4 Portapapeles sin lectura

`Services::clipboard_get_text()` devuelve `None`: `egui` solo expone la escritura. Pegar un color
o un SVG de otra app no funciona. Se puede resolver con JNI (`android.content.ClipboardManager`).

---

## 5. Hoja de ruta

### Fase 1 — que sea usable con el dedo (lo que falta)

1. **Verificar los gestos en el móvil.** Es lo primero y lo más bloqueante: 4.2.
2. **Teclado blando** (4.3). Sin esto no se edita texto, y un editor de diseño sin texto no
   sirve.
3. **Portapapeles de lectura** (4.4), para pegar entre apps.
4. **Medir fps** y meter un log de rendimiento como el de vectorcraft (`render 5,4 ms · ui 1,5 ms
   · 124 fps`).

### Fase 2 — que se sienta como una app de móvil

5. **Paneles como hojas.** Hoy los paneles laterales (240 pt de ancho) se comen media pantalla
   en un móvil. En vertical deberían ser hojas a pantalla completa con arrastre para cerrar
   (swipe), y el rail de iconos un bottom bar.
6. **Barra de herramientas adaptativa.** La barra inferior de 11 herramientas no cabe en 393 pt
   de ancho; en horizontal cabe, en vertical no.
7. **Swipe en el panel de capas** para reordenar filas arrastrando, y swipe horizontal en las
   pestañas para cambiar de documento.
8. **Zoom del panel.** Los paneles usan el tamaño fijo; en un móvil de densidad 2,5 el texto de
   11 pt son 27 px y los controles de 4 pt son 10 px, que no se tocan.

### Fase 3 — lo que pidió el dueño

Estos dos son del **dueño de `vectorcraft`/`figcraft`**, no de `storytold`, y no están en el
upstream. Ver la razon de cada uno abajo.

9. **Export a `.xml` drawable** (VectorDrawable).
   *Sin librerías externas*: es XML puro, y `figcraft_svg::try_export_svg` ya calcula el
   `pathData`. Añadir `ExportFormat::VectorDrawable` a `crates/doc/src/props.rs:1229` y un
   `file.exportVectorDrawable`. **4-8 h.**
   *Límite*: solo iconos y vectores. Un drawable no lleva layout, ni blur, ni auto layout; el
   texto va a outlines y los degradados son limitados.

10. **UI a código Compose Kotlin.**
    Ya existe `CodeLang::Compose` en `crates/engine/src/codegen.rs:987` (auto layout → `Column`/
    `Row`, shape → `Box`, instancias → calls), que se usa con `inspect.code {language: compose}`.
    Falta el paso de **exportar el frame entero** con variables a `Color.kt`/`Theme`. **3-6 h.**

11. **Plantilla Android completa en un `.zip`.** Lo que más renta de las tres:
    `MainActivity.kt` + un `.kt` por frame + `res/drawable/*.xml` (del VD) +
    `res/values/colors.xml`/`themes.xml` (de variables) + `AndroidManifest.xml`, empaquetado con
    `zip` (ya es dependencia del workspace). Listo para abrir en Android Studio y compilar.
    **8-15 h** para el scaffold con navegación stub.
    *La lógica* (viewmodels, estados, `onClick` reales) es otra historia: v1 solo
    `onClick { /* TODO */ }`.

Orden recomendado por rentabilidad: **11 → 10 → 9**, porque el 11 usa el 10 y el 9.

### Fase 4 — sincronizar con upstream

12. `storytold/figcraft` está en **v0.6.0 y en alpha**, así que upstream va a mover cosas. El
    `PORT.md` de vectorcraft tiene el mapa de los conflictos que caen siempre
    (`crates/ui-egui/src/canvas.rs`, `crates/ui-egui/src/dialogs/*.rs`, `Cargo.lock`) y la
    lección: **siempre `git merge`, nunca rebase**, y `Cargo.lock` **se regenera con un workflow,
    nunca a mano**.

---

## 6. Cómo reproducir

```bash
# Local, sin móvil (el mismo código corre en escritorio y en el .so)
cargo run -p figcraft-android

# CI
gh workflow run build-android-so.yml    # compila la .so  (~5-12 min)
gh workflow run build-apk.yml           # monta el APK     (~1-2 min)

# Bajar el APK (gh run download NO sirve; va por API)
A=$(gh api "repos/apexmiguel9-hub/figcraft-android/actions/runs/$RUN/artifacts" \
      --jq '.artifacts[]|select(.name=="figcraft-android-debug")|.id')
gh api "repos/apexmiguel9-hub/figcraft-android/actions/artifacts/$A/zip" > a.zip && unzip a.zip

# Instalar y probar en el móvil
adb install -r app-debug.apk
adb shell am start -n ai.storyteller.figcraft/.MainActivity
adb logcat -c && adb shell am start -n ai.storyteller.figcraft/.MainActivity
adb logcat | grep -iE "figcraft_android|fatal|panic"
```

---

## 7. Los tres workflows

| Workflow | Qué hace | Minutos |
|---|---|---|
| `build-android-so` | `cargo ndk` arm64-v8a, quita solo la tabla DWARF, valida `android_main` + `.text` > 5 MB + `.eh_frame` | 5-12 |
| `build-apk` | baja la última `.so` buena, `jniLibs`, `assembleDebug`, valida `.so` dentro + `.symtab` + `MainActivity` + `aapt` | 1-2 |
| `CI` / `packaging-lint` | fmt, clippy, tests, wasm / shellcheck + actionlint | 2-3 |

**Por qué están separados** (regla que costó sangre en vectorcraft): la `.so` son ~400 crates y
tarda 15 min; Gradle tarda 3. `javac` corre en el del APK y su código **no está en la `.so`**, así
que el del `.so` dio verde dos veces seguidas con el Java roto.

**Aviso**: el workflow del APK busca la última `.so` **con éxito**, no la de su commit. Con
artefactos encadenados significa que **el APK puede llevar una `.so` vieja**. Comprobar con
`strings` lo que se acaba de instalar.