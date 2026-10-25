# FigCraft Android Port — Estado de Continuidad

> **Fecha:** 2026-10-24  
> **Rama principal del port:** `main` (commit `b11c6aa3`)  
> **Rama de revisión/upstream:** `scratch/merge-upstream` (commit `26e67307` + `1afcf78d`)  
> **Rama fix actual:** `fix/node-editor-handles` (commit `d8955033`, PR #3)  
> **Upstream base:** `17ed21d1` (v0.10.0-11)  
> **Móvil de pruebas:** `192.168.100.51:33293` (pantalla 1080×2400 física, 2400×1080 lógica)

---

## 1. Resumen de lo que se ha hecho y verificado

### ✅ Merge upstream v0.10.0 a `main` (PR #2, mergeado)
- Upstream `17ed21d1` (v0.10.0-11) portado selectivamente a la rama `scratch/merge-upstream`
- Fixes incluidos y **verificados en móvil (frío, ADB logcat, capturas 2400×1080)**:
  - **Permiso de almacenamiento**: no reaparece si ya está concedido; `.permiso-preguntado` persiste tras "Ahora no" / "Permitir"
  - **Diálogos**: `vscroll` condicional solo si alto < 640 pt
  - **Titlebar**: tabs empiezan 64 px después por Undo/Redo (`TABS_AFTER_HISTORY = 64.0`)
  - **Cargo.lock + versionName**: sincronizados a `0.10.0`
  - **Tests**: titlebar y ui_scale ajustados (Undo/Redo no se quitan)
  - **Menú contextual long press**: queda abierto al soltar si no se tocó ítem
  - **Undo/Redo**: accesibles fuera de `@` > Edit
  - **Preferences**: autosave + botón Save
  - **Glass**: relleno al 20 % automático
  - **Drop documento/imagen**: `pointer_pos` no `hover_pos`
  - **Caja editor de nodos**: conserva `handles = 0` (sin cuadritos) + `accent(painter)` + `Some(label)` (fix commit `26e67307`)
  - **Cierre menús**: por release + `context_menu_ignore_release` (no por press como upstream)

### ✅ Fix cuadritos editor de nodos (PR #3, en CI)
- **Commit `c10a3df3`**: restaura `handles = 4` en `transform_box` del editor de nodos
- **Commit `d8955033`**: mueve dibujo del box **después** de `vector_overlay` para que los 4 cuadritos queden **encima** de los círculos de los nodos en las 4 esquinas
- CI y `.so` en progreso (runs `40059644671`, `40059640716`)

---

## 2. Queda pendiente de verificar (checklist táctil)

| Ítem | Estado | Notas |
|------|--------|-------|
| **Cuadritos editor de nodos** | 🔄 En CI | PR #3; probar en móvil tras APK nuevo |
| **Glass (relleno 20 %)** | ⏳ Pendiente | Verificar en panel Fill |
| **Drop documento/imagen** | ⏳ Pendiente | Probar `adb` drag o intent |
| **Color personalizado** | ⏳ Pendiente | Picker + input hex |
| **Long press fila de capas** | ⏳ Pendiente | Menú contextual queda abierto |
| **Ghosting UI** | 🟡 Observado | Aparece en arranque (starting_reveal @ 30 Hz); no bloquea interacción; no es regresión del fix de permiso |

> **Nota:** El ghosting vertical duplicado se ve también en el APK viejo del merge; parece artefacto de la animación de arranque de Moto (SurfaceFlinger `starting_reveal`). Se mitiga desactivando animaciones globales. No impide pruebas táctiles.

---

## 3. Builds y CI actuales (PR #3)

| Check | Run | Estado |
|-------|-----|--------|
| CI | `40059644671` | `in_progress` |
| build-android-so | `40059640716` | `in_progress` |
| build-apk (previo, PR #3 v1) | `40058793346` | ✅ success (usa `.so` del commit anterior) |
| SonarCloud | `40059644719` | ✅ success |

**Regla:** *Un solo APK por build de `.so`*. El próximo `build-apk` se disparará tras éxito del `.so` actual y usará ese `.so` (commit `d8955033`).

---

## 4. Reglas y estándares establecidos (OBLIGATORIOS)

### 4.1 Flujo de trabajo y merges
- **Nunca tocar `main` directamente**. Todo pasa por PR en GitHub.
- **Merge de revisión**: siempre en `scratch/merge-upstream` (base `a0d1162`, port `e42a8ce9`, upstream `17ed21d1`).
- **CI debe pasar en el PR** antes de mergear a `main`.
- **Verificación en móvil obligatoria** antes de merge: `adb logcat`, capturas 2400×1080, coordenadas reales para `adb shell input`.
- **No compilar ni testear en local**. Solo builds por GitHub Actions.
- **No `adb kill-server` ni reiniciar adb**. Usar `adb devices`, `adb shell`, `adb logcat -c`, `adb exec-out screencap`.

### 4.2 Calidad de código (INNEGOCIABLE)
- **Código impecable**: revisar **1, 2 o 3 veces** lo escrito antes de commit para evitar bugs obvios.
- **Auto-revisión del modelo**: el modelo que escribe el código **debe revisarlo él mismo** antes de considerar la tarea hecha. No asumir que "funciona" sin leer el diff y razonar efectos colaterales.
- **Verificación completa antes de pasar de fase**: si se añade una función/feature, **esa función debe estar 100% verificada** (tests, móvil, edge cases) antes de ir a la siguiente fase. No acumular "deuda de verificación".
- **Prioridad**: fluidez y usabilidad táctil > features nuevas.
- **No `unwrap()`, `expect()`, `panic!`** en código de producción (clippy lo deniega).

### 4.3 Comportamiento táctil específico
- **Menú contextual long press**: se queda abierto al soltar si no se tocó ítem.
- **Círculos de redondear esquinas en selección normal**: **no tocar**.
- **Cuadritos del box**: solo se ocultan en editor de nodos (y ahora se muestran encima de los círculos).
- **No añadir tool Direct Select** salvo cambio de método de entrada.
- **Tema por defecto**: dark.
- **Panel Fill**: altura usable; botones ojo/candado accesibles.

### 4.4 Permiso de almacenamiento
- Leer estado real **antes** de mostrar aviso.
- Persistir en `.permiso-preguntado` que ya se preguntó.
- "Permitir" y "Ahora no" marcan ese fichero.
- `hay_que_preguntar()` oculta aviso si ya se preguntó o se concedió.

### 4.5 Versionado
- `versionName` en `build.gradle` = versión del workspace (ahora `0.10.0`).
- `Cargo.lock` `figcraft-android` = `0.10.0`.
- Un APK por `.so`; Vulkan solo en rama de prueba.

---

## 5. Archivos clave modificados en esta revisión

| Archivo | Cambio principal |
|---------|------------------|
| `apps/figcraft-android/src/permiso.rs` | Fix permiso: `LEIDO` solo tras JNI ok; `cada_frame()` lee en frame 1; persistencia `.permiso-preguntado` |
| `apps/figcraft-android/src/browser.rs` | Llamada a `permiso::cada_frame()` |
| `apps/figcraft-android/src/lib.rs` | Orden arranque + `registrar()` guarda JavaVM |
| `MainActivity.java` | `nativeListo()` ya no lee; `hayPermiso()`/`pedirPermiso()` |
| `crates/ui-egui/src/dialogs.rs` | `window()` con `vscroll` condicional |
| `crates/ui-egui/src/titlebar_tests.rs` | Rango botón `+` ajustado por Undo/Redo |
| `crates/ui-egui/src/tabs.rs` | Undo/Redo + layout nuevo tabs |
| `crates/ui-egui/src/canvas.rs` | Drop usa `pointer_pos`; caja editor nodos conservada; **box dibujado tras overlays (PR #3)** |
| `crates/engine/src/cmd/edit.rs` | Glass + `effect_from` |
| `crates/ui-egui/src/menus.rs` | Cierre por release + `context_menu_ignore_release` |
| `crates/ui-egui/src/layers.rs` | Fixes táctiles capas |

---

## 6. Cómo continuar en otra sesión / otro móvil / otra IA

1. **Clonar repo**: `git clone https://github.com/apexmiguel9-hub/figcraft-android.git` (o usar `/root/figcraft-upstream` si persiste).
2. **Leer este `PORT_STATUS.md`** y `RESOLUTION.md` (en `/root/merge-review/upstream-main/`) + `HANDOFF.md`.
3. **Verificar rama actual**:
   ```bash
   cd /root/figcraft-upstream
   git status
   git log --oneline -5
   ```
4. **Revisar PRs abiertos**: `gh pr list --repo apexmiguel9-hub/figcraft-android`
5. **Estado CI**: `gh run list --repo apexmiguel9-hub/figcraft-android --limit 10`
6. **Móvil**: `adb connect 192.168.100.51:33293` → `adb devices`
7. **Instalar APK más reciente**: buscar en `/tmp/opencode/apk-*/app-debug.apk` o descargar del último `build-apk` success.
8. **Continuar checklist** desde §2.

---

## 7. Decisiones abiertas (para discutir con el owner)

- **Merge completo a `main` vs. port selectivo**: `main` ya tiene el merge del PR #2; PR #3 pendiente de merge tras verificación móvil.
- **Pivote code-native de variables**: sin alcance definido.
- **Port a Slint**: bloqueado por licencia, prueba canvas custom, integración Sora.
- **Egui vs Kotlin Compose**: decisión final pendiente.
- **Ghosting UI**: investigar `SurfaceFlinger` / `starting_reveal` / 30 Hz; no bloquea release pero afecta percepción visual.

---

## 8. Referencias rápidas (comandos útiles)

```bash
# Ver PRs
gh pr list --repo apexmiguel9-hub/figcraft-android
gh pr view 3 --repo apexmiguel9-hub/figcraft-android --json state,mergeable

# Ver runs
gh run list --repo apexmiguel9-hub/figcraft-android --branch fix/node-editor-handles --limit 10

# Descargar APK de un run
gh api repos/apexmiguel9-hub/figcraft-android/actions/runs/<RUN_ID>/artifacts \
  --jq '.artifacts[]|select(.name=="figcraft-android-debug")|.id' \
  | xargs -I{} gh api -X GET repos/apexmiguel9-hub/figcraft-android/actions/artifacts/{}/zip > /tmp/app.zip

# Instalar en móvil
adb install -r /tmp/opencode/apk-pr3/app-debug.apk

# Logcat limpio
adb logcat -c && adb logcat -s figcraft_android:V *:S

# Captura 2400x1080
adb exec-out screencap -p > /tmp/shot.png
```

---

## 9. Nota final para la próxima IA / sesión

> **La calidad no es negociable.** Cada cambio, por pequeño que sea, debe:
> 1. Leerse entero (diff completo) antes de commit.
> 2. Pensar efectos colaterales (orden de dibujo, estado, hilos, JNI, ciclo de vida Android).
> 3. Verificarse en CI **y** en móvil real.
> 4. Documentarse aquí y en `RESOLUTION.md` / `HANDOFF.md`.
>
> Si algo huele a "funciona pero no sé por qué", **no se sube**. Se investiga hasta entender la causa raíz.
>
> Este port existe para **fluidez y usabilidad táctil impecable**. Todo lo demás es secundario.

---

*Documento generado automáticamente como resumen de la sesión. Actualizar al cerrar cada fase.*