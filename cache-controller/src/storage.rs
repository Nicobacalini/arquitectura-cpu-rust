pub const TAMANO_RAM: usize = 4096;
pub const BLOQUE_BYTES: usize = 4;
pub const CANTIDAD_CONJUNTOS: usize = 4;
pub const CANTIDAD_CONJUNTOS_L2: usize = 8;

/// Representa una linea individual de la cache
#[derive(Debug, Clone, Default)]
pub struct LineaCache {
    /// Etiqueta para identificar que bloque de RAM esta almacenado
    pub tag: u16,
    /// Indica si los datos de esta linea son validos
    pub valido: bool,
    /// Bandera de modificacion para politica Write-Back
    pub dirty_bit: bool,
    /// Bytes almacenados en este bloque
    pub datos: [u8; BLOQUE_BYTES],
    /// Marca de ciclo para politica de reemplazo LRU
    pub ultimo_acceso: u64,
}

/// Representa un conjunto asociativo de 2 vias
#[derive(Debug, Clone, Default)]
pub struct ConjuntoCache {
    pub vias: [LineaCache; 2],
}

/// Representa el segundo nivel de cache asociativo por conjuntos
#[derive(Debug, Clone)]
pub struct NivelL2 {
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS_L2],
    pub contador_ciclos: u64,
    pub estadisticas: EstadisticasCache,
}

/// Metricas de rendimiento para evaluar la eficiencia de la cache
#[derive(Debug, Clone, Default)]
pub struct EstadisticasCache {
    pub hits: u64,
    pub misses: u64,
    pub desalojos_dirty: u64,
}

/// Controlador central que coordina lecturas y escrituras entre CPU, cache y RAM
pub struct ControladorMemoria {
    pub ram: [u8; TAMANO_RAM],
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS],
    pub contador_ciclos: u64,
    pub estadisticas: EstadisticasCache,
}

impl ControladorMemoria {
    /// Crea un nuevo controlador de memoria con cache y RAM vacias
    pub fn nuevo() -> Self {
        let linea_vacia = LineaCache::default();
        let conjunto_vacio = ConjuntoCache {
            vias: [linea_vacia.clone(), linea_vacia],
        };

        Self {
            ram: [0; TAMANO_RAM],
            cache: [
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio,
            ],
            contador_ciclos: 0,
            estadisticas: EstadisticasCache::default(),
        }
    }

    /// Alias idiomatico de Rust para [`ControladorMemoria::nuevo`]
    #[inline]
    pub fn new() -> Self {
        Self::nuevo()
    }

    /// Devuelve `(tag, indice, offset)` a partir de una direccion de 16 bits
    ///
    /// +-------------+-------------+---------------+
    /// | Tag (12b)   | Index (2b)  | Offset (2b)   |
    /// +-------------+-------------+---------------+
    /// | Bit 15...4  | Bit 3...2   | Bit 1...0     |
    /// +-------------+-------------+---------------+
    pub fn decodificar_direccion(&self, direccion: u16) -> (u16, usize, usize) {
        let offset = (direccion & 0b0000_0011) as usize;
        let indice = ((direccion & 0b0000_1100) >> 2) as usize;
        let tag = (direccion >> 4) & ((1u16 << 12) - 1);
        (tag, indice, offset)
    }

    /// Reconstruye la direccion base de un bloque a partir de su tag e indice
    pub fn reconstruir_direccion_base(&self, tag: u16, indice: usize) -> u16 {
        (tag << 4) | ((indice as u16) << 2)
    }

    /// Busca dentro del conjunto dado una via valida cuyo tag coincida.
    /// Devuelve el indice de via si hay acierto o None si hay fallo.
    pub fn buscar_via_hit(&self, indice_conjunto: usize, tag: u16) -> Option<usize> {
        for (via_idx, via) in self.cache[indice_conjunto].vias.iter().enumerate() {
            if via.valido && via.tag == tag {
                return Some(via_idx);
            }
        }
        None
    }
}

impl EstadisticasCache {
    /// Devuelve el porcentaje de aciertos sobre el total de accesos.
    /// Si no hubo ningun acceso devuelve 0.0.
    pub fn tasa_de_aciertos(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}

// ---------------------------------------------------------------------------
// NivelL2 — cache de segundo nivel, 8 conjuntos x 2 vias, Write-Back / LRU
//
// Mapa de bits de la direccion de 16 bits:
//
//   +-------------+-----------+----------+
//   |  Tag (11b)  | Index (3b)| Offset(2b)|
//   +-------------+-----------+----------+
//   | Bit 15...5  | Bit 4...2 | Bit 1...0 |
//   +-------------+-----------+----------+
//
// Offset = 2 bits  →  bloque de 4 bytes  (igual que L1, BLOQUE_BYTES = 4)
// Index  = 3 bits  →  8 conjuntos        (CANTIDAD_CONJUNTOS_L2 = 8)
// Tag    = 11 bits →  16 - 3 - 2 = 11
// ---------------------------------------------------------------------------

impl NivelL2 {
    /// Crea un NivelL2 con todas las vias invalidas y estadisticas en cero.
    pub fn nuevo() -> Self {
        let linea_vacia = LineaCache {
            tag: 0,
            valido: false,
            dirty_bit: false,
            datos: [0; BLOQUE_BYTES],
            ultimo_acceso: 0,
        };
        let conjunto_vacio = ConjuntoCache {
            vias: [linea_vacia.clone(), linea_vacia],
        };
        Self {
            cache: [
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio.clone(),
                conjunto_vacio,
            ],
            contador_ciclos: 0,
            estadisticas: EstadisticasCache::default(),
        }
    }

    #[inline]
    pub fn new() -> Self {
        Self::nuevo()
    }

    /// Decodifica una direccion de 16 bits usando el esquema propio de L2 con tag de 11 bits, indice de 3 bits y offset de 2 bits.
    pub fn decodificar_direccion(&self, direccion: u16) -> (u16, usize, usize) {
        let offset = (direccion & 0b0000_0000_0000_0011) as usize;
        let indice = ((direccion & 0b0000_0000_0001_1100) >> 2) as usize;
        let tag = (direccion >> 5) & ((1u16 << 11) - 1);
        (tag, indice, offset)
    }

    /// Reconstruye la direccion base a partir de tag e indice L2.
    pub(crate) fn reconstruir_direccion_base_l2(&self, tag: u16, indice: usize) -> u16 {
        (tag << 5) | ((indice as u16) << 2)
    }

    /// Reconstruye la direccion base publicamente para uso desde otros modulos.
    pub fn reconstruir_direccion_base_l2_pub(&self, tag: u16, indice: usize) -> u16 {
        self.reconstruir_direccion_base_l2(tag, indice)
    }

    /// Busca una via valida con el tag indicado dentro del conjunto.
    pub fn buscar_via_hit_l2(&self, indice_conjunto: usize, tag: u16) -> Option<usize> {
        for (via_idx, via) in self.cache[indice_conjunto].vias.iter().enumerate() {
            if via.valido && via.tag == tag {
                return Some(via_idx);
            }
        }
        None
    }

    /// Elige la via victima dentro del conjunto para reemplazo LRU.
    /// Prioriza vias invalidas eligiendo primero la via cero. Si ambas son validas
    /// desaloja la de menor tiempo de acceso desempatando por la via uno.
    fn elegir_via_victima_l2(&self, indice_conjunto: usize) -> usize {
        let via0 = &self.cache[indice_conjunto].vias[0];
        let via1 = &self.cache[indice_conjunto].vias[1];

        if !via0.valido {
            return 0;
        }
        if !via1.valido {
            return 1;
        }
        if via0.ultimo_acceso < via1.ultimo_acceso {
            0
        } else {
            1
        }
    }

    /// Maneja un fallo de L2 seleccionando victima, realizando write-back si es dirty
    /// y trayendo el bloque nuevo desde memoria RAM.
    fn manejar_miss_l2(
        &mut self,
        indice_conjunto: usize,
        tag: u16,
        ram: &mut [u8; TAMANO_RAM],
    ) -> usize {
        let via_victima = self.elegir_via_victima_l2(indice_conjunto);

        // Volcado a RAM si la victima es valida y dirty
        {
            let linea = &self.cache[indice_conjunto].vias[via_victima];
            if linea.valido && linea.dirty_bit {
                let dir_base =
                    self.reconstruir_direccion_base_l2(linea.tag, indice_conjunto) as usize;
                let datos = linea.datos;
                ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                self.estadisticas.desalojos_dirty += 1;
            }
        }

        let dir_base = self.reconstruir_direccion_base_l2(tag, indice_conjunto) as usize;
        let mut nuevos_datos = [0u8; BLOQUE_BYTES];
        nuevos_datos.copy_from_slice(&ram[dir_base..dir_base + BLOQUE_BYTES]);

        let linea = &mut self.cache[indice_conjunto].vias[via_victima];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = false;
        linea.datos = nuevos_datos;
        linea.ultimo_acceso = self.contador_ciclos;

        via_victima
    }

    /// Lee un byte de direccion usando L2 de forma directa.
    /// Ante acierto actualiza el acceso y devuelve el dato. Ante fallo
    /// carga previamente el bloque desde RAM.
    pub fn leer_byte(&mut self, direccion: u16, ram: &mut [u8; TAMANO_RAM]) -> u8 {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        if let Some(via_idx) = self.buscar_via_hit_l2(indice, tag) {
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.estadisticas.hits += 1;
            self.cache[indice].vias[via_idx].datos[offset]
        } else {
            self.estadisticas.misses += 1;
            let via_idx = self.manejar_miss_l2(indice, tag, ram);
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.cache[indice].vias[via_idx].datos[offset]
        }
    }

    /// Escribe un byte en direccion usando Write-Back y Write-Allocate.
    /// Ante acierto escribe en cache y marca dirty. Ante fallo carga primero
    /// el bloque antes de escribir.
    pub fn escribir_byte(&mut self, direccion: u16, dato: u8, ram: &mut [u8; TAMANO_RAM]) {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        let via_idx = if let Some(via_idx) = self.buscar_via_hit_l2(indice, tag) {
            self.estadisticas.hits += 1;
            via_idx
        } else {
            self.estadisticas.misses += 1;
            self.manejar_miss_l2(indice, tag, ram)
        };

        let linea = &mut self.cache[indice].vias[via_idx];
        linea.datos[offset] = dato;
        linea.dirty_bit = true;
        linea.ultimo_acceso = self.contador_ciclos;
    }
}

impl Default for NivelL2 {
    fn default() -> Self {
        Self::nuevo()
    }
}
