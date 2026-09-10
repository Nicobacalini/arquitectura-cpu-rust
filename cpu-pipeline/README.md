# cpu-pipeline

> Simulacion de una CPU de 5 etapas con pipeline, forwarding y deteccion de hazards, implementada en Rust.

---

## Tabla de Contenidos

- [Estructura del Crate](#estructura-del-crate)
- [Estructuras de Datos](#estructuras-de-datos)
- [Funciones Publicas](#funciones-publicas)
  - [1. detectar\_load\_use\_hazard](#1-detectar_load_use_hazard)
  - [2. calcular\_forwarding](#2-calcular_forwarding)
  - [3. resolver\_operando](#3-resolver_operando)
  - [4. ejecutar\_alu](#4-ejecutar_alu)
  - [5. ejecutar\_mem](#5-ejecutar_mem)
  - [6. ejecutar\_writeback](#6-ejecutar_writeback)
  - [7. ciclo\_reloj](#7-ciclo_reloj)
  - [8. Trait Display](#8-trait-display)
- [Diagrama Temporal: Load-Use Hazard con Stall y Forwarding](#diagrama-temporal-load-use-hazard-con-stall-y-forwarding)
- [Diagrama de Flujo de ciclo\_reloj](#diagrama-de-flujo-de-ciclo_reloj)
- [Suite de Tests](#suite-de-tests)

---

## Estructura del Crate

```
cpu-pipeline/
├── Cargo.toml
└── src/
    ├── lib.rs      # API publica: tipos, pipeline, hazard detection, forwarding
    ├── main.rs     # Binario de demo: ejecuta un programa de ejemplo
    └── tests.rs    # Suite completa de tests unitarios e integracion
```

| Archivo | Rol |
|---|---|
| `lib.rs` | Define todas las estructuras (`MemoriaProvisoria`, `Instruccion`, `CpuSegmentada`, etc.) y la logica del pipeline |
| `main.rs` | Instancia una CPU con registros de ejemplo y muestra el estado ciclo a ciclo |
| `tests.rs` | 36 tests: forwarding, load-use hazard, JUMP, R0 hardwired-zero, Display, aritmetica modular |

---

## Estructuras de Datos

### `MemoriaProvisoria`

Memoria RAM simulada de **256 bytes** direccionables con un indice `u8` (`0x00`..`0xFF`).
En una implementacion completa seria reemplazada por un controlador de cache.

```rust
pub struct MemoriaProvisoria {
    pub ram: [u8; 256],
}
```

| Metodo | Descripcion |
|---|---|
| `new()` | Inicializa toda la RAM en `0` |
| `leer_byte(dir)` | Lee el byte en la direccion `dir` |
| `escribir_byte(dir, dato)` | Escribe `dato` en la direccion `dir` |

---

### `Registro`

Identificador de los cuatro registros generales de la CPU.

```rust
pub enum Registro { R0, R1, R2, R3 }
```

> **R0 es hardwired-zero**: siempre vale `0`. Las escrituras sobre el son silenciosamente descartadas en WB y el forwarding nunca lo anticipa.

---

### `Instruccion`

Conjunto de instrucciones (ISA) soportadas por la CPU:

| Instruccion | Operandos | Descripcion |
|---|---|---|
| `NOP` | — | Sin operacion (burbuja) |
| `ADD` | `dest, src1, src2` | `dest = src1 + src2` (wrapping u16) |
| `SUB` | `dest, src1, src2` | `dest = src1 - src2` (wrapping u16) |
| `LOAD` | `dest, dir_ram` | `dest = RAM[dir_ram]` |
| `STORE` | `src, dir_ram` | `RAM[dir_ram] = src` |
| `JUMP` | `dir_destino` | Salta incondicionalmente a `dir_destino`; flushea el pipeline |

---

### `RegistroSegmentacion`

Buffer fisico entre dos etapas del pipeline. Cada uno de los cuatro registros del pipeline (`IF/ID`, `ID/EX`, `EX/MEM`, `MEM/WB`) es una instancia de esta estructura.

```rust
pub struct RegistroSegmentacion {
    pub instruccion: Instruccion,  // Instruccion en transito
    pub activa:      bool,          // false = burbuja (NOP inactivo)
    pub resultado:   Option<u16>,   // None hasta que la etapa calcula el valor
}
```

El campo `resultado: Option<u16>` es clave para el forwarding: un `LOAD` en `EX/MEM` tiene `resultado = None` hasta que termina la etapa `MEM`, lo que impide anticipar datos inexistentes.

---

### `CpuSegmentada`

Estado global del procesador. Contiene los cuatro registros de segmentacion, el banco de registros, el PC y el contador de ciclos.

```rust
pub struct CpuSegmentada {
    pub if_id:           RegistroSegmentacion, // Fetch -> Decode
    pub id_ex:           RegistroSegmentacion, // Decode -> Execute
    pub ex_mem:          RegistroSegmentacion, // Execute -> Memory
    pub mem_wb:          RegistroSegmentacion, // Memory -> Write Back
    pub registros:       [u16; 4],             // Banco R0..R3
    pub program_counter: usize,                // Indice de la prox. instruccion
    pub contador_ciclos: u64,                  // Ciclos transcurridos
}
```

---

## Funciones Publicas

### 1. `detectar_load_use_hazard`

```rust
pub fn detectar_load_use_hazard(&self, instruccion_en_id: &Instruccion) -> bool
```

#### Por que existe este hazard?

El forwarding resuelve la mayoria de los riesgos de datos, pero **no todos**. Una instruccion `ADD` produce su resultado al final de **EX**. Un `LOAD`, en cambio, obtiene el dato al final de **MEM** — un ciclo mas tarde. Si la instruccion inmediatamente siguiente necesita ese valor en su etapa **EX**, hay un conflicto insalvable: el dato no existe a tiempo para ser anticipado.

```
         LOAD  |  IF  |  ID  |  EX  |  MEM* |  WB  |
         ADD   |      |  IF  |  ID  |  EX*  |  MEM  |  WB  |
                                       ^        ^
                                ADD necesita  LOAD produce
                                R1 aqui       R1 aqui (MEM)
```

La unica solucion es insertar un **stall de 1 ciclo** (burbuja `NOP` en `id_ex`) y congelar `if_id` y el `PC`.

#### Logica interna

1. Verifica que `id_ex` este activo y contenga un `LOAD { dest: reg_load }`.
2. Inspecciona la instruccion en `ID`:
   - `ADD` / `SUB`: hazard si `src1 == reg_load` o `src2 == reg_load`.
   - `STORE`: hazard si `src == reg_load`.
   - `NOP`, `LOAD`, `JUMP`: no hay conflicto → retorna `false`.

---

### 2. `calcular_forwarding`

```rust
pub fn calcular_forwarding(&self, src: Registro) -> Option<u16>
```

#### Teoria

El **Forwarding (Bypassing)** conecta directamente la salida de etapas posteriores a la entrada de la ALU, evitando leer un valor desactualizado del banco de registros.

```
         +─────────── Forwarding MEM/WB ───────────────────+
         |                                                  |
         +── Forwarding EX/MEM ──+                          |
         |                       |                          v
[IF] -> [if_id] -> [ID] -> [id_ex] -> [ALU (EX)] -> [ex_mem] -> [MEM] -> [mem_wb] -> [WB]
```

#### Prioridad

| Prioridad | Fuente | Condicion |
|---|---|---|
| **1 (maxima)** | `EX/MEM` | activo, `dest == src`, `resultado == Some(_)` |
| **2** | `MEM/WB` | activo, `dest == src`, `resultado == Some(_)` |
| **Sin forwarding** | Banco de registros | Ninguna etapa produce `src` |

#### Casos especiales

- **R0 hardwired-zero**: retorna `None` inmediatamente. El banco garantiza `registros[0] == 0`.
- **LOAD en EX/MEM**: su `resultado` es `None` en esa etapa (aun no leyo la RAM), por lo que la funcion retorna `None` sin caer a `MEM/WB`. Esto previene anticipar datos inexistentes.

---

### 3. `resolver_operando`

```rust
pub fn resolver_operando(&self, src: Registro) -> u16
```

Implementa el **MUX** a la entrada de la ALU:

```
calcular_forwarding(src) == Some(v)  →  devuelve v   (forwarding)
calcular_forwarding(src) == None     →  devuelve registros[src]  (banco)
```

---

### 4. `ejecutar_alu`

```rust
pub fn ejecutar_alu(&self, instruccion: RegistroSegmentacion) -> RegistroSegmentacion
```

Ejecuta la etapa **EX (Execute)** y produce el `RegistroSegmentacion` listo para `EX/MEM`:

| Instruccion | `resultado` producido |
|---|---|
| `ADD` | `Some(op1.wrapping_add(op2))` — sin panics en overflow |
| `SUB` | `Some(op1.wrapping_sub(op2))` — comportamiento de silicio real |
| `LOAD` | `None` — el dato aun no esta disponible; MEM lo completara |
| `STORE` | `Some(resolver_operando(src))` — empaqueta el valor para transportarlo a MEM |
| `NOP` / inactivo | burbuja limpia (`activa: false, resultado: None`) |

> **El "vagon de carga" del STORE**: como STORE no produce un resultado para WB, la ALU empaqueta el valor del registro fuente en `resultado` para que la etapa MEM lo encuentre al llegar.

---

### 5. `ejecutar_mem`

```rust
pub fn ejecutar_mem(
    &self,
    instruccion: RegistroSegmentacion,
    memoria: &mut MemoriaProvisoria,
) -> RegistroSegmentacion
```

Ejecuta la etapa **MEM (Memory Access)**. Es la unica etapa autorizada para acceder a la RAM:

| Instruccion | Accion |
|---|---|
| `LOAD` | Lee `RAM[dir_ram]` → completa `resultado = Some(byte as u16)` |
| `STORE` | Escribe `resultado` (empaquetado en EX) → `RAM[dir_ram] = valor as u8` |
| Resto | Pasa sin modificaciones hacia `MEM/WB` |

---

### 6. `ejecutar_writeback`

```rust
pub fn ejecutar_writeback(&mut self)
```

Ejecuta la etapa **WB (Write Back)** usando el registro `mem_wb`:

- `ADD`, `SUB`, `LOAD` con `dest != R0`: escribe `resultado` en `registros[dest]`.
- **R0**: la escritura se descarta silenciosamente (hardwired-zero).
- `STORE`, `NOP`, registro inactivo: no modifica el banco.

---

### 7. `ciclo_reloj`

```rust
pub fn ciclo_reloj(&mut self, programa: &[Instruccion], memoria: &mut MemoriaProvisoria)
```

Orquesta un ciclo de reloj completo. Modela el **flanco ascendente de reloj** del hardware.

#### Por que el orden inverso?

En silicio, las 5 etapas operan en **paralelo**. En software (monohilo), deben ejecutarse en secuencia. Si avanzaramos de IF a WB, al actualizar `if_id` en el paso 1, la etapa ID leeria la instruccion del ciclo actual en vez de la del ciclo anterior — un *data race* de simulacion.

La solucion es procesar en **orden inverso a la ruta de datos** (WB → MEM → ... → IF), garantizando que cada etapa lee el estado estable del ciclo anterior antes de que las etapas tempranas lo sobreescriban.

#### Orden de evaluacion por ciclo

```
1. WB     →  ejecutar_writeback()           consolida mem_wb en el banco de registros
2. MEM    →  nuevo_mem_wb = ejecutar_mem()  resultado guardado en variable temporal
3. Hazard →  detectar_load_use_hazard()     inspecciona if_id vs id_ex
4. JUMP   →  inspecciona id_ex              guarda el destino si hay salto
             ╔══════════════════════════════════╗
5. Branch  ║  JUMP > Stall > Normal           ║
             ╠══════════════════════════════════╣
             ║ JUMP:   flush if_id + id_ex      ║
             ║         PC = dir_destino         ║
             ╠──────────────────────────────────╣
             ║ STALL:  ex_mem = id_ex (sin ALU) ║
             ║         id_ex  = burbuja         ║
             ║         if_id y PC congelados    ║
             ╠──────────────────────────────────╣
             ║ NORMAL: ex_mem = ALU(id_ex)      ║
             ║         id_ex  = if_id           ║
             ║         if_id  = Fetch(PC)       ║
             ║         PC    += 1               ║
             ╚══════════════════════════════════╝
6. mem_wb = nuevo_mem_wb
7. contador_ciclos += 1
```

#### Mecanica del JUMP — Branch Penalty

El `JUMP` se resuelve cuando llega a **EX** (en `id_ex`). Para ese momento el pipeline ya busco 2 instrucciones incorrectas:

```
Ciclo N-2:  JUMP  en IF
Ciclo N-1:  JUMP  en ID  |  inst_A  en IF   <- incorrecta
Ciclo N:    JUMP  en EX  |  inst_A  en ID  |  inst_B  en IF  <- incorrecta
                 ^
          Se detecta aqui:
            flush id_ex (inst_A) + flush if_id (inst_B) + PC = destino
```

**Penalidad: 2 ciclos** desperdiciados. Esta implementacion no incluye prediccion de saltos.

#### Drenado del Pipeline (Pipeline Draining)

Cuando `PC >= len(programa)`, la etapa IF inyecta burbujas (`NOP` inactivos). Las instrucciones legitimas en transito completan su ciclo de vida y consolidan sus escrituras de forma limpia.

---

### 8. Trait Display

Cada tipo implementa `fmt::Display` siguiendo la convencion idiomatica de Rust:

| Tipo | Salida de ejemplo |
|---|---|
| `Instruccion::ADD{R1,R2,R3}` | `ADD R1,R2,R3` |
| `Instruccion::LOAD{R1, 0x2A}` | `LOAD R1,0x2A` |
| `Instruccion::JUMP{5}` | `JUMP 0x05` |
| `RegistroSegmentacion` (activo) | muestra la instruccion |
| `RegistroSegmentacion` (inactivo) | `--` |
| `CpuSegmentada` | `Ciclo N \| IF/ID: X \| ID/EX: Y \| EX/MEM: Z \| MEM/WB: W` |

---

## Diagrama Temporal: Load-Use Hazard con Stall y Forwarding

**Programa de ejemplo:**
```assembly
LOAD  R1, 0x10   ; R1 = RAM[0x10] = 15
ADD   R2, R1, R0 ; R2 = R1 + 0    = 15  (necesita R1 -> Load-Use Hazard)
```

La tabla muestra el estado de los registros de segmentacion **al final de cada ciclo** (lo que imprime `Display`). La columna "Evento" describe lo ocurrido durante ese ciclo.

| Ciclo | IF/ID | ID/EX | EX/MEM | MEM/WB | Evento |
|:---:|:---:|:---:|:---:|:---:|---|
| **1** | `LOAD` | `--` | `--` | `--` | Fetch de `LOAD`. |
| **2** | `ADD` | `LOAD` | `--` | `--` | Fetch de `ADD`. `LOAD` pasa a decode. |
| **3** | `ADD` ❄️ | `--` | `LOAD` | `--` | **Load-Use Hazard detectado.** Burbuja insertada en ID/EX. IF/ID y PC congelados. `LOAD` avanza a EX/MEM sin pasar por la ALU. |
| **4** | `--` | `ADD` | `--` | `LOAD(15)` | Stall liberado. `LOAD` termina en MEM: lee `RAM[0x10]=15`. `ADD` descongelado, pasa a ID/EX. |
| **5** | `--` | `--` | `ADD(15)` | `--` | **WB** escribe `R1=15`. **Forwarding MEM/WB→EX**: `resolver_operando(R1)` anticipa 15 desde `mem_wb`. `ADD` calcula `15+0=15`. |
| **6** | `--` | `--` | `--` | `ADD(15)` | MEM pass-through de `ADD` (no accede a RAM). |
| **7** | `--` | `--` | `--` | `--` | **WB** escribe `R2=15`. Pipeline drenado. ✅ |

> **Nota sobre la columna ID/EX en el Ciclo 3**: el README anterior indicaba incorrectamente `ADD`❄️ en esa posicion. El codigo inserta una **burbuja NOP** en `id_ex` durante el stall; `ADD` permanece congelado en `if_id`, no en `id_ex`.

---

## Diagrama de Flujo de `ciclo_reloj`

```
                    ┌─────────────────────────────────┐
                    │         ciclo_reloj()            │
                    └────────────────┬────────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │   1. ejecutar_writeback │  <-- mem_wb -> registros
                         └───────────┬───────────┘
                                     │
                    ┌────────────────▼───────────────┐
                    │  2. nuevo_mem_wb = ejecutar_mem │  <-- ex_mem -> RAM
                    └────────────────┬───────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │  3. Detectar hazard    │  <-- if_id vs id_ex
                         └───────────┬───────────┘
                                     │
                         ┌───────────▼───────────┐
                         │  4. Detectar JUMP      │  <-- id_ex
                         └───────────┬───────────┘
                                     │
              ┌──────────────────────┼───────────────────────┐
              │                      │                        │
        JUMP en EX             STALL (hazard)           NORMAL
              │                      │                        │
   ex_mem=ALU(id_ex)       ex_mem=id_ex (directo)   ex_mem=ALU(id_ex)
   id_ex=burbuja            id_ex=burbuja            id_ex=if_id
   if_id=burbuja            (if_id y PC congelados)  if_id=Fetch(PC)
   PC=dir_destino                                    PC+=1
              │                      │                        │
              └──────────────────────┼───────────────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │  6. mem_wb = nuevo     │
                         └───────────┬───────────┘
                                     │
                         ┌───────────▼───────────┐
                         │  7. contador_ciclos++  │
                         └───────────────────────┘
```

---

## Suite de Tests

Todos los tests viven en [`src/tests.rs`](src/tests.rs). Se ejecutan con:

```bash
cargo test --package cpu-pipeline
```

| Categoria | Tests | Que verifican |
|---|---|---|
| **Forwarding unitario** | 5 | `calcular_forwarding`: sin pipeline, R0→None, EX/MEM activo, MEM/WB activo, LOAD en EX/MEM sin caer a MEM/WB |
| **Prioridad de forwarding** | 1 | EX/MEM tiene prioridad absoluta sobre MEM/WB |
| **Forwarding integrado** | 3 | EX/MEM→EX, MEM/WB→EX, ambos operandos src1+src2 simultaneous |
| **Hazard detection unitario** | 6 | LOAD→ADD detecta, LOAD→SUB detecta, LOAD→LOAD no, LOAD→JUMP no, id_ex inactivo no, ADD en id_ex no |
| **Load-Use con stall** | 3 | ADD despues de LOAD, SUB despues de LOAD, STORE despues de LOAD |
| **Control hazard (JUMP)** | 2 | Flush de instrucciones especulativas, reset de PC |
| **R0 hardwired-zero** | 2 | Forwarding nunca anticipa R0, LOAD y SUB no modifican R0 |
| **Memoria** | 2 | Lectura/escritura, estado inicial en cero |
| **Aritmetica** | 2 | ADD+SUB sin hazards, overflow wrapping u16 |
| **LOAD/STORE** | 2 | Round-trip STORE→LOAD, STORE con forwarding desde EX |
| **Pipeline NOP / vacio** | 2 | NOPs no modifican registros, programa vacio es estable |
| **Contador de ciclos** | 1 | Avanza exactamente 1 por ciclo |
| **Display** | 4 | Formato de cada instruccion, registro activo vs inactivo, CPU con ciclo 0, CPU con ciclo N |

**Resultado:** `36 passed; 0 failed` ✅
