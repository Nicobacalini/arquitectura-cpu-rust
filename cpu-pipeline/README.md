# cpu-pipeline

> Simulación de una CPU de 5 etapas con pipeline, forwarding y detección de hazards, implementada en Rust.


## 1. Contexto y Objetivo

Simulamos una CPU con **pipeline de 5 etapas** (IF, ID, EX, MEM, WB), el modelo clásico de arquitecturas RISC (MIPS, RISC-V). El objetivo es demostrar, con una implementación funcional y testeada, cómo el hardware real resuelve los **riesgos de datos (data hazards)** que aparecen al solapar la ejecución de instrucciones consecutivas — usando **forwarding** cuando es posible, y **stalls** cuando no queda otra opción — así como el **riesgo de control** que introduce una instrucción de salto (`JUMP`).

```
┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐
│  IF  │──▶│  ID  │──▶│  EX  │──▶│ MEM  │──▶│  WB  │
└──────┘   └──────┘   └──────┘   └──────┘   └──────┘
 Fetch      Decode     Execute    Memory    Write Back
 (trae la   (decodi-   (ALU)      (lee/     (escribe el
 instruc.)  fica y                escribe   resultado en
            lee regs)             RAM)      el banco)
```

En cada ciclo de reloj, hasta 5 instrucciones distintas pueden estar "en vuelo" simultáneamente, una en cada etapa — de ahí que el estado del procesador se modele con 4 **registros de segmentación** (`IF/ID`, `ID/EX`, `EX/MEM`, `MEM/WB`), cada uno un buffer entre dos etapas consecutivas.

## 2. Estructuras de Datos

### `MemoriaProvisoria`

Memoria RAM simulada de **256 bytes** direccionables con un índice `u8` (`0x00`..`0xFF`).
En una implementación completa sería reemplazada por el `ControladorMemoria` del Proyecto 2 (ver [Sección 9](#9-integración-con-el-controlador-de-caché-proyecto-2)).

```rust
pub struct MemoriaProvisoria {
    pub ram: [u8; 256],
}
```

| Método | Descripción |
|---|---|
| `new()` | Inicializa toda la RAM en `0` |
| `leer_byte(dir)` | Lee el byte en la dirección `dir` |
| `escribir_byte(dir, dato)` | Escribe `dato` en la dirección `dir` |

### `Registro`

Identificador de los cuatro registros generales de la CPU.

```rust
pub enum Registro { R0, R1, R2, R3 }
```

> **R0 es hardwired-zero**: siempre vale `0`. Las escrituras sobre él son silenciosamente descartadas en WB, y el forwarding nunca lo anticipa (ver [Sección 4.3](#4-decisiones-de-diseño-y-su-justificación)).

### `Instruccion`

Conjunto de instrucciones (ISA) soportadas por la CPU:

```rust
pub enum Instruccion {
    NOP,
    ADD { dest: Registro, src1: Registro, src2: Registro },
    SUB { dest: Registro, src1: Registro, src2: Registro },
    LOAD { dest: Registro, direccion_ram: u8 },
    STORE { src: Registro, direccion_ram: u8 },
    JUMP { direccion_destino: usize },
}
```

| Instrucción | Operandos | Descripción |
|---|---|---|
| `NOP` | — | Sin operación (burbuja) |
| `ADD` | `dest, src1, src2` | `dest = src1 + src2` (wrapping u16) |
| `SUB` | `dest, src1, src2` | `dest = src1 - src2` (wrapping u16) |
| `LOAD` | `dest, dir_ram` | `dest = RAM[dir_ram]` |
| `STORE` | `src, dir_ram` | `RAM[dir_ram] = src` |
| `JUMP` | `dir_destino` | Salta incondicionalmente a `dir_destino`; flushea el pipeline |

### `RegistroSegmentacion`

Buffer físico entre dos etapas del pipeline. Cada uno de los cuatro registros del pipeline (`IF/ID`, `ID/EX`, `EX/MEM`, `MEM/WB`) es una instancia de esta misma estructura (justificación del diseño uniforme en [Sección 4.5](#4-decisiones-de-diseño-y-su-justificación)).

```rust
pub struct RegistroSegmentacion {
    pub instruccion: Instruccion,  // Instrucción en tránsito
    pub activa:      bool,          // false = burbuja (NOP inactivo)
    pub resultado:   Option<u16>,   // None hasta que la etapa calcula el valor
}
```

El campo `resultado: Option<u16>` es clave para el forwarding: un `LOAD` en `EX/MEM` tiene `resultado = None` hasta que termina la etapa `MEM`, lo que impide anticipar datos inexistentes.

### `CpuSegmentada`

Estado global del procesador. Contiene los cuatro registros de segmentación, el banco de registros, el PC y el contador de ciclos.

```rust
pub struct CpuSegmentada {
    pub if_id:           RegistroSegmentacion, // Fetch -> Decode
    pub id_ex:           RegistroSegmentacion, // Decode -> Execute
    pub ex_mem:          RegistroSegmentacion, // Execute -> Memory
    pub mem_wb:          RegistroSegmentacion, // Memory -> Write Back
    pub registros:       [u16; 4],             // Banco R0..R3
    pub program_counter: usize,                // Índice de la próx. instrucción
    pub contador_ciclos: u64,                  // Ciclos transcurridos
}
```

---

## 3. Algoritmo de Flujo: Hazards, Forwarding y Ciclo de Reloj

### 3.1 `detectar_load_use_hazard`

```rust
pub fn detectar_load_use_hazard(&self, instruccion_en_id: &Instruccion) -> bool
```

#### ¿Por qué existe este hazard?

El forwarding resuelve la mayoría de los riesgos de datos, pero **no todos**. Una instrucción `ADD` produce su resultado al final de **EX**. Un `LOAD`, en cambio, obtiene el dato al final de **MEM** — un ciclo más tarde. Si la instrucción inmediatamente siguiente necesita ese valor en su etapa **EX**, hay un conflicto insalvable: el dato no existe a tiempo para ser anticipado.

```
         LOAD  |  IF  |  ID  |  EX  |  MEM* |  WB  |
         ADD   |      |  IF  |  ID  |  EX*  |  MEM  |  WB  |
                                       ^        ^
                                ADD necesita  LOAD produce
                                R1 aquí       R1 aquí (MEM)
```

La única solución es insertar un **stall de 1 ciclo** (burbuja `NOP` en `id_ex`) y congelar `if_id` y el `PC`.

#### Lógica interna

1. Verifica que `id_ex` esté activo y contenga un `LOAD { dest: reg_load }`.
2. Inspecciona la instrucción en `ID`:
   - `ADD` / `SUB`: hazard si `src1 == reg_load` o `src2 == reg_load`.
   - `STORE`: hazard si `src == reg_load`.
   - `NOP`, `LOAD`, `JUMP`: no hay conflicto → retorna `false`.

### 3.2 `calcular_forwarding`

```rust
pub fn calcular_forwarding(&self, src: Registro) -> Option<u16>
```

#### Teoría

El **Forwarding (Bypassing)** conecta directamente la salida de etapas posteriores a la entrada de la ALU, evitando leer un valor desactualizado del banco de registros.

```
         +─────────── Forwarding MEM/WB ───────────────────+
         |                                                  |
         +── Forwarding EX/MEM ──+                          |
         |                       |                          v
[IF] -> [if_id] -> [ID] -> [id_ex] -> [ALU (EX)] -> [ex_mem] -> [MEM] -> [mem_wb] -> [WB]
```

#### Prioridad

| Prioridad | Fuente | Condición |
|---|---|---|
| **1 (máxima)** | `EX/MEM` | activo, `dest == src`, `resultado == Some(_)` |
| **2** | `MEM/WB` | activo, `dest == src`, `resultado == Some(_)` |
| **Sin forwarding** | Banco de registros | Ninguna etapa produce `src` |

#### Casos especiales

- **R0 hardwired-zero**: retorna `None` inmediatamente. El banco garantiza `registros[0] == 0`.
- **LOAD en EX/MEM**: su `resultado` es `None` en esa etapa (aún no leyó la RAM), por lo que la función retorna `None` sin caer a `MEM/WB`. Esto previene anticipar datos inexistentes.

### 3.3 `resolver_operando`

```rust
pub fn resolver_operando(&self, src: Registro) -> u16
```

Implementa el **MUX** a la entrada de la ALU: intenta `calcular_forwarding` primero, y solo si devuelve `None` lee directamente `self.registros[src]`.

### 3.4 `ejecutar_alu`

```rust
pub fn ejecutar_alu(&self, instruccion: RegistroSegmentacion) -> RegistroSegmentacion
```

Ejecuta la etapa **EX**:

| Instrucción | Acción |
|---|---|
| `ADD` / `SUB` | Resuelve ambos operandos con `resolver_operando` (aplicando forwarding) y calcula con `wrapping_add`/`wrapping_sub` |
| `LOAD` | `resultado = None` — el dato se calcula recién en MEM |
| `STORE` | Resuelve `src` (con forwarding) y lo empaqueta en `resultado`, como "vagón de carga" hacia MEM |
| `NOP` / inactiva | Propaga una burbuja limpia sin efectos colaterales |

### 3.5 `ejecutar_mem`

```rust
pub fn ejecutar_mem(
    &self,
    instruccion: RegistroSegmentacion,
    memoria: &mut MemoriaProvisoria,
) -> RegistroSegmentacion
```

Ejecuta la etapa **MEM (Memory Access)**. Es la única etapa autorizada para acceder a la RAM:

| Instrucción | Acción |
|---|---|
| `LOAD` | Lee `RAM[dir_ram]` → completa `resultado = Some(byte as u16)` |
| `STORE` | Escribe `resultado` (empaquetado en EX) → `RAM[dir_ram] = valor as u8` |
| Resto | Pasa sin modificaciones hacia `MEM/WB` |

### 3.6 `ejecutar_writeback`

```rust
pub fn ejecutar_writeback(&mut self)
```

Ejecuta la etapa **WB (Write Back)** usando el registro `mem_wb`:

- `ADD`, `SUB`, `LOAD` con `dest != R0`: escribe `resultado` en `registros[dest]`.
- **R0**: la escritura se descarta silenciosamente (hardwired-zero).
- `STORE`, `NOP`, registro inactivo: no modifica el banco.

### 3.7 `ciclo_reloj`

```rust
pub fn ciclo_reloj(&mut self, programa: &[Instruccion], memoria: &mut MemoriaProvisoria)
```

Orquesta un ciclo de reloj completo. Modela el **flanco ascendente de reloj** del hardware.

#### ¿Por qué el orden inverso?

En silicio, las 5 etapas operan en **paralelo**. En software (monohilo), deben ejecutarse en secuencia. Si avanzáramos de IF a WB, al actualizar `if_id` en el paso 1, la etapa ID leería la instrucción del ciclo actual en vez de la del ciclo anterior — un *data race* de simulación.

La solución es procesar en **orden inverso a la ruta de datos** (WB → MEM → ... → IF), garantizando que cada etapa lee el estado estable del ciclo anterior antes de que las etapas tempranas lo sobreescriban. La justificación completa, con diagrama comparativo, está en la [Sección 4.1](#4-decisiones-de-diseño-y-su-justificación).

#### Orden de evaluación por ciclo

```
1. WB     →  ejecutar_writeback()           consolida mem_wb en el banco de registros
2. MEM    →  nuevo_mem_wb = ejecutar_mem()  resultado guardado en variable temporal
3. Hazard →  detectar_load_use_hazard()     inspecciona if_id vs id_ex
4. JUMP   →  inspecciona id_ex              guarda el destino si hay salto
             ╔══════════════════════════════════╗
5. Branch    ║  JUMP > Stall > Normal           ║
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

#### Mecánica del JUMP — Branch Penalty

El `JUMP` se resuelve cuando llega a **EX** (en `id_ex`). Para ese momento el pipeline ya buscó 2 instrucciones incorrectas:

```
Ciclo N-2:  JUMP  en IF
Ciclo N-1:  JUMP  en ID  |  inst_A  en IF   <- incorrecta
Ciclo N:    JUMP  en EX  |  inst_A  en ID  |  inst_B  en IF  <- incorrecta
                 ^
          Se detecta aquí:
            flush id_ex (inst_A) + flush if_id (inst_B) + PC = destino
```

**Penalidad: 2 ciclos** desperdiciados. Esta implementación no incluye predicción de saltos (justificación en [Sección 4.6](#4-decisiones-de-diseño-y-su-justificación)).

#### Drenado del Pipeline (Pipeline Draining)

Cuando `PC >= len(programa)`, la etapa IF inyecta burbujas (`NOP` inactivos). Las instrucciones legítimas en tránsito completan su ciclo de vida y consolidan sus escrituras de forma limpia.

### 3.8 Diagrama Temporal: Load-Use Hazard con Stall y Forwarding

**Programa de ejemplo:**
```assembly
LOAD  R1, 0x10   ; R1 = RAM[0x10] = 15
ADD   R2, R1, R0 ; R2 = R1 + 0    = 15  (necesita R1 -> Load-Use Hazard)
```

La tabla muestra el estado de los registros de segmentación **al final de cada ciclo** (lo que imprime `Display`). La columna "Evento" describe lo ocurrido durante ese ciclo.

| Ciclo | IF/ID | ID/EX | EX/MEM | MEM/WB | Evento |
|:---:|:---:|:---:|:---:|:---:|---|
| **1** | `LOAD` | `--` | `--` | `--` | Fetch de `LOAD`. |
| **2** | `ADD` | `LOAD` | `--` | `--` | Fetch de `ADD`. `LOAD` pasa a decode. |
| **3** | `ADD` ❄️ | `--` | `LOAD` | `--` | **Load-Use Hazard detectado.** Burbuja insertada en ID/EX. IF/ID y PC congelados. `LOAD` avanza a EX/MEM sin pasar por la ALU. |
| **4** | `--` | `ADD` | `--` | `LOAD(15)` | Stall liberado. `LOAD` termina en MEM: lee `RAM[0x10]=15`. `ADD` descongelado, pasa a ID/EX. |
| **5** | `--` | `--` | `ADD(15)` | `--` | **WB** escribe `R1=15`. **Forwarding MEM/WB→EX**: `resolver_operando(R1)` anticipa 15 desde `mem_wb`. `ADD` calcula `15+0=15`. |
| **6** | `--` | `--` | `--` | `ADD(15)` | MEM pass-through de `ADD` (no accede a RAM). |
| **7** | `--` | `--` | `--` | `--` | **WB** escribe `R2=15`. Pipeline drenado. ✅ |

### 3.9 Diagrama de Flujo General de `ciclo_reloj`

```
                    ┌─────────────────────────────────┐
                    │        ciclo_reloj()            │
                    └────────────────┬────────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 1. ejecutar_writeback │  <-- mem_wb -> registros
                         └───────────┬───────────┘
                                     │
                    ┌────────────────▼───────────────┐
                    │ 2. nuevo_mem_wb = ejecutar_mem │  <-- ex_mem -> RAM
                    └────────────────┬───────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 3. Detectar hazard    │  <-- if_id vs id_ex
                         └───────────┬───────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 4. Detectar JUMP      │  <-- id_ex
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
                         │ 6. mem_wb = nuevo     │
                         └───────────┬───────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 7. contador_ciclos++  │
                         └───────────────────────┘
```

### 3.10 Trait Display

Cada tipo implementa `fmt::Display` siguiendo la convención idiomática de Rust:

| Tipo | Salida de ejemplo |
|---|---|
| `Instruccion::ADD{R1,R2,R3}` | `ADD R1,R2,R3` |
| `Instruccion::LOAD{R1, 0x2A}` | `LOAD R1,0x2A` |
| `Instruccion::JUMP{5}` | `JUMP 0x05` |
| `RegistroSegmentacion` (activo) | muestra la instrucción |
| `RegistroSegmentacion` (inactivo) | `--` |
| `CpuSegmentada` | `Ciclo N \| IF/ID: X \| ID/EX: Y \| EX/MEM: Z \| MEM/WB: W` |

---

## 4. Decisiones de Diseño y su Justificación

Esta sección documenta **por qué** el pipeline está diseñado como está, no solo **qué** hace — incluyendo un bug real que se encontró y corrigió durante el desarrollo (4.3).

### 4.1 Evaluación en Orden Inverso del Pipeline

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

**Costo aceptado:** el código es menos "legible en orden de datapath" (uno esperaría leer IF primero) — se compensa documentando el orden explícitamente en cada `ciclo_reloj`.

### 4.2 Por qué el Load-Use Hazard No Se Resuelve con Forwarding

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

### 4.3 Por qué R0 se Filtra También en el Forwarding

**Contexto:** `ejecutar_writeback` descarta silenciosamente cualquier escritura a `R0` (hardwired-zero, como `$zero`/`x0` en MIPS/RISC-V). Este filtro **por sí solo no alcanza** — de hecho, la primera versión de este pipeline tenía un bug real por esta razón.

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

### 4.4 Prioridad de Forwarding: EX/MEM sobre MEM/WB

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

### 4.5 Un Solo Tipo para las Cuatro Etapas

**Decisión:** `RegistroSegmentacion` es un único struct genérico reutilizado para los 4 buffers (`if_id`, `id_ex`, `ex_mem`, `mem_wb`), en vez de 4 tipos distintos cada uno con los campos específicos que esa transición necesita.

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

### 4.6 Ausencia de Predicción de Saltos

**Decisión:** `JUMP` siempre asume, implícitamente, que se va a tomar — no hay lógica de "predicción" propiamente dicha, simplemente se resuelve en EX y se paga la penalidad de 2 ciclos siempre.

```
Con predicción "no tomado" (no implementado)     Sin predicción (implementado)
─────────────────────────────────────────────    ──────────────────────────────
Si predicción correcta: 0 ciclos perdidos         SIEMPRE 2 ciclos perdidos
Si predicción incorrecta: 2 ciclos perdidos        (flush de if_id + id_ex,
(requiere lógica adicional de checkpoint/          sin importar si "hubiera"
 rollback más allá del flush simple)               convenido predecir distinto)
```

**Por qué:** implementar predicción real requeriría además contabilizar aciertos/fallos de predicción y un mecanismo de rollback más elaborado (ver Sección 10, extensión opcional). Para el alcance de este proyecto, un `JUMP` incondicional con penalidad fija de 2 ciclos es estándar en CPUs simples sin unidad de predicción (ej. los primeros diseños MIPS clásicos), y mantiene el código de `ciclo_reloj` legible.

**Costo aceptado:** cualquier programa con saltos paga 2 ciclos de penalidad por cada `JUMP`, sin excepción — no hay forma de que el simulador "adivine bien" y los evite.

### 4.7 Aritmética Wrapping en vez de Checked

**Decisión:** `ejecutar_alu` usa `wrapping_add`/`wrapping_sub` en vez de la aritmética por defecto de Rust (que hace panic en overflow en modo debug) o `checked_add`/`checked_sub` (que devuelven `Option`).

**Por qué:** el hardware real de una ALU de N bits no "sabe" que ocurrió un overflow a menos que se le pida explícitamente verificar una flag de carry/overflow — simplemente trunca el resultado al ancho de bits disponible. `wrapping_*` es la operación de Rust que replica ese comportamiento físico exacto. Usar la aritmética por defecto haría que la simulación **crashee** con un programa perfectamente válido en hardware real (ej. un contador de 16 bits que da la vuelta de `0xFFFF` a `0x0000`) — sería simular mal el hardware, no una mejora de seguridad.

**Costo aceptado:** ninguno — es estrictamente más fiel al comportamiento real que cualquier alternativa.

---

## 5. API Pública

| Función / Método | Firma | Detalle en |
|---|---|---|
| `detectar_load_use_hazard` | `(&self, &Instruccion) -> bool` | [3.1](#31-detectar_load_use_hazard) |
| `calcular_forwarding` | `(&self, Registro) -> Option<u16>` | [3.2](#32-calcular_forwarding) |
| `resolver_operando` | `(&self, Registro) -> u16` | [3.3](#33-resolver_operando) |
| `ejecutar_alu` | `(&self, RegistroSegmentacion) -> RegistroSegmentacion` | [3.4](#34-ejecutar_alu) |
| `ejecutar_mem` | `(&self, RegistroSegmentacion, &mut MemoriaProvisoria) -> RegistroSegmentacion` | [3.5](#35-ejecutar_mem) |
| `ejecutar_writeback` | `(&mut self)` | [3.6](#36-ejecutar_writeback) |
| `ciclo_reloj` | `(&mut self, &[Instruccion], &mut MemoriaProvisoria)` | [3.7](#37-ciclo_reloj) |
| `MemoriaProvisoria::new/leer_byte/escribir_byte` | ver [Sección 2](#2-estructuras-de-datos) | — |

---

## 6. Suite de Tests

Todos los tests viven en [`src/tests.rs`](src/tests.rs). Se ejecutan con:

```bash
cargo test --package cpu-pipeline
```

| Categoría | Tests | Qué verifican |
|---|---|---|
| **Forwarding unitario** | 5 | `calcular_forwarding`: sin pipeline, R0→None, EX/MEM activo, MEM/WB activo, LOAD en EX/MEM sin caer a MEM/WB |
| **Prioridad de forwarding** | 1 | EX/MEM tiene prioridad absoluta sobre MEM/WB |
| **Forwarding integrado** | 3 | EX/MEM→EX, MEM/WB→EX, ambos operandos src1+src2 simultáneos |
| **Hazard detection unitario** | 6 | LOAD→ADD detecta, LOAD→SUB detecta, LOAD→LOAD no, LOAD→JUMP no, id_ex inactivo no, ADD en id_ex no |
| **Load-Use con stall** | 3 | ADD después de LOAD, SUB después de LOAD, STORE después de LOAD |
| **Control hazard (JUMP)** | 2 | Flush de instrucciones especulativas, reset de PC |
| **R0 hardwired-zero** | 2 | Forwarding nunca anticipa R0, LOAD y SUB no modifican R0 |
| **Memoria** | 2 | Lectura/escritura, estado inicial en cero |
| **Aritmética** | 2 | ADD+SUB sin hazards, overflow wrapping u16 |
| **LOAD/STORE** | 2 | Round-trip STORE→LOAD, STORE con forwarding desde EX |
| **Pipeline NOP / vacío** | 2 | NOPs no modifican registros, programa vacío es estable |
| **Contador de ciclos** | 1 | Avanza exactamente 1 por ciclo |
| **Display** | 4 | Formato de cada instrucción, registro activo vs inactivo, CPU con ciclo 0, CPU con ciclo N |

**Resultado:** `36 passed; 0 failed` ✅

---

## 7. Errores Comunes al Implementar (Gotchas)

Bugs reales encontrados durante el desarrollo de este crate — documentados para que no se repitan al extenderlo:

- **Perder la instrucción `LOAD` durante un stall:** en un borrador temprano, al insertar la burbuja se sobreescribía `id_ex` con `NOP` *antes* de haber movido el `LOAD` real a `ex_mem`, perdiéndolo del pipeline para siempre. La corrección: en la rama de stall, `ex_mem = self.id_ex` (el `LOAD` avanza) debe ejecutarse **antes** de asignar la burbuja a `id_ex`.
- **Filtrar R0 solo en `ejecutar_writeback` y no en `calcular_forwarding`:** ver [Sección 4.3](#4-decisiones-de-diseño-y-su-justificación) — el bug del "valor fantasma" de R0.
- **Definir la lógica del pipeline en `main.rs` en vez de `lib.rs`:** un binario (`main.rs`) no es importable desde otros crates del workspace. Toda la lógica reutilizable debe vivir en la librería (`lib.rs`), dejando `main.rs` únicamente como demo/punto de entrada.
- **Evaluar el ciclo en orden natural (IF→WB) en vez de inverso:** ver [Sección 4.1](#4-decisiones-de-diseño-y-su-justificación) — produce lecturas de buffers ya contaminados por el mismo ciclo.

---

## 8. Estructura de Archivos del Crate

```
cpu-pipeline/
├── Cargo.toml
└── src/
    ├── lib.rs      # API pública: tipos, pipeline, hazard detection, forwarding
    ├── main.rs     # Binario de demo: ejecuta un programa de ejemplo
    └── tests.rs    # Suite completa de tests unitarios e integración
```

---

## 9. Integración con el Controlador de Caché (Proyecto 2)

`MemoriaProvisoria` es deliberadamente "provisoria": su única razón de ser es desacoplar el desarrollo del pipeline del desarrollo de la jerarquía de memoria. Cuando el crate `cache-controller` esté listo:

- `cpu-pipeline/Cargo.toml` va a declarar `cache-controller = { path = "../cache-controller" }`.
- `ejecutar_mem` (Sección 3.5) va a recibir un `&mut ControladorMemoria` en vez de `&mut MemoriaProvisoria`, sin cambiar ninguna otra parte del pipeline — la firma de la función es idéntica en forma (`leer_byte`/`escribir_byte`), solo cambia el tipo concreto.
- El binario final (`sistema-integrado`) va a poder imprimir, al terminar la ejecución de un programa, tanto la traza del pipeline como las estadísticas de la caché (hits/misses/desalojos) en un único reporte combinado.

---

