mod bus;
mod policy;
/// # cache-controller
///
/// Simulador de jerarquia de memoria: RAM de 256 bytes intermediada por una cache
/// asociativa por conjuntos de 4 conjuntos x 2 vias (32 bytes efectivos),
/// con politica Write-Back / Write-Allocate y reemplazo LRU.
///
/// ## Modulos internos
/// - [storage]: tipos de datos y funciones de decodificacion de direcciones
/// - [policy]: politica LRU (`elegir_via_victima`) y manejo de miss (`manejar_miss`)
/// - [bus]: API publica de acceso (`leer_byte`, `escribir_byte`, `flush`)
pub mod storage;

// Re-exports publicos para que los consumidores del crate importen desde la raiz
pub use storage::{
    BLOQUE_BYTES, CANTIDAD_CONJUNTOS, ConjuntoCache, ControladorMemoria, EstadisticasCache,
    LineaCache, TAMANO_RAM,
};

impl Default for ControladorMemoria {
    fn default() -> Self {
        Self::nuevo()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests;
