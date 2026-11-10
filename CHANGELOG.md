# FigCraft Android — Changelog interno

**Versión:** 0.17.0  
**Tag:** `v0.17.0-android.1`  
**Base:** `storytold/figcraft` v0.17.0 (tag `v0.17.0` es ancestro de `main`)

---

## Commits del port Android en este release (orden cronológico inverso)

| Hash | Mensaje | Área |
|---|---|---|
| `5c3f029e` | feat(android): trash button next to undo/redo in the top bar | UI Android |
| `8c73b558` | fix(canvas): no enviar pointer al motor mientras el menu contextual esta abierto | UI Android / Canvas |
| `48c94971` | fix(canvas): consumir el release despues del long press para no disparar click | UI Android / Canvas |
| `72aae0ef` | fix(canvas): la pulsacion larga sobre una multiseleccion conserva la seleccion | UI Android / Canvas |
| `56f83de1` | feat(android): enable Vulkan on Imagination PowerVR with wgpu SPIR-V debug fix | Vulkan / PowerVR |
| `6c55c51e` | fix(gate): Undo/Redo de la tab bar solo en Android (movian las pestanias en PC) | UI Android |
| `5a2fc437` | fix(gate): un save por cambio de prefs | prefs Android |
| `9a6496de` | fix(gate): theme_startup tests de upstream (system, no dark) | tests |
| `78ba087c` | fix(gate): present_view test como upstream (sin Undo/Redo en PC) | tests |
| `2df2b323` | fix(gate): tests de tabs como upstream (sin Undo/Redo en PC) | tests |
| `8bb41364` | fix(gate): home_tests alineados con upstream (tema system -> light en test) | tests |
| `22ec67ba` | fix(gate): save inmediato de prefs solo en Android (duplicaba el de Your name) | prefs Android |
| `bb025cfe` | fix(ui): menu scroll solo en Android; en PC/test como upstream (roma crop) | UI Android |
| `56f807bb` | fix(ui): scroll del picker solo en Android (horizontal); en PC/test sin scroll | UI Android |
| `262f53dc` | fix(ui): deja caber el picker en pantallas altas (tope 520 rompia el wrap de swatches) | UI Android |
| `950f662f` | android: permiso, insets, browser y gestos tactiles | Android port |
| `3584d716` | feat(canvas): herramienta Add node junto a la tijera | Canvas |
| `542e0cab` | feat(canvas): lupa sobre el dedo al arrastrar vertices y manecillas | Canvas / Lupa |

---

## Detalles técnicos por fix

### PowerVR Vulkan (`56f83de1`)
- **Causa:** en `debug_assertions` (build debug), wgpu activa `InstanceFlags::DEBUG`, naga emite SPIR-V con debug info (`OpSource`, `OpLine`, `OpName`), y el driver PowerVR (vendor `0x1010`) aborta en `FragmentShaderCompileState::CompileUF → BILParseStream`.
- **Fix:** `[patch.crates-io]` apunta wgpu al fork `apexmiguel9-hub/wgpu@5e615d45` que desactiva `WriterFlags::DEBUG` solo para `vendor_id == 0x1010`. `Cargo.lock` coincide (rev `5e615d45c26e80085cb88373669ce059ad16b601`).
- **Efecto:** no afecta Mali, Adreno ni OpenGL; no fuerza SPIR-V 1.4.

### Long press sobre multiselección (`72aae0ef`, `48c94971`, `8c73b558`)
- **Causa:** el toque se aplazaba en `toque_prem`; al soltar, se enviaba Down+Up al engine, interpretado como clic sobre el rect bajo el dedo.
- **Fix estructural:** deferral en `toque_prem`; release sin movimiento = Down+Up (tap rápido); movimiento = Down en punto de presión + Move (arrastre); long press = descarta (menú sobre selección completa). Además `input()` retorna temprano si `app.ui.context_menu.is_some()`.
- **Prueba:** verificado en Moto G56 5G.

### Undo/Redo solo en Android (`6c55c51e`)
- **Causa:** los botones de la tab bar movían las pestañas en desktop y rompían tests de escritorio.
- **Fix:** `cfg!(target_os = "android")` guarda los botones a Android.

### Botón Delete/trash (`5c3f029e`)
- Nuevo icono `Icon::Trash` dibujado en código (MIT OR Apache-2.0, original).
- Botón en `history_buttons` (`tabs.rs`) con comando `edit.delete`, solo Android, deshabilitado si no hay selección.
- Ancho de la zona de botones de la tab bar: 64 → 96 pt.

### Pantalla de licencias (dialogs.rs)
- Nueva pestaña `AboutTab::Licencias` en el diálogo About.
- Muestra LICENSE-MIT, LICENSE-APACHE y ATTRIBUTION.md vía `include_str!`.

### Guarda anti-bucle de cierre (lib.rs del port Android)
- Marcador `.figcraft_crash_marker` en el directorio privado de datos, creado antes de `run_native`, borrado en el primer frame.
- Si existe al arrancar → ejecución previa falló → forzar OpenGL ES y loguearlo.

---

## Versiones de dependencias clave

- wgpu: `30.0.1` (fork `apexmiguel9-hub/wgpu@5e615d45`)
- egui/eframe: `0.37.0` (fork `storytold/egui@6d518fc7`)
- Android NDK: `26.1.10909125`
- target: `aarch64-linux-android` (arm64-v8a)

---

## CI

- `build-android-so`: compila `.so` debug con `cargo ndk`, arm64-v8a, `--locked`.
- `build-apk`: `assembleDebug`, firmado con keystore de debug versionado en el repo (`app/figcraft-debug.keystore`).
- **No hay job de release (firmado, `--release`) en CI.** Ver `RELEASE_NOTES.md` sección pendiente.
