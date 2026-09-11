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
- [Decisiones de Diseño y su Justificación](#decisiones-de-diseño-y-su-justificación)
  - [5.1 Evaluación en Orden Inverso del Pipeline](#51-evaluación-en-orden-inverso-del-pipeline)
  - [5.2 Por qué el Load-Use Hazard No Se Resuelve con Forwarding](#52-por-qué-el-load-use-hazard-no-se-resuelve-con-forwarding)
  - [5.3 Por qué R0 se Filtra También en el Forwarding](#53-por-qué-r0-se-filtra-también-en-el-forwarding)
  - [5.4 Prioridad de Forwarding: EX/MEM sobre MEM/WB](#54-prioridad-de-forwarding-exmem-sobre-memwb)
  - [5.5 Un Solo Tipo para las Cuatro Etapas](#55-un-solo-tipo-para-las-cuatro-etapas)
  - [5.6 Ausencia de Predicción de Saltos](#56-ausencia-de-predicción-de-saltos)
  - [5.7 Aritmética Wrapping en vez de Checked](#57-aritmética-wrapping-en-vez-de-checked)
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

## Decisiones de Diseño y su Justificación

Esta sección documenta **por qué** el pipeline está diseñado como está, no solo **qué** hace — incluyendo un bug real que se encontró y corrigió durante el desarrollo (5.3).

### 5.1 Evaluación en Orden Inverso del Pipeline

**Decisión:** `ciclo_reloj` procesa las etapas en orden `WB → MEM → hazard/JUMP → EX → ID → IF`, nunca en el orden natural del datapath.

**Por qué:** en silicio, las 5 etapas leen y escriben sus registros de segmentación **simultáneamente**, en el mismo flanco de reloj. En una simulación de un solo hilo, las asignaciones ocurren una detrás de otra — si procesáramos en orden natural, una etapa temprana sobreescribiría un buffer antes de que la etapa siguiente leyera su valor *del ciclo anterior*.

```
❌ ORDEN NATURAL (INCORRECTO)              ✅ ORDEN INVERSO (CORRECTO)
────────────────────────────              ──────────────────────────
1. if_id = Fetch(PC)     ← escribe         1. WB   lee mem_wb  (viejo)
2. id_ex = if_id         ← YA CONTAMINADO  2. MEM  lee ex_mem  (viejo)
   (if_id es el NUEVO,     con el fetch    3. hazard/JUMP: lee if_id, id_ex (viejos)
    no el de este ciclo)   de este mismo   4. ex_mem = ALU(id_ex)     ← recién ahora
3. ex_mem = ALU(id_ex)     ciclo           5. id_ex  = if_id
4. mem_wb = ex_mem       ← arrastra el     6. if_id  = Fetch(PC)     ← al final
   dato ya "adelantado"    error en cadena 7. mem_wb = nuevo_mem_wb
```

**Costo aceptado:** el código es menos "legible en orden de datapath" (uno esperaría leer IF primero) — se compensa documentando el orden explícitamente en cada `ciclo_reloj`, como ya hace este README.

### 5.2 Por qué el Load-Use Hazard No Se Resuelve con Forwarding

**Decisión:** todos los riesgos de datos entre instrucciones aritméticas (`ADD`/`SUB` dependiendo de `ADD`/`SUB`) se resuelven con forwarding, sin frenar el pipeline — pero un `LOAD` seguido inmediatamente de una instrucción que lo usa **siempre** requiere 1 ciclo de stall, sin excepción.

**Por qué:** la diferencia está en *cuándo* cada instrucción produce su dato:

```
CASO RESOLUBLE (ADD → ADD)                CASO NO RESOLUBLE (LOAD → ADD)
───────────────────────────               ──────────────────────────────
ADD |IF|ID|EX*|MEM|WB |                   LOAD |IF|ID|EX|MEM*|WB |
ADD    |IF|ID|EX |MEM|WB|                 ADD      |IF|ID|EX*|MEM|WB|
              ▲                                            ▲    ▲
       dato listo aquí                          ADD necesita   LOAD produce
       (fin de EX)                              el dato aquí   el dato aquí
                                                 (inicio de EX) (fin de MEM)
       ADD (consumidor) entra a EX
       en el MISMO ciclo en que el
       productor SALE de EX → el
       dato ya está en ex_mem,
       disponible para forwarding.
```

Un `ADD` productor termina su cálculo al **final de EX**. La siguiente instrucción entra a EX un ciclo después — exactamente cuando ese resultado ya está disponible en el buffer `EX/MEM`. El forwarding solo necesita "mirar un buffer que ya tiene el dato".

Un `LOAD`, en cambio, no tiene el dato hasta el **final de MEM** — un ciclo *más tarde* que un `ADD`. Si la instrucción siguiente entra a EX inmediatamente, el dato todavía no existe en ningún buffer del pipeline: no hay nada que "anticipar", literalmente no fue calculado todavía. La única solución física es esperar ese ciclo — de ahí el stall obligatorio.

**Costo aceptado:** cada `LOAD` seguido de un consumidor inmediato cuesta 1 ciclo extra de CPI. Compiladores reales mitigan esto reordenando instrucciones independientes para "rellenar" ese hueco (*instruction scheduling*) — está fuera del alcance de este simulador, que ejecuta el programa en el orden dado.

### 5.3 Por qué R0 se Filtra También en el Forwarding

**Contexto:** `ejecutar_writeback` descarta silenciosamente cualquier escritura a `R0` (hardwired-zero, como `$zero`/`x0` en MIPS/RISC-V). Este filtro **por sí solo no alcanza** — y de hecho, la primera versión de este pipeline tenía un bug real por esta razón.

```
❌ BUG (solo protegido en WB)                ✅ CORREGIDO (protegido también en forwarding)
──────────────────────────────               ──────────────────────────────────────────────
ADD R0, R1, R2   ; calcula 7                 ADD R0, R1, R2   ; calcula 7
  → resultado=Some(7) viaja por el pipe        → resultado=Some(7) viaja igual
ADD R3, R0, R0   ; necesita "R0"             ADD R3, R0, R0   ; necesita "R0"
  │                                             │
  ▼ resolver_operando(R0)                       ▼ resolver_operando(R0)
  │                                             │
  ▼ calcular_forwarding(R0)                     ▼ calcular_forwarding(R0)
  │  ex_mem.dest == R0? SÍ                      │  if src == R0 { return None } ← CORTA ACÁ
  │  → return Some(7)   ← 🐛 "fantasma"         │  nunca llega a mirar ex_mem/mem_wb
  ▼                                             ▼
  R3 = 7 + 7 = 14   ❌ INCORRECTO               R3 = 0 + 0 = 0   ✅ CORRECTO
  (viola el invariante "R0 siempre es 0")       (R0 sigue siendo hardwired-zero)
```

**Por qué pasa:** `ejecutar_writeback` protege *el banco de registros*, pero el forwarding lee directamente de los **buffers del pipeline** (`ex_mem`/`mem_wb`), sin pasar nunca por el banco. Una instrucción con `dest == R0` sigue teniendo un `resultado: Some(valor)` perfectamente válido viajando por el pipeline — ese valor simplemente nunca debía ser *visible*, y el forwarding no tenía ninguna razón para saberlo hasta que se agregó el chequeo explícito.

**Solución:** cortar en el primer paso de `calcular_forwarding` (`if src == Registro::R0 { return None; }`), antes de mirar cualquier buffer — la fuente más simple de verdad posible: "si estoy preguntando por R0, la respuesta es siempre 0, sin importar qué esté viajando por el pipeline". Verificado en el test `forwarding_no_anticipa_r0`.

### 5.4 Prioridad de Forwarding: EX/MEM sobre MEM/WB

**Decisión:** cuando dos etapas distintas podrían anticipar el mismo registro, gana siempre `EX/MEM` sobre `MEM/WB`.

**Por qué:** `EX/MEM` contiene el resultado de la instrucción que se ejecutó **más recientemente**; `MEM/WB` contiene el de una instrucción un ciclo más vieja. Si ambas escriben el mismo registro (algo posible con 3 instrucciones consecutivas dependientes en cadena), el dato de `EX/MEM` es el que refleja el estado *correcto y más actual* del programa — usar el de `MEM/WB` en su lugar leería un valor obsoleto.

```
              MUX de Forwarding (selector de prioridad)
                           ┌───────────────┐
     EX/MEM (más nuevo) ──►│               │
                           │   Prioridad   │──► resolver_operando(src)
     MEM/WB (más viejo) ──►│   1 > 2 > 3   │
                           │               │
     Banco de registros ──►│               │
     (ningún forwarding)   └───────────────┘

Ejemplo con 3 instrucciones en cadena (sin ningún LOAD de por medio → sin stalls):

ADD R1, R2, R3     ; ciclo 1: entra a EX
ADD R2, R1, R0     ; ciclo 2: entra a EX, necesita R1 → EX/MEM tiene el ADD anterior (prioridad 1)
SUB R3, R2, R1     ; ciclo 3: entra a EX, necesita R2 → EX/MEM tiene el 2do ADD (prioridad 1, no MEM/WB)
```

**Costo aceptado:** ninguno real — es la única prioridad físicamente correcta. La alternativa (priorizar `MEM/WB`) directamente produciría resultados incorrectos, no es una decisión de trade-off sino de corrección.

### 5.5 Un Solo Tipo para las Cuatro Etapas

**Decisión:** `RegistroSegmentacion` es un único struct genérico reutilizado para los 4 buffers (`if_id`, `id_ex`, `ex_mem`, `mem_wb`), en vez de 4 tipos distintos (`BufferIfId`, `BufferIdEx`, etc.) cada uno con los campos específicos que esa transición necesita.

```
❌ Alternativa: un tipo por etapa           ✅ Elegido: un tipo uniforme
──────────────────────────────             ─────────────────────────────
struct BufferIfId { instr: Instruccion }   struct RegistroSegmentacion {
struct BufferIdEx { instr: Instruccion }       instruccion: Instruccion,
struct BufferExMem {                           activa: bool,
    instr: Instruccion,                        resultado: Option<u16>,
    resultado: Option<u16>,                }
}                                           // el mismo tipo sirve para las 4
struct BufferMemWb { ... }                 // posiciones del pipeline
```

**Por qué:** con un tipo uniforme, avanzar el pipeline es una simple asignación de structs (`self.ex_mem = self.id_ex;`), sin conversiones entre tipos ni lógica condicional. El campo `resultado: Option<u16>` está "de más" para `if_id`/`id_ex` (donde siempre vale `None`), pero ese pequeño desperdicio de memoria es insignificante comparado con la simplicidad de tener un solo tipo, una sola implementación de `Display`, y poder escribir `let burbuja = RegistroSegmentacion { .. }` una vez y reutilizarla en los 4 lugares.

### 5.6 Ausencia de Predicción de Saltos

**Decisión:** `JUMP` siempre asume, implícitamente, que se va a tomar — no hay lógica de "predicción" propiamente dicha, simplemente se resuelve en EX y se paga la penalidad de 2 ciclos siempre.

```
Con predicción "no tomado" (no implementado)     Sin predicción (implementado)
─────────────────────────────────────────────    ──────────────────────────────
Si predicción correcta: 0 ciclos perdidos         SIEMPRE 2 ciclos perdidos
Si predicción incorrecta: 2 ciclos perdidos        (flush de if_id + id_ex,
(requiere lógica adicional de checkpoint/          sin importar si "hubiera"
 rollback más allá del flush simple)               convenido predecir distinto)
```

**Por qué:** implementar predicción real requeriría además contabilizar aciertos/fallos de predicción y un mecanismo de rollback más elaborado (ver Tarea 3.5 del TP, marcada como extensión opcional). Para el alcance de este proyecto, un `JUMP` incondicional con penalidad fija de 2 ciclos es standard en CPUs simples sin unidad de predicción (ej. los primeros diseños MIPS clásicos), y mantiene el código de `ciclo_reloj` legible.

**Costo aceptado:** cualquier programa con saltos paga 2 ciclos de penalidad por cada `JUMP`, sin excepción — no hay forma de que el simulador "adivine bien" y los evite.

### 5.7 Aritmética Wrapping en vez de Checked

**Decisión:** `ejecutar_alu` usa `wrapping_add`/`wrapping_sub` en vez de la aritmética por defecto de Rust (que hace panic en overflow en modo debug) o `checked_add`/`checked_sub` (que devuelven `Option`).

**Por qué:** el hardware real de una ALU de N bits no "sabe" que ocurrió un overflow a menos que se le pida explícitamente verificar una flag de carry/overflow — simplemente trunca el resultado al ancho de bits disponible. `wrapping_*` es la operación de Rust que replica ese comportamiento físico exacto. Usar la aritmética por defecto haría que la simulación **crashee** con un programa perfectamente válido en hardware real (ej. un contador de 16 bits que da la vuelta de `0xFFFF` a `0x0000`) — sería simular mal el hardware, no una mejora de seguridad.

**Costo aceptado:** ninguno — es estrictamente más fiel al comportamiento real que cualquier alternativa.

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
