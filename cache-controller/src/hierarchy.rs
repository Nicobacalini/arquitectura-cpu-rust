// JerarquiaCache: orquestador de dos niveles L1, L2 y RAM.
//
// Flujo de lectura: se consulta primero L1. Si acierta devuelve el byte.
// Ante un fallo en L1 se consulta L2, trayendo el bloque a L1 con posible
// desalojo dirty hacia L2. Si tambien falla L2, el bloque se recupera
// desde RAM cargandolo en L2 y luego en L1 para retornar el byte requerido.
//
// Flujo de escritura: aplica Write-Back y Write-Allocate en ambos niveles.
// La localizacion del bloque sigue el camino de lectura y el byte se escribe
// en L1 marcando dirty_bit en true.
//
// Write-back en cadena: el desalojo dirty de L1 se vuelca a L2 y el desalojo
// dirty de L2 se vuelca a RAM.

use crate::storage::{
    BLOQUE_BYTES, CANTIDAD_CONJUNTOS, CANTIDAD_CONJUNTOS_L2, ControladorMemoria, NivelL2,
    TAMANO_RAM,
};

/// Jerarquia de memoria de dos niveles.
///
/// Contiene el controlador L1 con 4 conjuntos asociativos de 2 vias,
/// el nivel L2 con 8 conjuntos asociativos de 2 vias y la memoria principal
/// RAM de 4096 bytes como fuente de verdad compartida.
pub struct JerarquiaCache {
    /// Cache L1 de primer nivel de consulta.
    pub l1: ControladorMemoria,
    /// Cache L2 de segundo nivel de consulta.
    pub l2: NivelL2,
    /// RAM principal utilizada cuando ambos niveles fallan.
    pub ram: [u8; TAMANO_RAM],
}

impl JerarquiaCache {
    /// Crea una jerarquia con L1, L2 y RAM vacias e inicializadas en cero.
    pub fn nuevo() -> Self {
        Self {
            l1: ControladorMemoria::nuevo(),
            l2: NivelL2::nuevo(),
            ram: [0; TAMANO_RAM],
        }
    }

    /// Alias idiomatico de Rust para [`JerarquiaCache::nuevo`].
    #[inline]
    pub fn new() -> Self {
        Self::nuevo()
    }

    /// Devuelve el indice de via dentro de L1 donde quedo instalado el bloque,
    /// aplicando LRU y write-back en cadena si la victima era dirty.
    /// El write-back de L1 se vuelca a L2 y no a RAM directamente.
    fn instalar_en_l1(&mut self, tag: u16, indice: usize) -> usize {
        let via_victima = Self::elegir_victima_l1(&self.l1, indice);

        {
            let linea = &self.l1.cache[indice].vias[via_victima];
            if linea.valido && linea.dirty_bit {
                let dir_base = self.l1.reconstruir_direccion_base(linea.tag, indice) as u16;
                let datos = linea.datos;
                Self::escribir_bloque_en_l2(&mut self.l2, &mut self.ram, dir_base, &datos);
                self.l1.estadisticas.desalojos_dirty += 1;
            }
        }

        let dir_base_nueva = self.l1.reconstruir_direccion_base(tag, indice);
        let bloque = Self::obtener_bloque_de_l2(&mut self.l2, &mut self.ram, dir_base_nueva);

        let linea = &mut self.l1.cache[indice].vias[via_victima];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = false;
        linea.datos = bloque;
        linea.ultimo_acceso = self.l1.contador_ciclos;

        via_victima
    }

    /// Aplica LRU sobre L1 para el conjunto dado con la misma logica de policy.
    fn elegir_victima_l1(l1: &ControladorMemoria, indice: usize) -> usize {
        let v0 = &l1.cache[indice].vias[0];
        let v1 = &l1.cache[indice].vias[1];
        if !v0.valido {
            return 0;
        }
        if !v1.valido {
            return 1;
        }
        if v0.ultimo_acceso < v1.ultimo_acceso {
            0
        } else {
            1
        }
    }

    /// Devuelve el bloque de 4 bytes correspondiente a `dir_base` desde L2,
    /// cargandolo desde `ram` si L2 tambien falla.
    fn obtener_bloque_de_l2(
        l2: &mut NivelL2,
        ram: &mut [u8; TAMANO_RAM],
        dir_base: u16,
    ) -> [u8; BLOQUE_BYTES] {
        let (tag, indice, _) = l2.decodificar_direccion(dir_base);

        if let Some(via_idx) = l2.buscar_via_hit_l2(indice, tag) {
            l2.cache[indice].vias[via_idx].ultimo_acceso = l2.contador_ciclos;
            l2.estadisticas.hits += 1;
            return l2.cache[indice].vias[via_idx].datos;
        }

        l2.estadisticas.misses += 1;
        let addr = dir_base as usize;
        let mut bloque = [0u8; BLOQUE_BYTES];
        bloque.copy_from_slice(&ram[addr..addr + BLOQUE_BYTES]);

        let via_victima = Self::elegir_victima_l2(l2, indice);
        {
            let linea = &l2.cache[indice].vias[via_victima];
            if linea.valido && linea.dirty_bit {
                let old_base = l2.reconstruir_direccion_base_l2_pub(linea.tag, indice) as usize;
                let old_datos = linea.datos;
                ram[old_base..old_base + BLOQUE_BYTES].copy_from_slice(&old_datos);
                l2.estadisticas.desalojos_dirty += 1;
            }
        }
        let linea = &mut l2.cache[indice].vias[via_victima];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = false;
        linea.datos = bloque;
        linea.ultimo_acceso = l2.contador_ciclos;

        bloque
    }

    /// Escribe un bloque completo de 4 bytes en L2 aplicando Write-Allocate.
    fn escribir_bloque_en_l2(
        l2: &mut NivelL2,
        ram: &mut [u8; TAMANO_RAM],
        dir_base: u16,
        datos: &[u8; BLOQUE_BYTES],
    ) {
        let (tag, indice, _) = l2.decodificar_direccion(dir_base);

        let via_idx = if let Some(v) = l2.buscar_via_hit_l2(indice, tag) {
            v
        } else {
            let via_victima = Self::elegir_victima_l2(l2, indice);
            {
                let linea = &l2.cache[indice].vias[via_victima];
                if linea.valido && linea.dirty_bit {
                    let old_base = l2.reconstruir_direccion_base_l2_pub(linea.tag, indice) as usize;
                    let old_datos = linea.datos;
                    ram[old_base..old_base + BLOQUE_BYTES].copy_from_slice(&old_datos);
                    l2.estadisticas.desalojos_dirty += 1;
                }
            }
            via_victima
        };

        let linea = &mut l2.cache[indice].vias[via_idx];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = true;
        linea.datos = *datos;
        linea.ultimo_acceso = l2.contador_ciclos;
    }

    /// Aplica LRU sobre L2 para el conjunto dado.
    fn elegir_victima_l2(l2: &NivelL2, indice: usize) -> usize {
        let v0 = &l2.cache[indice].vias[0];
        let v1 = &l2.cache[indice].vias[1];
        if !v0.valido {
            return 0;
        }
        if !v1.valido {
            return 1;
        }
        if v0.ultimo_acceso < v1.ultimo_acceso {
            0
        } else {
            1
        }
    }

    /// Lee un byte de la direccion indicada a traves de la jerarquia L1, L2 y RAM.
    /// Un acierto en L1 devuelve el dato actualizando el LRU. Un fallo en L1 busca
    /// el bloque en L2, y ante un fallo en L2 se recurre a la memoria RAM.
    pub fn leer_byte(&mut self, direccion: u16) -> u8 {
        self.l1.contador_ciclos += 1;

        let (tag, indice, offset) = self.l1.decodificar_direccion(direccion);

        if let Some(via_idx) = self.l1.buscar_via_hit(indice, tag) {
            self.l1.cache[indice].vias[via_idx].ultimo_acceso = self.l1.contador_ciclos;
            self.l1.estadisticas.hits += 1;
            return self.l1.cache[indice].vias[via_idx].datos[offset];
        }

        self.l1.estadisticas.misses += 1;
        let via_idx = self.instalar_en_l1(tag, indice);
        self.l1.cache[indice].vias[via_idx].ultimo_acceso = self.l1.contador_ciclos;
        self.l1.cache[indice].vias[via_idx].datos[offset]
    }

    /// Escribe un byte en la direccion indicada aplicando Write-Back y Write-Allocate.
    /// Si hay acierto en L1 modifica el dato y marca la linea sucia. Ante un fallo
    /// recupera previamente el bloque hacia L1 antes de escribir.
    pub fn escribir_byte(&mut self, direccion: u16, dato: u8) {
        self.l1.contador_ciclos += 1;

        let (tag, indice, offset) = self.l1.decodificar_direccion(direccion);

        let via_idx = if let Some(via_idx) = self.l1.buscar_via_hit(indice, tag) {
            self.l1.estadisticas.hits += 1;
            via_idx
        } else {
            self.l1.estadisticas.misses += 1;
            self.instalar_en_l1(tag, indice)
        };

        let linea = &mut self.l1.cache[indice].vias[via_idx];
        linea.datos[offset] = dato;
        linea.dirty_bit = true;
        linea.ultimo_acceso = self.l1.contador_ciclos;
    }

    /// Vuelca todas las lineas dirty de L1 hacia L2 y las de L2 hacia RAM.
    /// Garantiza que la RAM quede completamente sincronizada con ambos niveles de cache.
    pub fn flush(&mut self) {
        // Sincronizacion de L1 hacia L2
        for conjunto_idx in 0..CANTIDAD_CONJUNTOS {
            for via_idx in 0..2 {
                let linea = &self.l1.cache[conjunto_idx].vias[via_idx];
                if linea.valido && linea.dirty_bit {
                    let dir_base =
                        self.l1.reconstruir_direccion_base(linea.tag, conjunto_idx) as u16;
                    let datos = linea.datos;
                    Self::escribir_bloque_en_l2(&mut self.l2, &mut self.ram, dir_base, &datos);
                    self.l1.cache[conjunto_idx].vias[via_idx].dirty_bit = false;
                }
            }
        }

        // Sincronizacion de L2 hacia RAM
        for conjunto_idx in 0..CANTIDAD_CONJUNTOS_L2 {
            for via_idx in 0..2 {
                let linea = &self.l2.cache[conjunto_idx].vias[via_idx];
                if linea.valido && linea.dirty_bit {
                    let dir_base = self
                        .l2
                        .reconstruir_direccion_base_l2_pub(linea.tag, conjunto_idx)
                        as usize;
                    let datos = linea.datos;
                    self.ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                    self.l2.cache[conjunto_idx].vias[via_idx].dirty_bit = false;
                }
            }
        }
    }
}

impl Default for JerarquiaCache {
    fn default() -> Self {
        Self::nuevo()
    }
}
