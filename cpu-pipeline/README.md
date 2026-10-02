# cpu-pipeline

> Simulación de una CPU de 5 etapas con pipeline, forwarding y detección de hazards, implementada en Rust.

---

## 1. Contexto y Objetivo

Simulamos una CPU con **pipeline de 5 etapas** (IF, ID, EX, MEM, WB), el modelo clásico de arquitecturas RISC (MIPS, RISC-V). El objetivo es demostrar, con una implementación funcional y testeada, cómo el hardware real resuelve los **riesgos de datos (data hazards)** mediante **forwarding** y **stalls**, y el **riesgo de control** de una instrucción `JUMP`.

```
┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐
│  IF  │──▶│  ID  │──▶│  EX  │──▶│ MEM  │──▶│  WB  │
└──────┘   └──────┘   └──────┘   └──────┘   └──────┘
 Fetch      Decode     Execute    Memory    Write Back
 (trae la   (decodi-   (ALU)      (lee/     (escribe el
 instruc.)  fica y                escribe   resultado en
            lee regs)             RAM)      el banco)
```

En cada ciclo, hasta 5 instrucciones distintas pueden estar "en vuelo" simultáneamente, modeladas con 4 **registros de segmentación** (`IF/ID`, `ID/EX`, `EX/MEM`, `MEM/WB`).

---

## 2. Estructuras de Datos

### `MemoriaProvisoria` / `Memoria`

Alias que apuntan al `ControladorMemoria` del crate `cache-controller`. La memoria del pipeline es la caché L1 real (asociativa por conjuntos, LRU, Write-Back):

```rust
pub use cache_controller::ControladorMemoria as Memoria;
pub type MemoriaProvisoria = ControladorMemoria;
```

Los tests y el código existente pueden usar cualquier alias — ambos apuntan al mismo tipo.

| Método | Descripción |
|---|---|
| `new()` | Inicializa caché y RAM en `0` |
| `leer_byte(dir: u16)` | Lee a través de la caché (hit/miss con LRU) |
| `escribir_byte(dir: u16, dato: u8)` | Escribe en caché (Write-Back, Write-Allocate) |
| `flush()` | Vuelca líneas dirty a RAM sin invalidar |

> En la integración con jerarquía de dos niveles, `ejecutar_mem` puede recibir también una `&mut JerarquiaCache` — ver [Sección 9](#9-integración-con-el-subsistema-de-caché).

### `Registro`

```rust
pub enum Registro { R0, R1, R2, R3 }
```

> **R0 es hardwired-zero**: siempre vale `0`. Las escrituras se descartan en WB; el forwarding nunca lo anticipa (ver [§4.3](#43-por-qué-r0-se-filtra-también-en-el-forwarding)).

### `Instruccion`

Conjunto de instrucciones (ISA) soportadas por la CPU:

```rust
pub enum Instruccion {
    NOP,
    ADD  { dest: Registro, src1: Registro, src2: Registro },
    SUB  { dest: Registro, src1: Registro, src2: Registro },
    LOAD  { dest: Registro, direccion_ram: u16 },
    STORE { src:  Registro, direccion_ram: u16 },
    JUMP  { direccion_destino: usize },
}
```

| Instrucción | Descripción |
|---|---|
| `NOP` | Sin operación (burbuja) |
| `ADD` | `dest = src1 + src2` (wrapping u16) |
| `SUB` | `dest = src1 - src2` (wrapping u16) |
| `LOAD` | `dest = RAM[dir_ram]` (a través de la caché) |
| `STORE` | `RAM[dir_ram] = src` (a través de la caché, Write-Back) |
| `JUMP` | Salto incondicional; flushea el pipeline (branch penalty: 2 ciclos) |

> `LOAD` y `STORE` usan `u16` para `direccion_ram` — permite acceder a toda la RAM de 4096 bytes sin conversiones.

### `RegistroSegmentacion`

```rust
pub struct RegistroSegmentacion {
    pub instruccion: Instruccion,   // Instrucción en tránsito
    pub activa:      bool,           // false = burbuja NOP inactivo
    pub resultado:   Option<u16>,   // None hasta que la etapa calcula el valor
}
```

El campo `resultado: Option<u16>` es clave: un `LOAD` en `EX/MEM` tiene `resultado = None` hasta que termina MEM, impidiendo anticipar datos inexistentes.

### `CpuSegmentada`

```rust
pub struct CpuSegmentada {
    pub if_id:                    RegistroSegmentacion,
    pub id_ex:                    RegistroSegmentacion,
    pub ex_mem:                   RegistroSegmentacion,
    pub mem_wb:                   RegistroSegmentacion,
    pub registros:                [u16; 4],
    pub program_counter:          usize,
    pub contador_ciclos:          u64,
    pub instrucciones_completadas: u64,
}
```

---

## 3. Algoritmo de Flujo: Hazards, Forwarding y Ciclo de Reloj

### 3.1 `detectar_load_use_hazard`

```rust
pub fn detectar_load_use_hazard(&self, instruccion_en_id: &Instruccion) -> bool
```

El forwarding resuelve la mayoría de los riesgos de datos, pero **no todos**. Un `LOAD` obtiene el dato al final de **MEM** — si la instrucción siguiente lo necesita en **EX**, el dato no existe todavía. La única solución es un **stall de 1 ciclo**.

```
         LOAD  |  IF  |  ID  |  EX  |  MEM* |  WB  |
         ADD   |      |  IF  |  ID  |  EX*  |  MEM |  WB  |
                                       ^        ^
                                ADD necesita  LOAD produce
                                R1 aquí       R1 aquí (MEM)
```

Lógica: si `id_ex` es un `LOAD` activo y la instrucción en `ID` (`ADD`/`SUB`/`STORE`) usa el registro destino del `LOAD` → retorna `true`. `NOP`, `LOAD`, `JUMP` en ID nunca generan este hazard.

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

Casos especiales: **R0** retorna `None` directamente (hardwired-zero). **LOAD en EX/MEM**: `resultado` es `None` todavía — retorna `None` sin caer a `MEM/WB`.

### 3.3 `resolver_operando`

```rust
pub fn resolver_operando(&self, src: Registro) -> u16
```

Implementa el **MUX** de la ALU: intenta forwarding primero; si retorna `None`, lee del banco de registros.

### 3.4 `ejecutar_alu`

| Instrucción | Acción |
|---|---|
| `ADD` / `SUB` | Resuelve operandos (forwarding aplicado), calcula con `wrapping_add`/`wrapping_sub` |
| `LOAD` | `resultado = None` — el dato llega en MEM |
| `STORE` | Resuelve `src` (forwarding) y lo empaqueta en `resultado` |
| `NOP` / inactiva | Propaga burbuja limpia |

### 3.5 `ejecutar_mem`

```rust
pub fn ejecutar_mem(
    &self,
    instruccion: RegistroSegmentacion,
    memoria: &mut ControladorMemoria,
) -> RegistroSegmentacion
```

Única etapa autorizada para acceder a la RAM (a través de la caché):

| Instrucción | Acción |
|---|---|
| `LOAD` | `dato = memoria.leer_byte(direccion_ram)` → `resultado = Some(dato as u16)` |
| `STORE` | `memoria.escribir_byte(direccion_ram, resultado as u8)` |
| Resto | Pasa sin modificaciones |

### 3.6 `ejecutar_writeback`

```rust
pub fn ejecutar_writeback(&mut self)
```

- Incrementa `instrucciones_completadas += 1` (solo si `mem_wb.activa`).
- `ADD`, `SUB`, `LOAD` con `dest != R0`: escribe `resultado` en `registros[dest]`.
- Escrituras a R0 se descartan silenciosamente.

### 3.7 `ciclo_reloj`

```rust
pub fn ciclo_reloj(&mut self, programa: &[Instruccion], memoria: &mut ControladorMemoria)
```

Orquesta un ciclo de reloj completo. Modela el **flanco ascendente de reloj** del hardware.

#### ¿Por qué el orden inverso?

En silicio, las 5 etapas operan en **paralelo**. En software (monohilo), deben ejecutarse en secuencia. Si avanzáramos de IF a WB, al actualizar `if_id` en el paso 1, la etapa ID leería la instrucción del ciclo actual en vez de la del ciclo anterior — un *data race* de simulación.

La solución es procesar en **orden inverso a la ruta de datos** (WB → MEM → ... → IF), garantizando que cada etapa lee el estado estable del ciclo anterior antes de que las etapas tempranas lo sobreescriban. La justificación completa, con diagrama comparativo, está en la [Sección 4.1](#4-decisiones-de-diseño-y-su-justificación).

#### Orden de evaluación por ciclo

```
1. WB     → ejecutar_writeback()
2. MEM    → nuevo_mem_wb = ejecutar_mem()      [resultado en variable temporal]
3. Hazards→ detectar_load_use_hazard() + JUMP en id_ex
             ╔════════════════════════════════╗
4. Avance    ║  JUMP > Stall > Normal          ║
             ╠════════════════════════════════╣
             ║ JUMP:   flush if_id + id_ex    ║
             ║         PC = dir_destino       ║
             ╠────────────────────────────────╣
             ║ STALL:  ex_mem = id_ex (sin ALU)║
             ║         id_ex  = burbuja       ║
             ║         if_id y PC congelados  ║
             ╠────────────────────────────────╣
             ║ NORMAL: ex_mem = ALU(id_ex)    ║
             ║         id_ex  = if_id         ║
             ║         if_id  = Fetch(PC)     ║
             ║         PC    += 1             ║
             ╚════════════════════════════════╝
5. mem_wb = nuevo_mem_wb
6. contador_ciclos += 1
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

```assembly
LOAD  R1, 0x10   ; R1 = RAM[0x10] = 15
ADD   R2, R1, R0 ; R2 = R1 + 0    (Load-Use Hazard → 1 stall)
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
                    ┌────────────────▼───────────────┐
                    │ 3. Detectar hazard y JUMP      │  <-- if_id vs id_ex
                    └────────────────┬───────────────┘
                                     │
              ┌──────────────────────┼───────────────────────┐
              │                      │                       │
        JUMP en EX             STALL (hazard)           NORMAL
              │                      │                       │
   ex_mem=ALU(id_ex)       ex_mem=id_ex (directo)   ex_mem=ALU(id_ex)
   id_ex=burbuja            id_ex=burbuja            id_ex=if_id
   if_id=burbuja            (if_id y PC congelados)  if_id=Fetch(PC)
   PC=dir_destino                                    PC+=1
              │                      │                       │
              └──────────────────────┼───────────────────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 5. mem_wb = nuevo     │
                         └───────────┬───────────┘
                                     │
                         ┌───────────▼───────────┐
                         │ 6. contador_ciclos++  │
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

### 4.1 Evaluación en Orden Inverso del Pipeline

En silicio, las 5 etapas operan **en paralelo**. En software monohilo deben ser secuenciales. El orden inverso `WB → MEM → ... → IF` garantiza que cada etapa lee el estado estable del ciclo anterior antes de que etapas tempranas lo sobreescriban.

```
❌ ORDEN NATURAL (INCORRECTO)        ✅ ORDEN INVERSO (CORRECTO)
1. if_id = Fetch(PC)  ← escribe     1. WB   lee mem_wb  (viejo)
2. id_ex = if_id      ← CONTAMINADO 2. MEM  lee ex_mem  (viejo)
3. ex_mem = ALU(id_ex)              3. hazard: lee if_id, id_ex (viejos)
4. mem_wb = ex_mem                  4. ex_mem = ALU(id_ex)  ← recién ahora
                                    5. id_ex  = if_id
                                    6. if_id  = Fetch(PC)   ← al final
                                    7. mem_wb = nuevo_mem_wb
```

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

`RegistroSegmentacion` uniforme permite avanzar el pipeline con simples asignaciones de struct (`self.ex_mem = self.id_ex`), sin conversiones entre tipos. El campo `resultado` está "de más" en `if_id`/`id_ex`, pero el costo de memoria es insignificante frente a la simplicidad del código.

### 4.6 Ausencia de Predicción de Saltos

`JUMP` siempre paga 2 ciclos de penalidad. No hay lógica de predicción porque implementarla requeriría checkpoint/rollback y contabilización de aciertos/fallos — fuera del alcance pedagógico de este simulador.

### 4.7 Aritmética Wrapping

`wrapping_add`/`wrapping_sub` replica el comportamiento físico de la ALU: sin panic en overflow (como `0xFFFF + 1 → 0x0000` en hardware real).

---

## 5. API Pública

| Función / Método | Firma | Sección |
|---|---|---|
| `detectar_load_use_hazard` | `(&self, &Instruccion) -> bool` | 3.1 |
| `calcular_forwarding` | `(&self, Registro) -> Option<u16>` | 3.2 |
| `resolver_operando` | `(&self, Registro) -> u16` | 3.3 |
| `ejecutar_alu` | `(&self, RegistroSegmentacion) -> RegistroSegmentacion` | 3.4 |
| `ejecutar_mem` | `(&self, RegistroSegmentacion, &mut ControladorMemoria) -> RegistroSegmentacion` | 3.5 |
| `ejecutar_writeback` | `(&mut self)` | 3.6 |
| `ciclo_reloj` | `(&mut self, &[Instruccion], &mut ControladorMemoria)` | 3.7 |

---

## 6. Suite de Tests

```bash
cargo test --package cpu-pipeline
```

**Resultado: `39 passed + 1 doctest; 0 failed`**

| Categoría | Tests | Qué verifican |
|---|---|---|
| **Forwarding unitario** | 5 | `calcular_forwarding`: sin pipeline, R0→None, EX/MEM activo, MEM/WB activo, LOAD sin caer a MEM/WB |
| **Prioridad de forwarding** | 1 | EX/MEM tiene prioridad absoluta sobre MEM/WB |
| **Forwarding integrado** | 3 | EX/MEM→EX, MEM/WB→EX, ambos operandos src1+src2 simultáneos |
| **Forwarding en cadena** | 1 | 3 instrucciones consecutivas dependientes sin stalls |
| **Hazard detection unitario** | 6 | LOAD→ADD, LOAD→SUB, LOAD→LOAD no, LOAD→JUMP no, id_ex inactivo, ADD en id_ex |
| **Load-Use con stall** | 3 | ADD, SUB y STORE después de LOAD |
| **Control hazard (JUMP)** | 2 | Flush especulativo, reset de PC |
| **R0 hardwired-zero** | 2 | Forwarding no anticipa R0, LOAD/SUB no modifican R0 |
| **Memoria** | 2 | Lectura/escritura, estado inicial |
| **Aritmética** | 2 | ADD+SUB sin hazards, overflow wrapping u16 |
| **LOAD/STORE** | 2 | Round-trip STORE→LOAD, STORE con forwarding desde EX |
| **Pipeline NOP / vacío** | 2 | NOPs no modifican registros, programa vacío estable |
| **Métricas** | 2 | `contador_ciclos` avanza exactamente 1, contabiliza completadas ignorando burbujas |
| **Display** | 4 | Formato instrucciones, registro activo/inactivo, CPU ciclo 0 y N |

---

## 7. Errores Comunes al Implementar (Gotchas)

- **Perder el `LOAD` durante un stall:** asignar burbuja a `id_ex` antes de mover el `LOAD` a `ex_mem` → lo pierde. Orden correcto: `ex_mem = self.id_ex` **primero**, `id_ex = burbuja` después.
- **Filtrar R0 solo en `ejecutar_writeback`:** el forwarding lee de los buffers del pipeline directamente — sin el filtro en `calcular_forwarding`, R0 produce valores fantasma (ver §4.3).
- **Poner la lógica del pipeline en `main.rs`:** un binario no es importable. Toda lógica reutilizable vive en `lib.rs`.
- **Evaluar el ciclo en orden natural (IF→WB):** produce lecturas de buffers ya contaminados (ver §4.1).

---

## 8. Estructura de Archivos del Crate

```
cpu-pipeline/
├── Cargo.toml
└── src/
    ├── lib.rs      # API pública: tipos, pipeline, hazard detection, forwarding
    ├── main.rs     # Binario de demo
    └── tests.rs    # 39 tests unitarios + 1 doctest
```

---

## 9. Integración con el Subsistema de Caché

`MemoriaProvisoria` ya es un alias de `ControladorMemoria` (caché L1 real con LRU, Write-Back, Write-Allocate). La integración con la jerarquía de dos niveles está implementada en `cache-controller` mediante `JerarquiaCache`:

```rust
// En sistema-integrado, la CPU puede operar sobre ControladorMemoria (L1 solo)
// o sobre JerarquiaCache (L1 → L2 → RAM):
cpu.ciclo_reloj(&programa, &mut memoria);  // memoria: ControladorMemoria
// Para jerarquía completa, ejecutar_mem puede recibir &mut JerarquiaCache
// con la misma firma leer_byte/escribir_byte — no requiere cambios en el pipeline.
```

La etapa `ejecutar_mem` es la **única** que accede a memoria — no hay ningún otro punto de contacto entre pipeline y caché. El `flush()` final sincroniza las líneas dirty con la RAM principal.

```bash
# Ejecutar el sistema integrado completo (CPU + caché)
cargo run --package sistema-integrado
# Verificar todos los tests del workspace
cargo test --workspace
```

---

## Fase 2 — Integración con MMU

A partir de la Fase 2, la etapa `MEM (ejecutar_mem)` ya **no llama** a `JerarquiaCache` directamente. La firma cambió:

```rust
// Antes (Fase 1):
pub fn ejecutar_mem(&self, instruccion: RegistroSegmentacion, memoria: &mut JerarquiaCache)

// Ahora (Fase 2):
pub fn ejecutar_mem(&self, instruccion: RegistroSegmentacion, memoria: &mut Mmu)
```

La `Mmu` es transparente para el pipeline: ofrece las mismas operaciones `leer_byte(vaddr, tipo)` y `escribir_byte(vaddr, dato)` pero internamente interpone TLB → Page Table → L1 → L2 → RAM. El pipeline no sabe ni necesita saber que las direcciones que maneja son virtuales.

`MemoriaProvisoria` (alias de compatibilidad) ahora apunta a `Mmu` en lugar de `JerarquiaCache`.

### Tests de integración Fase 2 (en `tests.rs`)

| Test | Fenómeno |
|---|---|
| `test_integracion_cpu_dispara_page_fault` | 17 LOADs a 17 páginas → `page_faults ≥ 17` |
| `test_integracion_asid_aislamiento_de_traducciones` | ASID=0 y ASID=1 no comparten entradas TLB |
