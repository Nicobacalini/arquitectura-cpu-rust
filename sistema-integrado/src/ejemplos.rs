//! Catálogo de programas de ejemplo para el simulador integrado.
//!
//! Cada ejemplo se define como un [`Ejemplo`] que agrupa:
//! - Nombre y descripción educativa del fenómeno que demuestra.
//! - Estado inicial de los registros y de la RAM.
//! - El programa como `Vec<Instruccion>`.
//!
//! Para agregar un nuevo ejemplo basta con:
//! 1. Escribir una función `fn programa_ejemplo_N() -> Vec<Instruccion>`.
//! 2. Añadir un `Ejemplo { ... }` al `Vec` retornado por [`catalogo`].

use cpu_pipeline::{Instruccion, Registro};

// ─── Tipo de datos ────────────────────────────────────────────────────────────

/// Metadatos y contenido de un programa de ejemplo.
pub struct Ejemplo {
    /// Nombre corto del ejemplo (se muestra en el encabezado).
    pub nombre: &'static str,
    /// Descripción educativa del fenómeno principal que demuestra.
    pub descripcion: &'static str,
    /// Valores iniciales de los registros `[R0, R1, R2, R3]`.
    pub registros_iniciales: [u16; 4],
    /// Valores precargados en la RAM antes de ejecutar: lista de `(dirección, valor)`.
    pub ram_inicial: &'static [(u8, u8)],
    /// Función que construye el programa de instrucciones.
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

// ─── Catálogo público ─────────────────────────────────────────────────────────

/// Retorna todos los ejemplos disponibles en orden de complejidad creciente.
///
/// Para agregar un nuevo ejemplo:
/// 1. Define `fn programa_ejemplo_N() -> Vec<Instruccion>` arriba.
/// 2. Agrega un `Ejemplo { ... }` al `vec![]` de esta función.
pub fn catalogo() -> Vec<Ejemplo> {
    vec![
        Ejemplo {
            nombre: "Load-Use Hazard + Cache Hit",
            descripcion: "Stalls por dependencia de datos (load-use) y reutilizacion de linea de cache.",
            registros_iniciales: [0, 0, 10, 0],
            ram_inicial: &[(0x10, 15)],
            programa: programa_ejemplo_1,
        },
        Ejemplo {
            nombre: "Aritmetica pura sin accesos a memoria",
            descripcion: "Pipeline de solo ALU: forwarding elimina stalls; CPI cercano a 1. Cache sin ejercitar.",
            registros_iniciales: [0, 10, 20, 0],
            ram_inicial: &[],
            programa: programa_ejemplo_2,
        },
        Ejemplo {
            nombre: "Salto incondicional (JUMP) con penalizacion de pipeline",
            descripcion: "Flush de 2 instrucciones especulativas; penalizacion de 2 ciclos por JUMP.",
            registros_iniciales: [0, 5, 3, 0],
            ram_inicial: &[],
            programa: programa_ejemplo_3,
        },
        Ejemplo {
            nombre: "Multiples accesos a memoria: patrones de Hit y Miss",
            descripcion: "Hits en el mismo bloque, writes y recargas mostrando politica write-back.",
            registros_iniciales: [0, 0, 0, 0],
            ram_inicial: &[(0x10, 5), (0x11, 8)],
            programa: programa_ejemplo_4,
        },
    ]
}
