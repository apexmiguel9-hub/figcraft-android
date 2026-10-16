//! Binario de escritorio para el crate del port.
//!
//! El mismo codigo corre en el `.so` de Android (ver `lib.rs`) y en este
//! binario. Asi la logica se depura en local sin compilar para el movil.
fn main() -> eframe::Result {
    figcraft_android::run()
}
