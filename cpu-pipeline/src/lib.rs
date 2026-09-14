use std::fmt;

use cache_controller::ControladorMemoria;

// ─── Memoria ────────────────────────────────────────────────────────────────
// `ControladorMemoria` (re-exportado desde el crate `cache-controller`) actua
// como la memoria del pipeline: cada LOAD/STORE pasa por la cache asociativa
// de 4 conjuntos x 2 vias con politica LRU y Write-Back/Write-Allocate.
// Se re-exporta aqui para que los binarios que consumen `cpu-pipeline` no
// necesiten depender de `cache-controller` directamente.
pub use cache_controller::ControladorMemoria as Memoria;

/// Alias de compatibilidad: apunta al [`ControladorMemoria`] real del crate
/// `cache-controller`. Todo codigo existente que use `MemoriaProvisoria`
/// pasa ahora por la cache asociativa (LRU, Write-Back) en lugar de acceder
/// directamente a la RAM.
pub type MemoriaProvisoria = ControladorMemoria;

// ─── ISA ────────────────────────────────────────────────────────────────────

/// Identificador de un registro general de la CPU.
/// R0 es hardwired-zero: siempre vale 0 y no puede ser modificado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registro {
    R0,
    R1,
    R2,
    R3,
}

/// Conjunto de instrucciones soportadas por la CPU simulada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruccion {
    /// No hace nada. Ocupa un ciclo de pipeline (burbuja).
    NOP,
    /// Suma los registros `src1` y `src2` y guarda el resultado en `dest`.
    ADD {
        dest: Registro,
        src1: Registro,
        src2: Registro,
    },
    /// Resta `src2` a `src1` y guarda el resultado en `dest`.
    SUB {
        dest: Registro,
        src1: Registro,
        src2: Registro,
    },
    /// Lee un byte de la RAM en `direccion_ram` y lo guarda en `dest`.
    LOAD { dest: Registro, direccion_ram: u8 },
    /// Escribe el byte del registro `src` en la RAM en `direccion_ram`.
    STORE { src: Registro, direccion_ram: u8 },
    /// Salta incondicionalmente a la instruccion en `direccion_destino`.
    /// Flushea las dos instrucciones especulativas que estaban en IF/ID e ID/EX.
    JUMP { direccion_destino: usize },
}

// ─── Registros de segmentacion ───────────────────────────────────────────────

/// Estado fisico de un registro de segmentacion entre dos etapas del pipeline.
/// Cada campo del pipeline (IF/ID, ID/EX, EX/MEM, MEM/WB) es una instancia de esta estructura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistroSegmentacion {
    /// Instruccion que viaja a traves del pipeline en este registro.
    pub instruccion: Instruccion,
    /// Indica si el registro contiene una instruccion valida (`true`)
    /// o es una burbuja NOP (`false`).
    pub activa: bool,
    /// Resultado producido por la ALU o leido de la RAM.
    /// Es `None` mientras la etapa correspondiente no haya calculado el valor.
    pub resultado: Option<u16>,
}

// ─── CPU ─────────────────────────────────────────────────────────────────────

/// Estado global del procesador con pipeline de 5 etapas (IF, ID, EX, MEM, WB).
pub struct CpuSegmentada {
    /// Registro de segmentacion IF/ID.
    /// Almacena la instruccion recien buscada en memoria (Fetch)
    /// lista para ser decodificada (Decode).
    pub if_id: RegistroSegmentacion,

    /// Registro de segmentacion ID/EX.
    /// Almacena la instruccion decodificada y sus operandos,
    /// lista para ser ejecutada en la ALU (Execute).
    pub id_ex: RegistroSegmentacion,

    /// Registro de segmentacion EX/MEM.
    /// Almacena la instruccion y el resultado de la ALU,
    /// lista para acceder a la memoria RAM si es necesario (Memory).
    pub ex_mem: RegistroSegmentacion,

    /// Registro de segmentacion MEM/WB.
    /// Almacena el dato listo (de la ALU o leido de la RAM)
    /// para ser escrito en el banco de registros (Write Back).
    pub mem_wb: RegistroSegmentacion,

    /// Banco de registros generales de la CPU (R0, R1, R2, R3).
    /// `registros[0]` corresponde a R0 y siempre vale 0 (hardwired-zero).
    pub registros: [u16; 4],

    /// Contador de Programa (PC).
    /// Indice de la proxima instruccion a buscar en el slice de programa.
    pub program_counter: usize,

    /// Total de ciclos de reloj ejecutados desde el inicio de la simulacion.
    pub contador_ciclos: u64,

    /// Total de instrucciones completadas exitosamente (retiradas en Write Back).
    pub instrucciones_completadas: u64,
}

impl CpuSegmentada {
    /// Crea una nueva CPU segmentada en su estado inicial limpio (de stock):
    /// - Los 4 registros de segmentacion (`if_id`, `id_ex`, `ex_mem`, `mem_wb`) como burbujas NOP inactivas.
    /// - Banco de registros R0..R3 en 0.
    /// - Contador de Programa (`program_counter`) en 0.
    /// - `contador_ciclos` e `instrucciones_completadas` en 0.
    ///
    /// Puede usarse directamente o junto con la sintaxis de actualizacion de Rust (`..`):
    /// ```rust
    /// use cpu_pipeline::CpuSegmentada;
    /// let cpu = CpuSegmentada {
    ///     registros: [0, 0, 10, 0],
    ///     ..CpuSegmentada::nueva()
    /// };
    /// ```
    pub fn nueva() -> Self {
        let burbuja = RegistroSegmentacion {
            instruccion: Instruccion::NOP,
            activa: false,
            resultado: None,
        };

        Self {
            if_id: burbuja,
            id_ex: burbuja,
            ex_mem: burbuja,
            mem_wb: burbuja,
            registros: [0; 4],
            program_counter: 0,
            contador_ciclos: 0,
            instrucciones_completadas: 0,
        }
    }

    /// Alias en ingles
    #[inline]
    pub fn new() -> Self {
        Self::nueva()
    }

    /// Detecta si existe un Load-Use Hazard entre la instruccion en ID y el LOAD en EX.
    ///
    /// Retorna `true` cuando la instruccion en `id_ex` es un LOAD activo y la instruccion
    /// en `if_id` (parametro `instruccion_en_id`) usa como fuente el registro destino de ese LOAD.
    /// En ese caso se debe insertar una burbuja (stall) porque el dato del LOAD no estara
    /// disponible hasta que termine la etapa MEM, un ciclo despues de cuando la instruccion
    /// siguiente lo necesita en EX. Este hazard no puede resolverse con forwarding.
    pub fn detectar_load_use_hazard(&self, instruccion_en_id: &Instruccion) -> bool {
        match &self.id_ex.instruccion {
            Instruccion::LOAD { dest: reg_load, .. } if self.id_ex.activa => {
                match instruccion_en_id {
                    Instruccion::ADD { src1, src2, .. } | Instruccion::SUB { src1, src2, .. } => {
                        src1 == reg_load || src2 == reg_load
                    }
                    Instruccion::STORE { src, .. } => src == reg_load,
                    // JUMP no usa registros de datos como fuente, nunca genera Load-Use Hazard
                    Instruccion::NOP | Instruccion::LOAD { .. } | Instruccion::JUMP { .. } => false,
                }
            }
            Instruccion::ADD { .. }
            | Instruccion::SUB { .. }
            | Instruccion::STORE { .. }
            | Instruccion::NOP
            | Instruccion::JUMP { .. } => false,
            // LOAD inactivo
            Instruccion::LOAD { .. } => false,
        }
    }

    /// Busca el valor del registro fuente `src` en las etapas activas del pipeline
    /// (forwarding), evitando leer un dato desactualizado del banco de registros.
    ///
    /// Comprueba primero EX/MEM (mayor prioridad, dato mas reciente) y luego MEM/WB.
    /// Si ninguna etapa produce `src`, retorna `None` para que `resolver_operando`
    /// lea el valor del banco de registros.
    ///
    /// R0 nunca se anticipa: como R0 es hardwired-zero, se retorna `None` directamente
    /// y `resolver_operando` devuelve `registros[0]` que siempre es 0.
    pub fn calcular_forwarding(&self, src: Registro) -> Option<u16> {
        // R0 es hardwired-zero: nunca se anticipa desde el pipeline.
        // El banco de registros garantiza registros[0] == 0 siempre.
        if src == Registro::R0 {
            return None;
        }

        // EX/MEM: maxima prioridad (dato mas reciente)
        if self.ex_mem.activa {
            match self.ex_mem.instruccion {
                Instruccion::ADD { dest, .. }
                | Instruccion::SUB { dest, .. }
                | Instruccion::LOAD { dest, .. }
                    if dest == src =>
                {
                    // dest == R0 ya fue descartado arriba, este dest es R1..R3.
                    // Si resultado es None (LOAD todavia en EX/MEM), no caemos a MEM/WB.
                    return self.ex_mem.resultado;
                }
                _ => {}
            }
        }

        // MEM/WB: segunda prioridad
        if self.mem_wb.activa {
            match self.mem_wb.instruccion {
                Instruccion::ADD { dest, .. }
                | Instruccion::SUB { dest, .. }
                | Instruccion::LOAD { dest, .. }
                    if dest == src =>
                {
                    if let Some(valor) = self.mem_wb.resultado {
                        return Some(valor);
                    }
                }
                _ => {}
            }
        }

        // Ninguna etapa en el pipeline produce `src`: leer del banco de registros
        None
    }

    /// Resuelve el valor del registro fuente `src` que la ALU debe usar como operando.
    ///
    /// Primero intenta obtener el valor via forwarding (`calcular_forwarding`).
    /// Si no hay forwarding disponible, lee directamente del banco de registros.
    /// Equivale al multiplexor (MUX) a la entrada de la ALU en hardware real.
    pub fn resolver_operando(&self, src: Registro) -> u16 {
        match self.calcular_forwarding(src) {
            Some(valor) => valor,
            None => {
                let idx = match src {
                    Registro::R0 => 0,
                    Registro::R1 => 1,
                    Registro::R2 => 2,
                    Registro::R3 => 3,
                };
                self.registros[idx]
            }
        }
    }

    /// Ejecuta la etapa EX (Execute) sobre la instruccion almacenada en `instruccion`.
    ///
    /// Calcula el campo `resultado` segun el tipo de instruccion y devuelve el
    /// `RegistroSegmentacion` resultante listo para avanzar a EX/MEM:
    /// - `ADD` / `SUB`: suma o resta con aritmetica modular (wrapping) para evitar panics.
    /// - `STORE`: resuelve el operando fuente (con forwarding si aplica) y lo empaqueta
    ///   en `resultado` para transportarlo hasta la etapa MEM.
    /// - `LOAD`: no produce resultado en EX; deja `resultado = None` para que el
    ///   forwarding no adelante un dato inexistente antes de que MEM lea la RAM.
    /// - `NOP` / buffer inactivo: propaga una burbuja limpia sin efectos secundarios.
    pub fn ejecutar_alu(&self, instruccion: RegistroSegmentacion) -> RegistroSegmentacion {
        if !instruccion.activa {
            return RegistroSegmentacion {
                instruccion: Instruccion::NOP,
                activa: false,
                resultado: None,
            };
        }

        let resultado = match instruccion.instruccion {
            Instruccion::ADD { src1, src2, .. } => {
                let op1 = self.resolver_operando(src1);
                let op2 = self.resolver_operando(src2);
                Some(op1.wrapping_add(op2))
            }
            Instruccion::SUB { src1, src2, .. } => {
                let op1 = self.resolver_operando(src1);
                let op2 = self.resolver_operando(src2);
                Some(op1.wrapping_sub(op2))
            }
            Instruccion::LOAD { .. } => None,
            Instruccion::STORE { src, .. } => Some(self.resolver_operando(src)),
            Instruccion::NOP => None,
            Instruccion::JUMP { .. } => None,
        };

        RegistroSegmentacion {
            resultado,
            ..instruccion
        }
    }

    /// Ejecuta la etapa MEM (Memory) sobre la instruccion almacenada en `instruccion`.
    ///
    /// - `LOAD`: lee un byte de `memoria` en la direccion indicada y lo escribe en `resultado`.
    /// - `STORE`: toma el valor en `instruccion.resultado` (empaquetado en EX) y lo escribe
    ///   en `memoria` en la direccion indicada.
    /// - Cualquier otra instruccion o buffer inactivo: se propaga sin cambios.
    pub fn ejecutar_mem(
        &self,
        instruccion: RegistroSegmentacion,
        memoria: &mut ControladorMemoria,
    ) -> RegistroSegmentacion {
        if !instruccion.activa {
            return instruccion;
        }

        match instruccion.instruccion {
            Instruccion::LOAD { direccion_ram, .. } => {
                let dato = memoria.leer_byte(direccion_ram);
                RegistroSegmentacion {
                    resultado: Some(dato as u16),
                    ..instruccion
                }
            }
            Instruccion::STORE { direccion_ram, .. } => {
                if let Some(valor) = instruccion.resultado {
                    memoria.escribir_byte(direccion_ram, valor as u8);
                }
                instruccion
            }
            _ => instruccion,
        }
    }

    /// Ejecuta la etapa WB (Write Back) usando el registro `mem_wb`.
    ///
    /// Si `mem_wb` esta activo y contiene un resultado, escribe el valor
    /// en el banco de registros segun el tipo de instruccion:
    /// - `ADD`, `SUB`, `LOAD`: escriben en el registro `dest`.
    ///   Las escrituras sobre R0 se descartan silenciosamente (hardwired-zero).
    /// - `STORE`, `NOP`: no escriben en registros.
    pub fn ejecutar_writeback(&mut self) {
        if !self.mem_wb.activa {
            return;
        }

        self.instrucciones_completadas += 1;

        if let Some(valor) = self.mem_wb.resultado {
            match self.mem_wb.instruccion {
                Instruccion::ADD { dest, .. }
                | Instruccion::SUB { dest, .. }
                | Instruccion::LOAD { dest, .. } => match dest {
                    // R0 es hardwired zero: las escrituras se descartan.
                    Registro::R0 => {}
                    Registro::R1 => self.registros[1] = valor,
                    Registro::R2 => self.registros[2] = valor,
                    Registro::R3 => self.registros[3] = valor,
                },
                _ => {} // STORE y NOP no escriben en registros
            }
        }
    }

    /// Avanza el pipeline un ciclo de reloj completo.
    ///
    /// Orden de evaluacion en cada ciclo (evita RAW ocultos):
    /// 1. WB  - escribe `mem_wb` en el banco de registros.
    /// 2. MEM - procesa `ex_mem` y genera el nuevo valor de `mem_wb`.
    /// 3. Deteccion de hazards - Load-Use y JUMP.
    /// 4. Avance de etapas:
    ///    - JUMP en EX: flushea IF/ID e ID/EX, redirige el PC.
    ///    - Load-Use stall: inserta burbuja en ID/EX, congela IF/ID y el PC.
    ///    - Normal: avanza todas las etapas y busca la proxima instruccion.
    /// 5. Actualiza `mem_wb` con el resultado de MEM.
    /// 6. Incrementa `contador_ciclos`.
    pub fn ciclo_reloj(&mut self, programa: &[Instruccion], memoria: &mut ControladorMemoria) {
        self.ejecutar_writeback();

        let nuevo_mem_wb = self.ejecutar_mem(self.ex_mem, memoria);

        let instruccion_en_id = self.if_id.instruccion;
        let hazard = self.if_id.activa && self.detectar_load_use_hazard(&instruccion_en_id);

        let jump_en_ex = if self.id_ex.activa {
            match self.id_ex.instruccion {
                Instruccion::JUMP { direccion_destino } => Some(direccion_destino),
                _ => None,
            }
        } else {
            None
        };

        let burbuja = RegistroSegmentacion {
            instruccion: Instruccion::NOP,
            activa: false,
            resultado: None,
        };

        if let Some(destino) = jump_en_ex {
            self.ex_mem = self.ejecutar_alu(self.id_ex);
            self.id_ex = burbuja;
            self.if_id = burbuja;
            self.program_counter = destino;
        } else if hazard {
            self.ex_mem = self.id_ex;
            self.id_ex = burbuja;
        } else {
            self.ex_mem = self.ejecutar_alu(self.id_ex);
            self.id_ex = self.if_id;

            if self.program_counter < programa.len() {
                self.if_id = RegistroSegmentacion {
                    instruccion: programa[self.program_counter],
                    activa: true,
                    resultado: None,
                };
                self.program_counter += 1;
            } else {
                self.if_id = burbuja;
            }
        }

        self.mem_wb = nuevo_mem_wb;
        self.contador_ciclos += 1;
    }
}

// ─── Display ─────────────────────────────────────────────────────────────────

/// Formatea una `Instruccion` como texto legible (ej: "ADD R1,R2,R3").
impl fmt::Display for Instruccion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruccion::NOP => write!(f, "NOP"),
            Instruccion::ADD { dest, src1, src2 } => {
                write!(f, "ADD {:?},{:?},{:?}", dest, src1, src2)
            }
            Instruccion::SUB { dest, src1, src2 } => {
                write!(f, "SUB {:?},{:?},{:?}", dest, src1, src2)
            }
            Instruccion::LOAD {
                dest,
                direccion_ram,
            } => {
                write!(f, "LOAD {:?},0x{:02X}", dest, direccion_ram)
            }
            Instruccion::STORE { src, direccion_ram } => {
                write!(f, "STORE {:?},0x{:02X}", src, direccion_ram)
            }
            Instruccion::JUMP { direccion_destino } => {
                write!(f, "JUMP 0x{:02X}", direccion_destino)
            }
        }
    }
}

/// Formatea un `RegistroSegmentacion`:
/// - Si esta activo, muestra la instruccion que contiene.
/// - Si es una burbuja (inactivo), muestra `"--"`.
impl fmt::Display for RegistroSegmentacion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.activa {
            write!(f, "{}", self.instruccion)
        } else {
            write!(f, "--")
        }
    }
}

/// Formatea el estado completo del pipeline de la CPU en una sola linea:
/// muestra el numero de ciclo y el contenido de cada registro de segmentacion.
impl fmt::Display for CpuSegmentada {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Ciclo {} | IF/ID: {} | ID/EX: {} | EX/MEM: {} | MEM/WB: {}",
            self.contador_ciclos, self.if_id, self.id_ex, self.ex_mem, self.mem_wb
        )
    }
}

impl Default for CpuSegmentada {
    fn default() -> Self {
        Self::nueva()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
