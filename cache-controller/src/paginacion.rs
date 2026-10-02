// paginacion.rs — Capa de Memoria Virtual: TLB, Page Table y MMU
//
// Fase 2 del TP. Esta capa se antepone a JerarquiaCache sin modificarla.
//
// Descomposicion de la direccion virtual (u16):
//
//   +--------- VPN (8 bits) ---------+------- OFFSET (8 bits) ------+
//   | Bit 15                    Bit 8 | Bit 7                  Bit 0 |
//   +---------------------------------+------------------------------+
//
// Reconstruccion de la direccion fisica:
//
//   dir_fisica = (marco_fisico as u16) << 8 | offset as u16
//
// Con marco_fisico en [0..16), el resultado cae siempre dentro de los
// 4096 bytes de TAMANO_RAM (0x0000..0x0FFF). Consecuencia directa de
// tener paginas de 256 bytes y 16 marcos fisicos.

use std::collections::VecDeque;

use crate::hierarchy::JerarquiaCache;

/// Tamano de pagina en bytes (2^8 = 256). Define los 8 bits de offset.
pub const TAMANO_PAGINA: usize = 256;

/// Cantidad de marcos fisicos disponibles = TAMANO_RAM / TAMANO_PAGINA = 16.
pub const MARCOS_FISICOS: usize = 16;

/// Cantidad de paginas virtuales posibles = 2^8 = 256.
pub const PAGINAS_VIRTUALES: usize = 256;

/// Entrada de la tabla de paginas. Una por cada pagina virtual posible (256 entradas).
#[derive(Debug, Clone)]
pub struct EntradaPagina {
    /// La entrada fue inicializada (hay una asignacion valida para esta VPN).
    pub valida: bool,
    /// La pagina esta cargada en un marco fisico actualmente.
    pub presente: bool,
    /// Numero de marco fisico asignado (0..16). Solo valido si `presente == true`.
    pub marco_fisico: u8,
    /// Proteccion: si es true, cualquier escritura genera ViolacionProteccion.
    pub solo_lectura: bool,
    /// La pagina fue modificada desde que se cargo (para write-back a disco).
    pub sucia: bool,
    /// La pagina fue accedida al menos una vez desde que se cargo.
    pub accedida: bool,
    /// A que ASID pertenece esta entrada (para el reemplazo de pagina).
    pub asid_propietario: u32,
    /// Ciclo del ultimo acceso a esta pagina (politica LRU para reemplazo de marco).
    pub ultimo_acceso: u64,
}

impl Default for EntradaPagina {
    fn default() -> Self {
        Self {
            valida: false,
            presente: false,
            marco_fisico: 0,
            solo_lectura: false,
            sucia: false,
            accedida: false,
            asid_propietario: 0,
            ultimo_acceso: 0,
        }
    }
}

/// Tabla de paginas: 256 entradas indexadas por VPN.
/// Tambien mantiene que VPN ocupa cada marco fisico para poder hacer reemplazo LRU.
pub struct TablaDePaginas {
    /// Una entrada por pagina virtual posible, indexada por VPN.
    pub entradas: Vec<EntradaPagina>,
    /// Para cada marco fisico (indice), que VPN esta mapeada ahi (None = libre).
    pub marcos_ocupados: [Option<u16>; MARCOS_FISICOS],
}

impl TablaDePaginas {
    /// Crea una tabla de paginas limpia: todas las entradas invalidas, todos los marcos libres.
    pub fn nueva() -> Self {
        Self {
            entradas: (0..PAGINAS_VIRTUALES)
                .map(|_| EntradaPagina::default())
                .collect(),
            marcos_ocupados: [None; MARCOS_FISICOS],
        }
    }

    /// Devuelve el indice del primer marco fisico libre, o None si los 16 estan ocupados.
    pub fn buscar_marco_libre(&self) -> Option<u8> {
        for (i, ocupado) in self.marcos_ocupados.iter().enumerate() {
            if ocupado.is_none() {
                return Some(i as u8);
            }
        }
        None
    }

    /// Elige el marco victima para desalojar cuando no hay marcos libres.
    ///
    /// Politica LRU: se desaloja la pagina presente con el `ultimo_acceso` mas
    /// antiguo — el mismo patron que ya se usa en L1 y L2 para elegir via victima.
    /// Consistencia intencional: es el mismo problema de reemplazo con recursos
    /// limitados apareciendo en una cuarta capa.
    pub fn elegir_marco_victima(&self) -> u8 {
        let mut victima_marco: u8 = 0;
        let mut tiempo_min = u64::MAX;

        for (marco_idx, vpn_opt) in self.marcos_ocupados.iter().enumerate() {
            if let Some(&vpn) = vpn_opt.as_ref() {
                let tiempo = self.entradas[vpn as usize].ultimo_acceso;
                if tiempo < tiempo_min {
                    tiempo_min = tiempo;
                    victima_marco = marco_idx as u8;
                }
            }
        }

        victima_marco
    }
}

/// Entrada individual de la TLB.
#[derive(Debug, Clone)]
pub struct EntradaTlb {
    /// La entrada es valida (contiene una traduccion usable).
    pub valida: bool,
    /// ASID del proceso dueno de esta traduccion.
    pub asid: u32,
    /// Numero de pagina virtual.
    pub vpn: u8,
    /// Marco fisico al que mapea esta VPN bajo este ASID.
    pub marco_fisico: u8,
    /// Ciclo del ultimo acceso para politica LRU de reemplazo en la TLB.
    pub ultimo_acceso: u64,
}

/// TLB totalmente asociativa.
///
/// Se uso LRU (mismo patron que L1/L2/Page Table) en lugar de VecDeque de indices
/// por coherencia con el resto de la jerarquia: todas las capas comparten la misma
/// politica de reemplazo, facilitando la comparacion de comportamiento entre niveles.
/// Ambas implementaciones son equivalentes en complejidad O(n); se eligio LRU
/// basado en `ultimo_acceso` por consistencia con el codigo existente.
pub struct Tlb {
    /// Vector de entradas. La capacidad maxima es configurable al construir la TLB.
    pub entradas: Vec<EntradaTlb>,
    /// Capacidad maxima de la TLB (cuantas traducciones puede retener).
    pub capacidad: usize,
    /// Cantidad de aciertos (VPN+ASID encontrados en la TLB).
    pub hits: u64,
    /// Cantidad de fallos (VPN+ASID no encontrados, hay que ir a la Page Table).
    pub misses: u64,
    /// Orden de uso para politica LRU interna (indices a `entradas`).
    /// Se usa como VecDeque para practicar la estructura pedida en el enunciado.
    orden_uso: VecDeque<usize>,
}

impl Tlb {
    /// Crea una TLB vacia con la capacidad indicada.
    pub fn nueva(capacidad: usize) -> Self {
        Self {
            entradas: Vec::with_capacity(capacidad),
            capacidad,
            hits: 0,
            misses: 0,
            orden_uso: VecDeque::with_capacity(capacidad),
        }
    }

    /// Busca una entrada valida que coincida en VPN **y** ASID.
    ///
    /// Hit si y solo si: `valida && vpn == vpn_buscada && asid == asid_buscado`.
    /// La separacion por ASID es la clave de la memoria virtual con multiproceso
    /// simulado: un proceso no puede "ver" las traducciones de otro aunque compartan
    /// el mismo espacio de direcciones virtuales.
    ///
    /// Devuelve el marco fisico si hay hit, None si hay miss.
    pub fn buscar(&mut self, vpn: u8, asid: u32, ciclo_actual: u64) -> Option<u8> {
        for (idx, entrada) in self.entradas.iter_mut().enumerate() {
            if entrada.valida && entrada.vpn == vpn && entrada.asid == asid {
                entrada.ultimo_acceso = ciclo_actual;
                // Mover este indice al frente de orden_uso (MRU)
                self.orden_uso.retain(|&x| x != idx);
                self.orden_uso.push_front(idx);
                self.hits += 1;
                return Some(entrada.marco_fisico);
            }
        }
        self.misses += 1;
        None
    }

    /// Inserta o actualiza una traduccion (vpn, asid) -> marco_fisico.
    ///
    /// Si la TLB tiene espacio libre, se agrega directamente.
    /// Si esta llena, se desaloja la entrada LRU (la del fondo de `orden_uso`).
    pub fn insertar(&mut self, vpn: u8, asid: u32, marco_fisico: u8, ciclo_actual: u64) {
        // Actualizar si ya existe la misma entrada (vpn+asid)
        for (idx, entrada) in self.entradas.iter_mut().enumerate() {
            if entrada.vpn == vpn && entrada.asid == asid {
                entrada.marco_fisico = marco_fisico;
                entrada.valida = true;
                entrada.ultimo_acceso = ciclo_actual;
                self.orden_uso.retain(|&x| x != idx);
                self.orden_uso.push_front(idx);
                return;
            }
        }

        let nueva_entrada = EntradaTlb {
            valida: true,
            asid,
            vpn,
            marco_fisico,
            ultimo_acceso: ciclo_actual,
        };

        if self.entradas.len() < self.capacidad {
            // Hay lugar: agregar al final del Vec y al frente del orden_uso
            let idx = self.entradas.len();
            self.entradas.push(nueva_entrada);
            self.orden_uso.push_front(idx);
        } else {
            // TLB llena: desalojar la entrada LRU (al fondo de orden_uso)
            if let Some(idx_victima) = self.orden_uso.pop_back() {
                self.entradas[idx_victima] = nueva_entrada;
                self.orden_uso.push_front(idx_victima);
            }
        }
    }

    /// Invalida todas las entradas de la TLB con el ASID indicado.
    /// Equivalente a un TLB flush selectivo en un cambio de contexto.
    pub fn invalidar_asid(&mut self, asid: u32) {
        for entrada in &mut self.entradas {
            if entrada.asid == asid {
                entrada.valida = false;
            }
        }
    }

    /// Devuelve la tasa de aciertos de la TLB (hits / (hits + misses)).
    pub fn tasa_hits(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }
}

/// Tipo de acceso que se esta realizando a memoria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoAcceso {
    Lectura,
    Escritura,
}

/// Resultado de intentar traducir una direccion virtual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultadoTraduccion {
    /// Traduccion exitosa. `direccion_fisica` esta lista para pasarse a JerarquiaCache.
    Exitosa { direccion_fisica: u16 },
    /// La pagina no estaba cargada en RAM fisica. Se requiere traer la pagina.
    PageFault { vpn: u8 },
    /// Intento de escritura en una pagina marcada solo_lectura.
    ViolacionProteccion { vpn: u8, fue_escritura: bool },
}

/// Descompone una direccion virtual de 16 bits en (vpn, offset).
///
/// La direccion virtual tiene el formato:
/// - Bits [15..8]: VPN (8 bits) — identifica cual pagina virtual
/// - Bits [7..0]:  offset (8 bits) — byte dentro de la pagina
pub fn descomponer_direccion_virtual(direccion: u16) -> (u8, u8) {
    let offset = (direccion & 0xFF) as u8;
    let vpn = ((direccion >> 8) & 0xFF) as u8;
    (vpn, offset)
}

/// Reconstruye la direccion fisica a partir del marco fisico y el offset.
///
/// Con marco_fisico en [0..16), el resultado siempre cae en [0..4096),
/// que es exactamente el rango de TAMANO_RAM — por diseno, no por casualidad.
pub fn reconstruir_direccion_fisica(marco_fisico: u8, offset: u8) -> u16 {
    ((marco_fisico as u16) << 8) | (offset as u16)
}

/// Unidad de Gestion de Memoria: combina TLB, Page Table y JerarquiaCache.
///
/// Es la unica interfaz que `cpu-pipeline` deberia usar a partir de la Fase 2.
/// Internamente: cada acceso pasa por TLB → Page Table → JerarquiaCache.
/// La JerarquiaCache no se modifica: sigue trabajando con direcciones fisicas.
pub struct Mmu {
    /// Buffer de traduccion adelantada (Translation Lookaside Buffer).
    pub tlb: Tlb,
    /// Tabla de paginas: fuente de verdad de las traducciones VPN → marco.
    pub page_table: TablaDePaginas,
    /// Jerarquia de cache L1+L2+RAM de la Fase 1, sin modificaciones.
    pub jerarquia: JerarquiaCache,
    /// ASID del proceso actualmente en ejecucion.
    pub asid_actual: u32,
    /// Contador global de ciclos (compartido con la logica de LRU).
    pub contador_ciclos: u64,
    /// Penalidad en ciclos por cada TLB miss (ir a Page Table).
    /// Valor tipico: 10 ciclos.
    pub penalidad_tlb_miss: u32,
    /// Penalidad en ciclos por cada page fault (ir a disco/swap simulado).
    /// Valor: 1_000_000 ciclos. Nota de escala: en un sistema real son ~10ms
    /// a 100MHz = 1_000_000 ciclos. Se usa esta escala de juguete para que
    /// los contadores sean observables sin simular decenas de millones de ciclos.
    pub penalidad_page_fault: u32,
    /// Cantidad total de page faults ocurridos.
    pub page_faults: u64,
    /// Cantidad total de violaciones de proteccion (escritura en pagina RO).
    pub violaciones_proteccion: u64,
}

impl Mmu {
    /// Crea una MMU nueva con TLB de 8 entradas y penalidades por defecto.
    pub fn nueva() -> Self {
        Self {
            tlb: Tlb::nueva(8),
            page_table: TablaDePaginas::nueva(),
            jerarquia: JerarquiaCache::nuevo(),
            asid_actual: 0,
            contador_ciclos: 0,
            penalidad_tlb_miss: 10,
            penalidad_page_fault: 1_000_000,
            page_faults: 0,
            violaciones_proteccion: 0,
        }
    }

    /// Alias en ingles.
    #[inline]
    pub fn new() -> Self {
        Self::nueva()
    }

    /// Cambia el ASID actual. Equivalente a un cambio de contexto de proceso.
    ///
    /// No invalida la TLB completa porque el ASID en cada entrada ya previene
    /// que un proceso acceda a traducciones de otro. Si se quisiera forzar un
    /// flush selectivo, llamar a `tlb.invalidar_asid(asid_viejo)`.
    pub fn cambiar_asid(&mut self, nuevo_asid: u32) {
        self.asid_actual = nuevo_asid;
    }

    /// Lee un byte de la direccion virtual indicada.
    ///
    /// Flujo:
    /// 1. Traducir direccion virtual → fisica (TLB → Page Table).
    /// 2. Si `Exitosa`, delegar a `jerarquia.leer_byte(dir_fisica)`.
    /// 3. Si `PageFault` o `ViolacionProteccion`: acumular penalidad y devolver
    ///    `Some(0)` — decision de diseno: la CPU recibe un valor neutro (0) y el
    ///    costo ya quedo registrado en `contador_ciclos`. El registro destino
    ///    queda con el valor 0 (equivalente a "dato invalido aun no disponible").
    ///    Esta decision simplifica el pipeline: no hay stall adicional, pero el
    ///    costo en ciclos es visible en el AMAT extendido.
    pub fn leer_byte(
        &mut self,
        direccion_virtual: u16,
        tipo_para_proteccion: TipoAcceso,
    ) -> Option<u8> {
        self.contador_ciclos += 1;
        match traducir_direccion(self, direccion_virtual, &tipo_para_proteccion) {
            ResultadoTraduccion::Exitosa { direccion_fisica } => {
                Some(self.jerarquia.leer_byte(direccion_fisica))
            }
            ResultadoTraduccion::PageFault { .. } => {
                // La pagina fue instalada por traducir_direccion. Acumular penalidad
                // y reintentar la traduccion (ahora la pagina esta presente).
                // Modela el comportamiento real del OS: luego del page fault, la
                // instruccion que fallo se reintenta automaticamente.
                self.contador_ciclos += self.penalidad_page_fault as u64;
                match traducir_direccion(self, direccion_virtual, &tipo_para_proteccion) {
                    ResultadoTraduccion::Exitosa { direccion_fisica } => {
                        Some(self.jerarquia.leer_byte(direccion_fisica))
                    }
                    _ => Some(0),
                }
            }
            ResultadoTraduccion::ViolacionProteccion { .. } => {
                Some(0)
            }
        }
    }

    /// Escribe un byte en la direccion virtual indicada.
    ///
    /// Devuelve `true` si la escritura se completo, `false` si hubo page fault
    /// o violacion de proteccion (en cuyo caso no se toca la jerarquia de cache).
    pub fn escribir_byte(&mut self, direccion_virtual: u16, dato: u8) -> bool {
        self.contador_ciclos += 1;
        match traducir_direccion(self, direccion_virtual, &TipoAcceso::Escritura) {
            ResultadoTraduccion::Exitosa { direccion_fisica } => {
                self.jerarquia.escribir_byte(direccion_fisica, dato);
                true
            }
            ResultadoTraduccion::PageFault { .. } => {
                // La pagina fue instalada. Acumular penalidad y reintentar la escritura.
                // Modela el comportamiento real del OS: la instruccion se reintenta
                // despues de que el manejador de page fault carga la pagina.
                self.contador_ciclos += self.penalidad_page_fault as u64;
                match traducir_direccion(self, direccion_virtual, &TipoAcceso::Escritura) {
                    ResultadoTraduccion::Exitosa { direccion_fisica } => {
                        self.jerarquia.escribir_byte(direccion_fisica, dato);
                        true
                    }
                    _ => false,
                }
            }
            ResultadoTraduccion::ViolacionProteccion { .. } => {
                false
            }
        }
    }

    /// Expone estadisticas L1 para el reporte de rendimiento.
    #[inline]
    pub fn estadisticas_l1(&self) -> &crate::storage::EstadisticasCache {
        &self.jerarquia.l1.estadisticas
    }

    /// Exposa estadisticas L2 para el reporte de rendimiento.
    #[inline]
    pub fn estadisticas_l2(&self) -> &crate::storage::EstadisticasCache {
        &self.jerarquia.l2.estadisticas
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self::nueva()
    }
}

// ─── Logica de traduccion (separada para facilitar tests) ─────────────────────

/// Traduce una direccion virtual a fisica pasando por TLB y Page Table.
///
/// Orden de consulta:
/// 1. TLB (VPN + ASID) → hit directo, sin penalidad.
/// 2. Page Table       → penalidad `penalidad_tlb_miss` por el miss en TLB.
///    a. Entrada invalida o no presente → PageFault + reemplazo de pagina.
///    b. Escritura en pagina solo_lectura → ViolacionProteccion.
///    c. OK → instalar en TLB y devolver Exitosa.
pub fn traducir_direccion(
    mmu: &mut Mmu,
    direccion: u16,
    tipo: &TipoAcceso,
) -> ResultadoTraduccion {
    let (vpn, offset) = descomponer_direccion_virtual(direccion);

    if let Some(marco) = mmu.tlb.buscar(vpn, mmu.asid_actual, mmu.contador_ciclos) {
        // TLB hit: verificar proteccion de escritura
        let solo_lectura = mmu.page_table.entradas[vpn as usize].solo_lectura;
        if *tipo == TipoAcceso::Escritura && solo_lectura {
            mmu.violaciones_proteccion += 1;
            return ResultadoTraduccion::ViolacionProteccion {
                vpn,
                fue_escritura: true,
            };
        }
        // Actualizar bits de acceso / suciedad
        {
            let entrada = &mut mmu.page_table.entradas[vpn as usize];
            entrada.accedida = true;
            entrada.ultimo_acceso = mmu.contador_ciclos;
            if *tipo == TipoAcceso::Escritura {
                entrada.sucia = true;
            }
        }
        return ResultadoTraduccion::Exitosa {
            direccion_fisica: reconstruir_direccion_fisica(marco, offset),
        };
    }

    mmu.contador_ciclos += mmu.penalidad_tlb_miss as u64;

    // Leer los campos que necesitamos del borrow inmutable antes de mutar
    let (es_valida, es_presente, es_solo_lectura, marco_en_pt) = {
        let e = &mmu.page_table.entradas[vpn as usize];
        (e.valida, e.presente, e.solo_lectura, e.marco_fisico)
    };

    // Verificar proteccion antes de resolver el page fault
    if es_valida && es_presente && *tipo == TipoAcceso::Escritura && es_solo_lectura {
        mmu.violaciones_proteccion += 1;
        return ResultadoTraduccion::ViolacionProteccion {
            vpn,
            fue_escritura: true,
        };
    }

    // Entrada valida y presente → Page Table hit (TLB miss ya contado)
    if es_valida && es_presente {
        let marco = marco_en_pt;
        {
            let e = &mut mmu.page_table.entradas[vpn as usize];
            e.accedida = true;
            e.ultimo_acceso = mmu.contador_ciclos;
            if *tipo == TipoAcceso::Escritura {
                e.sucia = true;
            }
        }
        mmu.tlb.insertar(vpn, mmu.asid_actual, marco, mmu.contador_ciclos);
        return ResultadoTraduccion::Exitosa {
            direccion_fisica: reconstruir_direccion_fisica(marco, offset),
        };
    }

    // ── Page fault ───────────────────────────────────────────────────────────
    mmu.page_faults += 1;

    // Encontrar un marco libre o desalojar la pagina LRU
    let marco = if let Some(m) = mmu.page_table.buscar_marco_libre() {
        m
    } else {
        // No hay marcos libres: desalojar la pagina con ultimo_acceso mas antiguo
        let marco_victima = mmu.page_table.elegir_marco_victima();
        let vpn_victima = mmu.page_table.marcos_ocupados[marco_victima as usize]
            .expect("marco_victima debe tener VPN asignada") as u8;

        // Desalojar la pagina victima
        mmu.page_table.entradas[vpn_victima as usize].presente = false;

        // Invalidar entradas de TLB que apunten a la pagina desalojada
        for tlb_entry in &mut mmu.tlb.entradas {
            if tlb_entry.vpn == vpn_victima {
                tlb_entry.valida = false;
            }
        }

        mmu.page_table.marcos_ocupados[marco_victima as usize] = None;
        marco_victima
    };

    // Instalar la nueva pagina en el marco obtenido
    mmu.page_table.marcos_ocupados[marco as usize] = Some(vpn as u16);
    {
        let entrada_nueva = &mut mmu.page_table.entradas[vpn as usize];
        entrada_nueva.valida = true;
        entrada_nueva.presente = true;
        entrada_nueva.marco_fisico = marco;
        entrada_nueva.asid_propietario = mmu.asid_actual;
        entrada_nueva.accedida = true;
        entrada_nueva.ultimo_acceso = mmu.contador_ciclos;
        if *tipo == TipoAcceso::Escritura {
            entrada_nueva.sucia = true;
        }
    }

    mmu.tlb.insertar(vpn, mmu.asid_actual, marco, mmu.contador_ciclos);

    ResultadoTraduccion::PageFault { vpn }
}

/// Calcula el AMAT extendido con TLB, Page Table y la jerarquia L1/L2.
///
/// Formula:
///   AMAT_TLB   = T_tlb + miss_tlb * (T_pt + miss_pf * P_pf)
///   AMAT_total = AMAT_TLB + AMAT_L1
///
/// donde AMAT_L1 es el AMAT de la jerarquia cache de la Fase 1.
pub fn calcular_amat(
    tiempo_tlb: f64,
    tasa_miss_tlb: f64,
    tiempo_page_table: f64,
    tasa_page_fault: f64,
    penalidad_page_fault: f64,
    amat_l1: f64,
) -> f64 {
    let amat_tlb =
        tiempo_tlb + tasa_miss_tlb * (tiempo_page_table + tasa_page_fault * penalidad_page_fault);
    amat_tlb + amat_l1
}
