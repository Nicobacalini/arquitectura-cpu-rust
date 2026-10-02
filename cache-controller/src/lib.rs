mod bus;
mod hierarchy;
mod policy;

/// Simulador de jerarquia de memoria con RAM de 4096 bytes intermediada por
/// dos niveles de cache asociativa por conjuntos bajo politicas Write-Back y LRU.
/// Incluye L1 de 4 conjuntos por 2 vias y L2 de 8 conjuntos por 2 vias.
/// Estructurado internamente en modulos para almacenamiento, politicas de desalojo,
/// operaciones de bus y jerarquia multinivel.
pub mod storage;

/// Modulo de memoria virtual: TLB, Page Table, ASID y MMU (Fase 2).
pub mod paginacion;

pub use storage::{
    BLOQUE_BYTES, CANTIDAD_CONJUNTOS, CANTIDAD_CONJUNTOS_L2, ConjuntoCache, ControladorMemoria,
    EstadisticasCache, LineaCache, NivelL2, TAMANO_RAM,
};

pub use hierarchy::JerarquiaCache;

pub use paginacion::{
    EntradaPagina, EntradaTlb, Mmu, ResultadoTraduccion, TablaDePaginas, TipoAcceso, Tlb,
    calcular_amat, descomponer_direccion_virtual, reconstruir_direccion_fisica,
    traducir_direccion, MARCOS_FISICOS, PAGINAS_VIRTUALES, TAMANO_PAGINA,
};

impl Default for ControladorMemoria {
    fn default() -> Self {
        Self::nuevo()
    }
}

#[cfg(test)]
mod tests;
