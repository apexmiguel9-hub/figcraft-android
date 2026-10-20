//! Knobs de ejecucion: ajustes que se leen de un fichero de texto y se recargan en caliente, sin
//! recompilar ni reinstalar la app.
//!
//! MEDIDO que hacia falta: cada A/B de rendimiento costaba una build de CI (~8 min de `.so`), otra
//! del APK, descargar los artefactos e instalar. Con esto el mismo A/B son dos segundos en el movil:
//! se cambia el fichero y la app lo coge en el frame siguiente. Los valores se escriben en el log
//! `PERF` de cada 3 s, asi que cada medida queda autoexplicada.
//!
//! Formato: una clave por linea, `clave = valor`, `#` empieza un comentario. Las claves que no se
//! conocen se ignoran (no se pierde el resto del fichero por una errata).
//!
//! Claves y valores por defecto de compilacion (lo que se usa sin fichero):
//!
//! | clave | defecto | que cambia |
//! |---|---|---|
//! | `overscan` | `16` | pixeles de margen de la textura del lienzo (0 = sin margen) |
//! | `preview_after_ms` | auto (12) | a partir de que ms de render se muestra un preview antes |
//! | `preview_factor` | auto (4) | cuantas veces menos pixeles tiene ese preview |
//! | `glass` | `on` | efectos de glass: `off` los salta enteros (A/B) |
//! | `glass_log` | `off` | vuelca cada glass: tamano, margen, ms y alfa media |
//! | `glass_max_px` | `0` | si el layer del glass pasa de estos px, se salta (0 = sin limite) |
//! | `glass_scale` | `1` (2 en Android) | divide la resolucion del glass (2 = mitad por eje, 4x menos px) |
//!
//! Donde se busca, en orden (el primero que exista manda):
//!
//! 1. `$FIGCRAFT_KNOBS`
//! 2. `/storage/emulated/0/Android/data/<paquete>/files/knobs.conf` (facil de `adb push`)
//! 3. `/data/user/0/<paquete>/files/knobs.conf` (por `adb shell run-as <paquete>`)
//! 4. `/data/data/<paquete>/files/knobs.conf`
//! 5. `./knobs.conf`
//! 6. `$HOME/.config/figcraft/knobs.conf`
//!
//! El paquete se saca de `/proc/self/cmdline`, que en Android es el nombre del paquete.

use std::path::PathBuf;

/// Cada cuanto se mira si el fichero cambio (stat de un fichero: barato, y solo desde
/// `knobs()`, que los llamantes usan una vez por render, no por pixel).
#[cfg(not(target_arch = "wasm32"))]
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct Knobs {
    /// Pixeles de margen que se anaden a la textura del lienzo (0 = ninguno).
    pub overscan: u32,
    /// Sustituye a `PREVIEW_AFTER_MS` del canvas (`None` = el de compilacion).
    pub preview_after_ms: Option<f64>,
    /// Sustituye a `PREVIEW_FACTOR` (`None` = el de compilacion).
    pub preview_factor: Option<f64>,
    /// Efectos de glass encendidos.
    pub glass: bool,
    /// Vuelca cada efecto de glass al log.
    pub glass_log: bool,
    /// Salta los glass cuyo layer pase de estos pixeles (0 = sin limite).
    pub glass_max_px: u32,
    /// Divide por este factor la resolucion a la que se calcula el glass (1 = completa, 2 = la
    /// mitad por eje, 4x menos pixeles). La lente es suave y se sube con bilineal.
    pub glass_scale: u32,
    /// Fichero del que salieron los valores (None: los de compilacion).
    pub file: Option<PathBuf>,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            overscan: 16,
            preview_after_ms: None,
            preview_factor: None,
            glass: true,
            glass_log: false,
            glass_max_px: 0,
            // MEDIDO: en el movil el glass a resolucion completa costaba 163 ms por llamada (4-6
            // llamadas por render). A la mitad por eje son 40 ms y a simple vista no se distingue:
            // el borde de la lente mide ~49 px, asi que 24 px siguen siendo de sobra.
            glass_scale: if cfg!(target_os = "android") { 2 } else { 1 },
            file: None,
        }
    }
}

/// Los valores que hay que usar ahora. Barato (un `Arc`), pero **no** para llamar por pixel.
pub fn knobs() -> std::sync::Arc<Knobs> {
    #[cfg(target_arch = "wasm32")]
    let out = {
        static DEFAULTS: std::sync::OnceLock<std::sync::Arc<Knobs>> = std::sync::OnceLock::new();
        DEFAULTS.get_or_init(|| std::sync::Arc::new(Knobs::default())).clone()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let out = {
        use std::sync::{Arc, Mutex, OnceLock};
        static STATE: OnceLock<Mutex<State>> = OnceLock::new();
        let state = STATE.get_or_init(|| Mutex::new(State::new()));
        match state.lock() {
            Ok(mut s) => {
                s.refresh();
                s.current.clone()
            }
            Err(_) => Arc::new(Knobs::default()),
        }
    };
    out
}

/// Una linea con los valores activos, para el log de rendimiento.
pub fn summary() -> String {
    let k = knobs();
    let after = k.preview_after_ms.map(|v| format!("{v}")).unwrap_or_else(|| "auto".to_string());
    let factor = k.preview_factor.map(|v| format!("{v}")).unwrap_or_else(|| "auto".to_string());
    format!(
        "overscan={} previewAfter={after} previewFactor={factor} glass={} glassMaxPx={} glassScale={} knobs={}",
        k.overscan,
        if k.glass { "on" } else { "off" },
        k.glass_max_px,
        k.glass_scale,
        k.file.as_ref().map_or_else(|| "ninguno".to_string(), |p| p.display().to_string())
    )
}

#[cfg(not(target_arch = "wasm32"))]
struct State {
    current: std::sync::Arc<Knobs>,
    path: Option<PathBuf>,
    stamp: Option<std::time::SystemTime>,
    checked: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl State {
    fn new() -> Self {
        let loaded = load();
        Self { current: std::sync::Arc::new(loaded.0), path: loaded.1, stamp: loaded.2, checked: std::time::Instant::now() }
    }

    /// Recarga si toca: cada `CHECK_EVERY`, y solo si el fichero cambio de fecha.
    fn refresh(&mut self) {
        if self.checked.elapsed() < CHECK_EVERY {
            return;
        }
        self.checked = std::time::Instant::now();
        if self.path.is_none() {
            // Quiza el fichero se creo despues de arrancar.
            if let Some(path) = find() {
                self.path = Some(path);
                self.stamp = None;
            }
        }
        let Some(path) = self.path.as_ref() else { return };
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if stamp == self.stamp {
            return;
        }
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut k = parse(&text);
                k.file = Some(path.clone());
                log::info!("KNOBS {}: {}", path.display(), summary_of(&k));
                self.current = std::sync::Arc::new(k);
                self.stamp = stamp;
            }
            Err(e) => {
                log::warn!("KNOBS no se pudo leer {}: {e}", path.display());
                // No se reintenta en bucle: se queda con lo que tenia.
                self.stamp = stamp;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load() -> (Knobs, Option<PathBuf>, Option<std::time::SystemTime>) {
    let Some(path) = find() else { return (Knobs::default(), None, None) };
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let mut k = parse(&text);
            k.file = Some(path.clone());
            log::info!("KNOBS {}: {}", path.display(), summary_of(&k));
            (k, Some(path), stamp)
        }
        Err(e) => {
            log::warn!("KNOBS no se pudo leer {}: {e}", path.display());
            (Knobs::default(), None, None)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn find() -> Option<PathBuf> {
    let mut list: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("FIGCRAFT_KNOBS") {
        list.push(PathBuf::from(p));
    }
    if let Some(pkg) = package_name() {
        list.push(PathBuf::from(format!("/storage/emulated/0/Android/data/{pkg}/files/knobs.conf")));
        list.push(PathBuf::from(format!("/data/user/0/{pkg}/files/knobs.conf")));
        list.push(PathBuf::from(format!("/data/data/{pkg}/files/knobs.conf")));
    }
    list.push(PathBuf::from("knobs.conf"));
    if let Some(home) = std::env::var_os("HOME") {
        list.push(PathBuf::from(home).join(".config/figcraft/knobs.conf"));
    }
    list.into_iter().find(|p| p.is_file())
}

/// El paquete de la app en Android: `/proc/self/cmdline` es su nombre. En escritorio es la ruta del
/// binario (con `/`), que no sirve como paquete.
#[cfg(not(target_arch = "wasm32"))]
fn package_name() -> Option<String> {
    let raw = std::fs::read("/proc/self/cmdline").ok()?;
    let first = raw.split(|b| *b == 0).next()?;
    let name = std::str::from_utf8(first).ok()?.trim();
    if name.is_empty() || name.contains('/') || !name.contains('.') {
        return None;
    }
    Some(name.to_string())
}

fn parse(text: &str) -> Knobs {
    let mut k = Knobs::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "overscan" => k.overscan = value.parse().unwrap_or(k.overscan),
            "preview_after_ms" => {
                if let Ok(v) = value.parse::<f64>() {
                    k.preview_after_ms = Some(v.max(0.0));
                }
            }
            "preview_factor" => {
                if let Ok(v) = value.parse::<f64>()
                    && v >= 1.0
                {
                    k.preview_factor = Some(v);
                }
            }
            "glass" => k.glass = truthy(value),
            "glass_log" => k.glass_log = truthy(value),
            "glass_max_px" => k.glass_max_px = value.parse().unwrap_or(k.glass_max_px),
            "glass_scale" => k.glass_scale = value.parse::<u32>().unwrap_or(k.glass_scale).clamp(1, 8),
            _ => {}
        }
    }
    k
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "si" | "sí" | "on" | "true" | "yes")
}

#[cfg(not(target_arch = "wasm32"))]
fn summary_of(k: &Knobs) -> String {
    format!(
        "overscan={} previewAfter={} previewFactor={} glass={} glassLog={} glassMaxPx={} glassScale={}",
        k.overscan,
        k.preview_after_ms.map(|v| format!("{v}")).unwrap_or_else(|| "auto".to_string()),
        k.preview_factor.map(|v| format!("{v}")).unwrap_or_else(|| "auto".to_string()),
        if k.glass { "on" } else { "off" },
        k.glass_log,
        k.glass_max_px,
        k.glass_scale
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_compiled_ones() {
        let k = Knobs::default();
        assert_eq!(k.overscan, 16);
        assert!(k.glass);
        assert_eq!(k.preview_after_ms, None);
        assert_eq!(k.preview_factor, None);
        assert_eq!(k.glass_scale, if cfg!(target_os = "android") { 2 } else { 1 });
    }

    #[test]
    fn it_parses_keys_values_and_ignores_comments_and_typos() {
        let k = parse("# prueba\noverscan = 0\npreview_after_ms=250\nglass = off\nbasura\nno_existe=1\nglass_max_px = 400000\nglass_scale = 2\n");
        assert_eq!(k.overscan, 0);
        assert_eq!(k.preview_after_ms, Some(250.0));
        assert!(!k.glass);
        assert_eq!(k.glass_max_px, 400_000);
        assert_eq!(k.glass_scale, 2);
    }

    #[test]
    fn a_glass_scale_below_one_or_absurd_is_clamped() {
        assert_eq!(parse("glass_scale=0").glass_scale, 1);
        assert_eq!(parse("glass_scale=99").glass_scale, 8);
    }

    #[test]
    fn a_preview_factor_below_one_is_ignored() {
        let k = parse("preview_factor=0.5");
        assert_eq!(k.preview_factor, None);
    }

    #[test]
    fn truthy_accepts_the_ways_one_writes_it() {
        for v in ["on", "ON", "si", "sí", "true", "yes", "1"] {
            assert!(truthy(v), "{v}");
        }
        for v in ["off", "0", "no", "", "false"] {
            assert!(!truthy(v), "{v}");
        }
    }
}
