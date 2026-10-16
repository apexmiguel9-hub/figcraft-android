# FigCraft en Android — mapa del port (v1 esqueleto)

Plantilla replicada de `vectorcraft-android` (`apps/vectorcraft-android/` + workflows).
Mismo stack: `eframe 0.36` + `egui 0.36` + `winit 0.30` + `wgpu`, render CPU
(`tiny-skia` en figcraft, `vello_cpu` en vectorcraft). La GPU solo presenta,
por eso se fuerza **solo OpenGL ES** y se esperan los mismos 120 fps.

## Lo que trae este esqueleto

| Fichero | Qué es |
|---|---|
| `apps/figcraft-android/Cargo.toml` | lib `cdylib` `figcraft_android` + bin escritorio |
| `apps/figcraft-android/src/lib.rs` | `android_main`, GLES solo, recorte `screen_rect`, logcat, hook panics |
| `apps/figcraft-android/src/main.rs` | mismo `run()` en escritorio para depurar sin movil |
| `apps/figcraft-android/android/` | proyecto Gradle, `MainActivity extends NativeActivity`, permiso + insets |
| `.github/workflows/build-android-so.yml` | compila `.so` `arm64-v8a` con `cargo ndk`, comprueba `android_main` + `.text` |
| `.github/workflows/build-apk.yml` | monta APK desde la ultima `.so` buena, comprueba `.so` + dex + aapt |

## Lo que falta (siguiente paso, copiado de vectorcraft)

1. `src/permiso.rs`: JNI `MANAGE_EXTERNAL_STORAGE` + `insets()` via `AndroidApp::vm_as_ptr()`
   + enchufar en `raw_input_hook` (hoy `insets()` devuelve 0).
2. `src/browser.rs`: browser `egui_file 0.28` + `Services` con `std::fs`
   (hoy `NoServices`; sin esto Open/Save no abre nada en el movil).
3. Gestos tactiles primero en `crates/ui-egui/src/canvas.rs` (hoy mouse-first).
4. Probar en movil: `gh workflow run build-android-so.yml`, luego `build-apk.yml`.
   Bajar APK por API (ver `vectorcraft-android/PORT.md §12`), `gh run download` no sirve.

## Reproducir

```bash
cargo run -p figcraft-android   # escritorio, sin movil
gh workflow run build-android-so.yml
gh workflow run build-apk.yml
```
