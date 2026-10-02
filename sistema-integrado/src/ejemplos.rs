//! Catalogo de programas de ejemplo para el simulador integrado.
//!
//! Cada caso agrupa el nombre, la descripcion del fenomeno analizado,
//! el estado inicial de registros y RAM, el ASID de proceso, y la funcion
//! constructora de instrucciones.

use cpu_pipeline::{Instruccion, Registro};

/// Metadatos y contenido de un programa de ejemplo.
pub struct Ejemplo {
    /// Nombre corto del ejemplo mostrado en el encabezado.
    pub nombre: &'static str,
    /// Descripcion educativa del fenomeno principal demostrado.
    pub descripcion: &'static str,
    /// Valores iniciales para el banco de registros R0 a R3.
    pub registros_iniciales: [u16; 4],
    /// Valores precargados en RAM antes de iniciar la simulacion.
    pub ram_inicial: &'static [(u8, u8)],
    /// ASID del proceso simulado. Permite demostrar la separacion de traducciones.
    pub asid: u32,
    /// Funcion generadora del vector de instrucciones.
    pub programa: fn() -> Vec<Instruccion>,
}

// ─── Ejemplo 1: Load-Use Hazard + Cache Hit ───────────────────────────────────
//
// Estado inicial: R2=10, RAM[0x10]=15
// Fenómenos demostrados:
//   - Load-Use Hazard: LOAD R1 seguido inmediatamente de ADD que usa R1 → stall 1 ciclo
//   - Cache Miss seguido de Hit: primer acceso a 0x20 es miss (Write-Allocate),
//     re-lectura del mismo bloque es hit directo.
// Resultado esperado: R1=15, R2=25, R3=10

fn programa_ejemplo_1() -> Vec<Instruccion> {
    vec![
        // I0: Cargar RAM[0x10] (15) en R1
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        // I1: R2 = R1 + R2 (15 + 10 = 25) — Load-Use Hazard: stall 1 ciclo
        Instruccion::ADD {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I2: RAM[0x20] = R2 (25) — STORE con Write-Allocate
        Instruccion::STORE {
            src: Registro::R2,
            direccion_ram: 0x20,
        },
        // I3: R3 = RAM[0x20] (25) — Cache Hit: mismo bloque recien cargado
        Instruccion::LOAD {
            dest: Registro::R3,
            direccion_ram: 0x20,
        },
        // I4: R3 = R3 - R1 (25 - 15 = 10) — Load-Use Hazard adicional
        Instruccion::SUB {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R1,
        },
    ]
}

// ─── Ejemplo 2: Aritmética pura (sin accesos a memoria) ──────────────────────
//
// Estado inicial: R1=10, R2=20
// Fenómenos demostrados:
//   - Pipeline de solo ALU: forwarding elimina todos los stalls entre instrucciones aritméticas
//   - CPI ideal: muy cercano a 1 (pipeline completamente ocupado)
//   - La caché no se ejercita (sin LOAD/STORE)
// Resultado esperado: R1=30, R2=10, R3=50

fn programa_ejemplo_2() -> Vec<Instruccion> {
    vec![
        // I0: R3 = R1 + R2  (10 + 20 = 30)
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I1: R2 = R3 - R1  (30 - 10 = 20)
        Instruccion::SUB {
            dest: Registro::R2,
            src1: Registro::R3,
            src2: Registro::R1,
        },
        // I2: R1 = R2 + R1  (20 + 10 = 30)
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R2,
            src2: Registro::R1,
        },
        // I3: R3 = R3 + R2  (30 + 20 = 50)
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R2,
        },
        // I4: R2 = R1 - R2  (30 - 20 = 10)
        Instruccion::SUB {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R2,
        },
    ]
}

// ─── Ejemplo 3: Salto incondicional (JUMP) ────────────────────────────────────
//
// Estado inicial: R1=5, R2=3
// Fenómenos demostrados:
//   - JUMP resuelto en EX: flushea 2 instrucciones especulativas (I2, I3)
//   - Branch penalty de 2 ciclos: I2 e I3 aparecen en el trace pero son descartadas
//   - El pipeline retoma correctamente desde I4 (destino del salto)
//   - Instrucciones completadas = 4 (no 6), confirmando que I2 e I3 no se contabilizan
// Resultado esperado: R1=8, R3=8, RAM[0x30]=8

fn programa_ejemplo_3() -> Vec<Instruccion> {
    vec![
        // I0: R1 = R1 + R2  (5 + 3 = 8)
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I1: Salta al índice 4 — flushea I2 e I3 (branch penalty: 2 ciclos)
        Instruccion::JUMP {
            direccion_destino: 4,
        },
        // I2: (NUNCA ejecutada — descartada por flush del JUMP)
        Instruccion::ADD {
            dest: Registro::R2,
            src1: Registro::R2,
            src2: Registro::R1,
        },
        // I3: (NUNCA ejecutada — descartada por flush del JUMP)
        Instruccion::SUB {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R2,
        },
        // I4: R3 = R1 + R3  (8 + 0 = 8) ← primera instrucción post-salto
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R3,
        },
        // I5: RAM[0x30] = R3  (guarda 8)
        Instruccion::STORE {
            src: Registro::R3,
            direccion_ram: 0x30,
        },
    ]
}

// ─── Ejemplo 4: Múltiples accesos a memoria (patrones Hit/Miss) ──────────────
//
// Estado inicial: RAM[0x10]=5, RAM[0x11]=8
// Fenómenos demostrados:
//   - Miss en primer LOAD (bloque frío no cargado)
//   - Hit en segundo LOAD: 0x11 comparte bloque con 0x10 (localidad espacial)
//   - Hit en re-lectura post-STORE: Write-Back mantiene el dato en caché
//   - Dos Load-Use Hazards: I1→I2 y I4→I5
//   - Política Write-Back: flush al final vuelca líneas sucias a RAM
// Resultado esperado: R1=13, R2=5, R3=13, RAM[0x20]=13, RAM[0x21]=5

fn programa_ejemplo_4() -> Vec<Instruccion> {
    vec![
        // I0: R1 = RAM[0x10]  (5) — Miss de caché, carga el bloque completo
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        // I1: R2 = RAM[0x11]  (8) — Hit (0x11 esta en el mismo bloque que 0x10)
        Instruccion::LOAD {
            dest: Registro::R2,
            direccion_ram: 0x11,
        },
        // I2: R3 = R1 + R2  (5 + 8 = 13) — Load-Use Hazard desde I1
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I3: RAM[0x20] = R3  (guarda 13)
        Instruccion::STORE {
            src: Registro::R3,
            direccion_ram: 0x20,
        },
        // I4: R1 = RAM[0x20]  (13) — Hit (dato recien almacenado, aun en cache)
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x20,
        },
        // I5: R2 = R1 - R2  (13 - 8 = 5) — Load-Use Hazard desde I4
        Instruccion::SUB {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I6: RAM[0x21] = R2  (guarda 5)
        Instruccion::STORE {
            src: Registro::R2,
            direccion_ram: 0x21,
        },
    ]
}

// ─── Ejemplo 5 (Fase 2): Page Fault por acceso a más de 16 páginas distintas ─
//
// Fenomeno demostrado:
//   - Con 16 marcos fisicos y 256 paginas virtuales posibles, al acceder a
//     direcciones en 17 paginas distintas se producen page faults con reemplazo.
//   - Cada LOAD a una direccion de pagina N genera un page fault la primera vez
//     (la pagina no esta presente). A partir del 17mo acceso, la MMU debe desalojar
//     una pagina existente para hacer lugar (reemplazo LRU).
//   - El campo `mmu.page_faults` debera ser > 0 al terminar.
//
// Direcciones usadas: 0x0000, 0x0100, 0x0200, ..., 0x1000 (17 paginas, VPN 0..16).
// Resultado: R1 con el ultimo valor leido; mmu.page_faults >= 17.

fn programa_ejemplo_5() -> Vec<Instruccion> {
    // Accedemos a 17 paginas distintas (VPN 0 a 16)
    // Para forzar reemplazo: al acceder a la pagina 16, los 16 marcos ya estan ocupados.
    vec![
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0000 }, // VPN=0
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0100 }, // VPN=1
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0200 }, // VPN=2
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0300 }, // VPN=3
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0400 }, // VPN=4
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0500 }, // VPN=5
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0600 }, // VPN=6
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0700 }, // VPN=7
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0800 }, // VPN=8
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0900 }, // VPN=9
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0A00 }, // VPN=10
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0B00 }, // VPN=11
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0C00 }, // VPN=12
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0D00 }, // VPN=13
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0E00 }, // VPN=14
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x0F00 }, // VPN=15
        Instruccion::LOAD { dest: Registro::R1, direccion_ram: 0x1000 }, // VPN=16 → reemplazo!
    ]
}

// ─── Ejemplo 6 (Fase 2): Demo ASID — traducciones separadas por proceso ───────
//
// Fenomeno demostrado:
//   - Dos "procesos" simulados bajo ASID 0 y ASID 1 respectivamente.
//   - El Ejemplo 6A se ejecuta bajo ASID 0, escribe en la pagina VPN=1 (0x0100).
//   - El Ejemplo 6B se ejecuta bajo ASID 1 (cambio de contexto), accede a la
//     misma pagina virtual VPN=1 (0x0100) — la TLB no mezcla sus traducciones.
//   - Al volver a ASID 0 y releer 0x0100, se obtiene el valor del proceso A.
//
// Este ejemplo se ejecuta directamente desde main (no a traves de ejecutar_ejemplo)
// para poder cambiar el ASID entre corridas. Aqui se define solo como referencia.
//
// Para el catalogo, incluimos solo el programa A como Ejemplo 6 con ASID=0:

fn programa_ejemplo_6() -> Vec<Instruccion> {
    vec![
        // Proceso A (ASID=0): escribe 42 en direccion virtual 0x0100 (VPN=1, offset=0)
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R0,
            src2: Registro::R0,
        }, // R1 = 0
        Instruccion::STORE {
            src: Registro::R2,  // R2 sera inicializado a 42
            direccion_ram: 0x0100,
        },
        Instruccion::LOAD {
            dest: Registro::R3,
            direccion_ram: 0x0100,
        },
    ]
}

/// Retorna todos los ejemplos disponibles en orden de complejidad creciente.
pub fn catalogo() -> Vec<Ejemplo> {
    vec![
        Ejemplo {
            nombre: "Load-Use Hazard + Cache Hit",
            descripcion: "Stalls por dependencia de datos (load-use) y reutilizacion de linea de cache.",
            registros_iniciales: [0, 0, 10, 0],
            ram_inicial: &[(0x10, 15)],
            asid: 0,
            programa: programa_ejemplo_1,
        },
        Ejemplo {
            nombre: "Aritmetica pura sin accesos a memoria",
            descripcion: "Pipeline de solo ALU: forwarding elimina stalls; CPI cercano a 1. Cache sin ejercitar.",
            registros_iniciales: [0, 10, 20, 0],
            ram_inicial: &[],
            asid: 0,
            programa: programa_ejemplo_2,
        },
        Ejemplo {
            nombre: "Salto incondicional (JUMP) con penalizacion de pipeline",
            descripcion: "Flush de 2 instrucciones especulativas; penalizacion de 2 ciclos por JUMP.",
            registros_iniciales: [0, 5, 3, 0],
            ram_inicial: &[],
            asid: 0,
            programa: programa_ejemplo_3,
        },
        Ejemplo {
            nombre: "Multiples accesos a memoria: patrones de Hit y Miss",
            descripcion: "Hits en el mismo bloque, writes y recargas mostrando politica write-back.",
            registros_iniciales: [0, 0, 0, 0],
            ram_inicial: &[(0x10, 5), (0x11, 8)],
            asid: 0,
            programa: programa_ejemplo_4,
        },
        Ejemplo {
            nombre: "[Fase 2] Page Faults con reemplazo LRU de marcos fisicos",
            descripcion: "17 LOADs a 17 paginas distintas: los primeros 16 llenan los marcos fisicos, \
                          el 17mo fuerza un reemplazo LRU. mmu.page_faults >= 17 al finalizar.",
            registros_iniciales: [0, 0, 0, 0],
            ram_inicial: &[],
            asid: 0,
            programa: programa_ejemplo_5,
        },
        Ejemplo {
            nombre: "[Fase 2] ASID: separacion de traducciones entre procesos",
            descripcion: "Proceso A (ASID=0) escribe en VPN=1 y lo vuelve a leer. \
                          La TLB registra la traduccion bajo ASID 0. \
                          Un cambio a ASID 1 fuerza un TLB miss en la misma VPN (traducciones separadas).",
            registros_iniciales: [0, 0, 42, 0],
            ram_inicial: &[],
            asid: 0,
            programa: programa_ejemplo_6,
        },
    ]
}
