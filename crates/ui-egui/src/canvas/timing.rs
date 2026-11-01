//! Instrumentación del pipeline de render del lienzo:
//!   input (dedo) → request_render → render (worker) → upload textura → repaint
//! Cada ciclo se abre con el primer input y se cierra cuando llega un `Done`.
//! Los ciclos cerrados se guardan en un ring buffer y se loggea por logcat
//! cuando el total supera el umbral, para encontrar el cuello de botella.

use std::time::Instant;

/// Un ciclo cerrado del pipeline.
#[derive(Clone, Debug, Default)]
pub struct Cycle {
    pub kind: &'static str, // "gesture", "coarse", "refined"
    /// Momento en que llegó el dedo (input)
    pub input: Option<Instant>,
    /// Momento en que se pidió el render
    pub request: Option<Instant>,
    /// Momento en que el worker terminó el render
    pub done: Option<Instant>,
    /// Momento en que se subió la textura a la GPU
    pub upload: Option<Instant>,
    /// Momento en que egui terminó de pintar el frame
    pub repaint: Option<Instant>,
}

/// Datos del ciclo actualmente abierto (aún no ha llegado su `Done`).
#[derive(Clone, Debug, Default)]
pub struct OpenCycle {
    pub kind: &'static str,
    pub input: Option<Instant>,
    pub request: Option<Instant>,
}

/// Timing del pipeline de render del canvas.
pub struct CanvasTiming {
    /// Ciclos cerrados (ring buffer).
    pub closed: Vec<Cycle>,
    pub closed_index: usize,
    pub closed_capacity: usize,
    /// Ciclo abierto ahora mismo.
    pub open: OpenCycle,
    /// Umbral en ms: por encima se loggea el ciclo por logcat.
    pub log_threshold_ms: f64,
}

impl CanvasTiming {
    pub fn new(capacity: usize) -> Self {
        Self {
            closed: vec![Cycle::default(); capacity],
            closed_index: 0,
            closed_capacity: capacity,
            open: OpenCycle::default(),
            log_threshold_ms: 8.0,
        }
    }

    /// Abre un nuevo ciclo (llama al llegar un dedo o al pedir un render).
    pub fn open_cycle(&mut self, kind: &'static str, input: Option<Instant>, request: Option<Instant>) {
        self.open = OpenCycle { kind, input, request };
    }

    /// Cierra el ciclo abierto con los instantes de done/upload/repaint y lo guarda.
    /// Devuelve el tiempo total ms si se pudo calcular.
    pub fn close_cycle(&mut self, done: Instant, upload: Instant, repaint: Instant) -> Option<f64> {
        let input = self.open.input?;
        let total_ms = repaint.duration_since(input).as_secs_f64() * 1000.0;

        let cycle = Cycle {
            kind: self.open.kind,
            input: self.open.input,
            request: self.open.request,
            done: Some(done),
            upload: Some(upload),
            repaint: Some(repaint),
        };

        if total_ms > self.log_threshold_ms {
            let i2r = cycle.input.zip(cycle.request).map(|(i, r)| r.duration_since(i).as_secs_f64() * 1000.0).unwrap_or(0.0);
            let r2d = cycle.request.zip(cycle.done).map(|(r, d)| d.duration_since(r).as_secs_f64() * 1000.0).unwrap_or(0.0);
            let d2u = cycle.done.zip(cycle.upload).map(|(d, u)| u.duration_since(d).as_secs_f64() * 1000.0).unwrap_or(0.0);
            let u2p = cycle.upload.zip(cycle.repaint).map(|(u, p)| p.duration_since(u).as_secs_f64() * 1000.0).unwrap_or(0.0);
            log::info!(
                "TIMING kind={} input→req={:.1}ms req→done={:.1}ms done→upload={:.1}ms upload→repaint={:.1}ms TOTAL={:.1}ms",
                cycle.kind,
                i2r,
                r2d,
                d2u,
                u2p,
                total_ms
            );
        }

        self.closed[self.closed_index] = cycle;
        self.closed_index = (self.closed_index + 1) % self.closed_capacity;
        self.open = OpenCycle::default();
        Some(total_ms)
    }
}
