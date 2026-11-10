# FigCraft Android — Notas de release

**Versión:** 0.17.0  
**Tag:** `v0.17.0-android.1`  
**Port:** Android (arm64-v8a), fork no oficial de `storytold/figcraft`

---

## Novedades en este release

- **Menú contextual con long press** sobre el canvas: una pulsación larga sobre una selección múltiple abre el menú contextual y, al soltar el dedo, se conserva toda la selección intacta (no se deselecta todo salvo el objeto bajo el dedo).
- **Botón Delete (basura) en la barra superior de Android:** tercer botón junto a Deshacer/Rehacer. Borra la selección actual sin abrir el menú Editar. Está deshabilitado cuando no hay nada seleccionado. Solo en Android.
- **Botones Deshacer/Rehacer en la barra superior de Android:** siempre al alcance, sin abrir el menú Editar (dos toques). Solo en Android.
- **Vulkan en PowerVR (MediaTek Dimensity):** la app ahora arranca en el Moto G56 5G (PowerVR BXM-8-256, vendor 0x1010) sin abortar al crear el pipeline de egui. Se usa un fork de wgpu que desactiva `WriterFlags::DEBUG` solo para vendor `0x1010`; no afecta a Mali, Adreno ni OpenGL.
- **Pantalla de Licencias en Acerca de:** nueva pestaña "Licencias" que muestra el texto de MIT, Apache-2.0 y las atribuciones (incluidas las fuentes).
- **Guarda anti-bucle de cierre (crash-loop):** si una ejecución previa falla antes del primer frame, el siguiente arranque fuerza OpenGL ES y lo registra.

---

## Probado en

| Dispositivo | SoC | GPU | RAM | Vulkan | Android |
|---|---|---|---|---|---|
| Moto G56 5G | MediaTek MT6855G (Dimensity 7060) | PowerVR B-Series BXM-8-256 | ~8 GB | 1.3.303 | 16 |
| Infinix Hot 10i | Mediatek Helio P65 | Mali-G52 | 4 GB | 1.1 | 11 |

*El Infinix Hot 10i no estaba conectado al preparar este release; sus datos no se pudieron obtener vía adb.*

## No probado

- GPUs Adreno y otras no PowerVR/Mali.
- El Infinix Hot 10i (no conectado).

## Requisitos

- Android 11 o superior (probado en Android 11 y 16).
- arm64-v8a.

## Problemas conocidos

- **"no such node" al copiar, pegar y mover lo pegado:** tras un pegado, al intentar mover el elemento pegado puede aparecer "no such node" y la app tarda unos segundos en permitir el movimiento. Prioridad baja. No está relacionado con el fix del long press.

## Cómo reportar problemas

Usa la plantilla de reporte de errores en `.github/ISSUE_TEMPLATE/bug_report.yml` e incluye: dispositivo, GPU, versión de Android, versión de la app, pasos para reproducir, y la información del sistema (botón "Copiar info del sistema" en Acerca de).

---

# FigCraft Android — Release Notes

**Version:** 0.17.0  
**Tag:** `v0.17.0-android.1`  
**Port:** Android (arm64-v8a), unofficial fork of `storytold/figcraft`

---

## What's new in this release

- **Context menu on long press:** a long press over a multi-selection opens the context menu and, on release, keeps the entire selection intact (it no longer deselects everything except the object under the finger).
- **Delete (trash) button in the Android top bar:** third button next to Undo/Redo. Deletes the current selection without opening the Edit menu. Disabled when nothing is selected. Android only.
- **Undo/Redo buttons in the Android top bar:** always within reach, without opening the Edit menu (two taps away). Android only.
- **Vulkan on PowerVR (MediaTek Dimensity):** the app now starts on the Moto G56 5G (PowerVR BXM-8-256, vendor 0x1010) without aborting when creating the egui pipeline. Uses a wgpu fork that disables `WriterFlags::DEBUG` only for vendor `0x1010`; does not affect Mali, Adreno or OpenGL.
- **Licenses screen in About:** new "Licenses" tab showing MIT, Apache-2.0 and attribution text (including fonts).
- **Crash-loop guard:** if a previous run failed before the first frame, the next start forces OpenGL ES and logs it.

---

## Tested on

| Device | SoC | GPU | RAM | Vulkan | Android |
|---|---|---|---|---|---|
| Moto G56 5G | MediaTek MT6855G (Dimensity 7060) | PowerVR B-Series BXM-8-256 | ~8 GB | 1.3.303 | 16 |
| Infinix Hot 10i | Mediatek Helio P65 | Mali-G52 | 4 GB | 1.1 | 11 |

*The Infinix Hot 10i was not connected when preparing this release; its data could not be obtained via adb.*

## Not tested

- Adreno and other non-PowerVR/Mali GPUs.
- Infinix Hot 10i (not connected).

## Requirements

- Android 11 or higher (tested on Android 11 and 16).
- arm64-v8a.

## Known issues

- **"no such node" when copying, pasting and moving the paste:** after pasting, attempting to move the pasted element may show "no such node" and the app may take a few seconds to allow movement. Low priority. Not related to the long press fix.

## How to report issues

Use the bug report template in `.github/ISSUE_TEMPLATE/bug_report.yml` and include: device, GPU, Android version, app version, steps to reproduce, and system information (via the "Copy system info" button in About).
