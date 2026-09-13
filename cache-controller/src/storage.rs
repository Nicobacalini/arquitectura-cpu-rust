// Tamano total de la memoria RAM principal en bytes (256 bytes = direcciones de 0 a 255 -> 8 bits)
pub const TAMANO_RAM: usize = 256;
// Tamano de un bloque en bytes. Cada bloque contiene 4 bytes
pub const BLOQUE_BYTES: usize = 4;
// Cantidad de conjuntos en la cache
pub const CANTIDAD_CONJUNTOS: usize = 4;

/// Representa una linea individual de la cache
#[derive(Debug, Clone, Default)]
pub struct LineaCache {
    /// Etiqueta para identificar que bloque de RAM esta almacenado
    pub tag: u8,
    /// Indica si los datos de esta linea son validos (true) o basura false (inicializado en false)
    pub valido: bool,
    /// Bandera de modificacion (Write-Back): si es true, la CPU escribio aca
    /// y los datos de la cache son mas recientes que los de la RAM
    pub dirty_bit: bool,
    /// Los bytes reales almacenados en este bloque (4 bytes)
    pub datos: [u8; BLOQUE_BYTES],
    /// Marca de tiempo/ciclo para la politica de reemplazo LRU (Least Recently Used)
    pub ultimo_acceso: u64,
}

/// Representa un conjunto (Set). Al ser asociativa de 2 vias,
/// cada conjunto contiene exactamente 2 lineas (vias) donde un bloque puede ubicarse
#[derive(Debug, Clone, Default)]
pub struct ConjuntoCache {
    pub vias: [LineaCache; 2],
}

/// Metricas de rendimiento para evaluar la eficiencia de la cache
#[derive(Debug, Clone, Default)]
pub struct EstadisticasCache {
    /// Acierto: el dato pedido ya estaba en cache
    pub hits: u64,
    /// Fallo: el dato pedido no estaba en cache y hubo que buscarlo en RAM
    pub misses: u64,
    /// Desalojos sucios: cuantas veces se expulso una linea modificada (dirty_bit == true),
    /// lo que obliga a escribir primero el bloque en RAM antes de sobreescribirlo
    pub desalojos_dirty: u64,
}

/// Controlador central que coordina las lecturas/escrituras entre la CPU, la cache y la RAM
pub struct ControladorMemoria {
    /// Memoria principal simulada como un arreglo de 256 bytes
    pub ram: [u8; TAMANO_RAM],
    /// La memoria cache completa: 4 conjuntos x 2 vias = 8 lineas total (32 bytes de capacidad)
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS],
    /// Contador global de accesos (sirve para actualizar 'ultimo_acceso' en el LRU)
    pub contador_ciclos: u64,
    /// Registro acumulado de estadisticas
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

    /// Devuelve `(tag, indice, offset)` a partir de una direccion de 8 bits
    ///
    /// +------------+-------------+---------------+
    /// | Tag (4b)   | Index (2b)  | Offset (2b)   |
    /// +------------+-------------+---------------+
    /// | Bit 7...4  | Bit 3...2   | Bit 1...0     |
    /// +------------+-------------+---------------+
    pub fn decodificar_direccion(&self, direccion: u8) -> (u8, usize, usize) {
        let offset = (direccion & 0b0000_0011) as usize;
        let indice = ((direccion & 0b0000_1100) >> 2) as usize;
        let tag = (direccion & 0b1111_0000) >> 4;
        (tag, indice, offset)
    }

    /// Reconstruye la direccion base de un bloque a partir de su tag e indice
    /// La direccion base es la del primer byte del bloque (offset = 0)
    pub fn reconstruir_direccion_base(&self, tag: u8, indice: usize) -> u8 {
        (tag << 4) | ((indice as u8) << 2)
    }

    /// Busca, dentro del conjunto dado, una via valida cuyo tag coincida
    /// Para que haya un acierto (hit) deben cumplirse 2 condiciones:
    /// 1. via.valido: Debe ser true, lo que indica que hay informacion valida en la via, sino hay basura
    /// 2. via.tag == tag: El tag de la via debe coincidir con el tag de la direccion que se esta buscando
    /// Devuelve el indice de via (0 o 1) si hay hit, `None` si hay miss
    pub fn buscar_via_hit(&self, indice_conjunto: usize, tag: u8) -> Option<usize> {
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
    /// Si no hubo ningun acceso, devuelve 0.0 para evitar division por cero.
    pub fn tasa_de_aciertos(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}
