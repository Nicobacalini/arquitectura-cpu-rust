# Arquitectura CPU en Rust

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-dea584.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Cargo](https://img.shields.io/badge/Cargo-Workspace-orange.svg?style=flat-square&logo=rust)](https://doc.rust-lang.org/cargo/)
[![Architecture](https://img.shields.io/badge/Architecture-RISC%2016--bit-blue.svg?style=flat-square)](https://en.wikipedia.org/wiki/Reduced_instruction_set_computer)
[![Pipeline](https://img.shields.io/badge/Pipeline-5--Stage-blueviolet.svg?style=flat-square)](https://en.wikipedia.org/wiki/Classic_RISC_pipeline)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)]()
[![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg?style=flat-square&logo=linux)](https://www.linux.org/)

Simulación didáctica de un procesador RISC de **16 bits** con **pipeline de 5 etapas** (`IF`, `ID`, `EX`, `MEM`, `WB`), resolución de **riesgos de datos (Data Hazards)** mediante *Forwarding* y *Stalls*, y subsistema de memoria caché.

> **Nota de diseño**: Este proyecto replica fielmente el comportamiento de una CPU segmentada a nivel de ciclo de reloj. Los conceptos implementados aplican directamente a arquitecturas reales como MIPS, RISC-V y ARM.

---

## 1. Fundamentos Teóricos de la Arquitectura

### 1.1. Concepto de Pipeline (Segmentación)
Un procesador segmentado no espera a que una instrucción complete todas sus fases de ejecución antes de iniciar la siguiente. En su lugar, divide el procesamiento en **etapas independientes**, permitiendo ejecutar múltiples instrucciones de forma **solapada en el tiempo** (similar a una línea de ensamblaje industrial).

El pipeline implementado sigue el modelo clásico RISC de 5 etapas:

1. **IF (Instruction Fetch)**: Busca la siguiente instrucción en la memoria apuntada por el Contador de Programa (`program_counter`).
2. **ID (Instruction Decode)**: Decodifica la instrucción trazada, lee los operandos desde el Banco de Registros (`registros`) y detecta posibles riesgos.
3. **EX (Execute)**: Realiza operaciones aritméticas o lógicas en la Unidad Aritmético Lógica (**ALU**) o calcula direcciones efectivas de memoria.
4. **MEM (Memory Access)**: Lee o escribe datos en la memoria RAM (únicamente para instrucciones `LOAD` y `STORE`).
5. **WB (Write Back)**: Escribe el resultado final (de la ALU o leídos de RAM) de regreso en el Banco de Registros.

```text
[IF] ---> (if_id) ---> [ID] ---> (id_ex) ---> [EX] ---> (ex_mem) ---> [MEM] ---> (mem_wb) ---> [WB]
Fetch      Buffer      Decode     Buffer      Execute    Buffer       Memory     Buffer        WriteBack
```

---

### 1.2. Registros de Segmentación (Buffers de Desacople)
Entre cada par de etapas contiguas existen **registros de segmentación** (`if_id`, `id_ex`, `ex_mem`, `mem_wb`). Estos actúan como buffers que guardan la "fotografía" o estado físico de la instrucción y sus datos al final de cada ciclo de reloj, aislando una etapa de la otra.

---

### 1.3. Riesgos de Datos (Data Hazards / Dependencias RAW)
Un **Data Hazard** de tipo **RAW (Read After Write)** ocurre cuando una instrucción intenta leer un registro cuyo valor actualizado aún está siendo procesado o escrito por una instrucción anterior en el pipeline.

Para resolver esto sin perder ciclos innecesarios, el procesador aplica dos técnicas principales:
* **Forwarding (Anticipación de Datos)**.
* **Stalls (Congelamiento e Inyección de Burbujas NOP)**.

---

### 1.4. Especificaciones del Procesador (Arquitectura de 16 bits)
En la teoría clásica de arquitectura de computadoras, el tamaño en bits de una CPU (*word size* o tamaño de palabra) está definido por el **ancho de sus registros de propósito general y el bus de datos interno de su Unidad Aritmético Lógica (ALU)**.

Por lo tanto, este simulador implementa una **CPU de 16 bits** (palabra de **2 bytes**):

| Componente | Tipo en Rust | Tamaño en bits | Tamaño en bytes | Descripción en la Arquitectura |
| :--- | :--- | :--- | :--- | :--- |
| **Banco de Registros (`R0`–`R3`)** | `[u16; 4]` | **16 bits** | **2 bytes** | Almacena operandos y resultados de 0 a 65.535 (`0x0000`..`0xFFFF`). |
| **ALU (Bus de Datos / Operaciones)**| `u16` | **16 bits** | **2 bytes** | Cálculos aritméticos (`wrapping_add`, `wrapping_sub`) y resultados de cómputo. |
| **Valores Inmediatos** | `u16` | **16 bits** | **2 bytes** | Constantes numéricas en instrucciones (permite rangos completos de 16 bits). |
| **Celda de RAM** | `u8` | **8 bits** | **1 byte** | Cada posición direccionable de la memoria almacena un byte individual. |
| **Espacio de Direccionamiento RAM** | `[u8; 4096]` | **16 bits** (`u16`) | **4096 bytes** | Rango `0x0000` a `0x0FFF` direccionable por instrucciones `LOAD`/`STORE`. |
| **Contador de Programa (`PC`)** | `usize` | 32 o 64 bits | 4 u 8 bytes | Puntero/índice en la memoria de instrucciones del simulador en memoria de host. |

> **Comparación conceptual**: Es una arquitectura comparable a procesadores históricos de 16 bits como el **Intel 8086** o implementaciones didácticas compactas de **MIPS-16/DLX**, donde los datos procesados en la ruta de datos son de 16 bits mientras que la memoria física se organiza y direcciona a nivel de bytes (`u8`).

---

## 2. Estructuras de Datos en Rust

El modelo de dominio está expresado en Rust garantizando seguridad de tipos y patrones exhaustivos:

### `pub enum Registro`
Representa el conjunto de registros de propósito general disponibles en la CPU (banco de 4 registros de 16 bits):
* `R0`: **Registro hardwired zero** — siempre vale `0`. Las escrituras sobre él se descartan silenciosamente. Replica el comportamiento de `$zero` en MIPS y `x0` en RISC-V. Es útil como operando neutro o para instrucciones que no necesitan un destino real.
* `R1`, `R2`, `R3`: Registros de propósito general de lectura/escritura.

### `pub enum Instruccion`
Define la arquitectura de conjunto de instrucciones (ISA) soportada:
* `NOP`: Operación nula (burbuja).
* `ADD { dest: Registro, src1: Registro, src2: Registro }`: Suma de registros (`dest = src1 + src2`).
* `SUB { dest: Registro, src1: Registro, src2: Registro }`: Resta de registros (`dest = src1 - src2`).
* `LOAD { dest: Registro, direccion_ram: u16 }`: Carga un dato de RAM en un registro (`dest = RAM[direccion]`).
* `STORE { src: Registro, direccion_ram: u16 }`: Almacena el contenido de un registro en RAM (`RAM[direccion] = src`).
* `JUMP { direccion_destino: usize }`: Salto incondicional. Redirige el `program_counter` a `direccion_destino` y descarta las instrucciones incorrectas que ya entraron al pipeline (**branch penalty de 2 ciclos**).

### `pub struct RegistroSegmentacion`
Estructura física de los buffers inter-etapa:
```rust
pub struct RegistroSegmentacion {
    pub instruccion: Instruccion,
    pub activa: bool,
    pub resultado: Option<u16>,
}
```
* **`instruccion`**: Instrucción en tránsito por esa etapa.
* **`activa`**: Indica si el buffer contiene una instrucción válida (evita ejecutar burbujas o datos basura).
* **`resultado`**: Contiene el resultado parcial o final (`Option<u16>`). Vale `None` si la instrucción aún no ha producido su dato (por ejemplo, un `LOAD` en la etapa EX).

### `pub struct CpuSegmentada`
Representa el estado global de la CPU:
```rust
pub struct CpuSegmentada {
    pub if_id: RegistroSegmentacion,   // Buffer IF → ID
    pub id_ex: RegistroSegmentacion,   // Buffer ID → EX
    pub ex_mem: RegistroSegmentacion,  // Buffer EX → MEM
    pub mem_wb: RegistroSegmentacion,  // Buffer MEM → WB
    pub registros: [u16; 4],           // Banco de registros: [R0, R1, R2, R3]
    pub program_counter: usize,        // Índice de la próxima instrucción a buscar
    pub contador_ciclos: u64,          // Ciclos de reloj transcurridos
    pub instrucciones_completadas: u64,// Instrucciones completadas (retiradas en WB)
}
```

### `pub struct MemoriaProvisoria` / `Mmu` / `JerarquiaCache`
El subsistema de memoria está desacoplado en capas ortogonales:
1. **`Mmu`** (disponible en `cpu-pipeline` bajo el alias `MemoriaProvisoria`): Capa de memoria virtual. Contiene la `Tlb` (8 entradas asociativas) y la `TablaDePaginas` (256 entradas). Traduce direcciones virtuales a direcciones físicas de RAM e intercepta page faults y violaciones de permisos.
2. **`JerarquiaCache`**: Capa física de memoria intermediada. Orquesta la caché L1 (`ControladorMemoria`), la caché L2 (`NivelL2`) y la memoria principal física (`[u8; TAMANO_RAM]` de 4096 bytes).

```rust
pub struct Mmu {
    pub tlb: Tlb,
    pub page_table: TablaDePaginas,
    pub jerarquia: JerarquiaCache,
    pub asid_actual: u32,
    // ...
}
```
| Método | Firma | Descripción |
|---|---|---|
| `nueva()` / `default()` | `fn nueva() -> Self` | Inicializa TLB, tabla de páginas, L1, L2 y RAM vacías con valores por defecto. |
| `leer_byte` | `fn leer_byte(&mut self, dir_virtual: u16, tipo: TipoAcceso) -> Option<u8>` | Traduce la dirección virtual a física y delega a la jerarquía de caché. Reintenta tras resolver un page fault. |
| `escribir_byte` | `fn escribir_byte(&mut self, dir_virtual: u16, dato: u8) -> bool` | Escribe aplicando Write-Back / Write-Allocate tras traducir. Devuelve `false` ante violación de protección (solo lectura). |
| `cambiar_asid` | `fn cambiar_asid(&mut self, nuevo_asid: u32)` | Cambia el identificador de espacio de direcciones activo (context switch). |

---


## 3. Diagrama General de la Arquitectura y Ruta de Datos (Datapath)

```text
                                +--------------------------- Camino de WB (Dato escrito) -----------------------------------------------+
                                |                                                                                                       |
                                v                                                                                                       |
  [ PC ] ---> [ Mem. Prog ] ---> | IF/ID | ---> [ Banco Regs ] ---> | ID/EX | ---> [ MUX ] ---> [  ALU  ] ---> | EX/MEM |                      |
    ^              |                 |               |                 |             ^           ^                |                         |
    |              v                 |               v                 |             |           |                v                         |
    |         (Instrucción)          |          (Lectura R)            |             |           v        [ MMU (TLB / PT) ]                |
    |                                |                                 |             |        [ MUX ]             | (Dir. física)           |
    |                                |                                 |             |           ^                v                         |
    +----[ Hazard Detection Unit ]<--+                                 |             |           |       [ Caché L1/L2/RAM ] ---> | MEM/WB | --+
    |      - Congela PC              |                                 |             |           |                |                  |
    |      - Congela IF/ID           +---------------------------------+             |           |                v                  |
    |      - Inserta burbuja (NOP)                                                   |           |           (Dato leído)            |
    +-----> en ID/EX                                                                 |           |                                   |
                                                                                     |           |                                   |
                                             +---------------------------------------+-----------+                                   |
                                             |                                                                                       |
                                             |                     Unidad de Forwarding                                              |
                                             |     (Resuelve operandos anticipando desde EX/MEM y MEM/WB)                            |
                                             +---------------------------------------------------------------------------------------+
                                                                 ^                                                                   |
                                                                 |------------- Forwarding desde EX/MEM -----------------------------+
                                                                 |                                                                   |
                                                                 +------------- Forwarding desde MEM/WB -----------------------------+
```

---

## 4. Estructura del Proyecto Workspace

```text
arquitectura-cpu-rust/
├── Cargo.toml                       # Configuración raíz del Cargo Workspace
├── README.md                        # Documentación teórica y técnica global
│
├── cpu-pipeline/                    # Crate: Simulación del pipeline del procesador
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   # Pipeline segmentado, forwarding, hazard unit, MemoriaProvisoria (= Mmu)
│       ├── main.rs                  # Demo individual de CPU segmentada
│       └── tests.rs                 # 42 tests unitarios e integrados (hazards, stalls, MMU/ASID)
│
├── cache-controller/                # Crate: Jerarquía de caché multinivel y memoria virtual
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   # Re-exports públicos del controlador de memoria y paginación
│       ├── storage.rs               # Líneas, bloques, ControladorMemoria (L1) y NivelL2
│       ├── policy.rs                # Políticas de reemplazo LRU y Write-Back en L1
│       ├── hierarchy.rs             # JerarquiaCache: orquestador L1 + L2 + RAM
│       ├── paginacion.rs            # Fase 2: EntradaPagina, TablaDePaginas, Tlb, Mmu, AMAT extendido
│       ├── bus.rs                   # Operaciones de lectura, escritura y flush de sincronización
│       ├── main.rs                  # Demo interactiva de la jerarquía de caché
│       └── tests.rs                 # 33 tests de caché L1/L2, TLB, Page Table y AMAT
│
└── sistema-integrado/               # Crate ejecutable: Integración final y suite de benchmarks
    ├── Cargo.toml
    └── src/
        ├── main.rs                  # Runner que ejecuta los 6 programas de prueba sobre la CPU y MMU
        ├── ejemplos.rs              # Catálogo de 6 programas didácticos con metadatos y configuración ASID
        └── display.rs               # Reporte detallado de métricas (CPI, IPC, AMAT extendido, TLB, Caché)
```

---

## 5. Cómo Ejecutar el Proyecto

```bash
# Ejecutar la simulación completa con los 6 programas y reporte de rendimiento (AMAT, TLB, CPI, IPC):
cargo run -p sistema-integrado

# Ejecutar la demo interactiva de la jerarquía de caché (L1 + L2 + RAM):
cargo run -p cache-controller

# Ejecutar la demo de la CPU segmentada:
cargo run -p cpu-pipeline

# Ejecutar los 77 tests de todo el workspace:
cargo test --workspace

# Compilar todo el workspace y verificar ausencia de advertencias:
cargo build

# Generar y abrir la documentación web local de todos los crates:
cargo doc --workspace --open
```

---

## 6. Convenciones de Código y Documentación

| Convención | Descripción |
|---|---|
| `///` Doc comments | Todos los tipos y funciones públicas tienen doc comments en formato Rust standard (compatibles con `cargo doc`). |
| `wrapping_add` / `wrapping_sub` | Aritmética modular para simular overflow de hardware sin panics. |
| `Option<u16>` en `resultado` | Distingue entre "dato aún no disponible" (`None`) y "dato calculado" (`Some(v)`). Es el mecanismo central del forwarding. |
| `Copy` en `Instruccion` y `RegistroSegmentacion` | Evita complejidad de ownership al copiar instrucciones entre etapas, igual que el hardware copia bits entre flip-flops. |
| `R0` como hardwired zero | Escrituras sobre `R0` se descartan; siempre vale `0`. Estándar en ISAs RISC. |
| Evaluación en reversa en `ciclo_reloj` | Procesar `WB → MEM → EX → ID → IF` garantiza que no se sobreescriban buffers del ciclo anterior antes de leerlos. |
| `Default` en `MemoriaProvisoria` | Convencion idiomática de Rust para tipos con construcción vacía bien definida. |
| Tests unitarios | Módulo `#[cfg(test)] mod tests` en `lib.rs`, verificando casos borde de hazards y forwarding (ej: validación de no-anticipación de valores fantasma sobre `R0`). |

---

## 7. Fase 2 — Memoria Virtual (TLB, Page Table, ASID, AMAT Extendido)

### 7.1. Decisiones de Diseño

#### ¿Por qué 256 páginas virtuales y solo 16 marcos físicos no es una limitación?

Con una dirección virtual de 16 bits y páginas de 256 bytes, el espacio virtual tiene 256 páginas posibles (VPN de 8 bits). La RAM física de 4096 bytes dividida en marcos de 256 bytes da exactamente 16 marcos físicos. La desproporción **16 marcos vs. 256 páginas virtuales es el punto central de la memoria virtual**: permite que múltiples procesos (o un proceso grande) presenten un espacio de direcciones más amplio que la memoria física disponible. El hardware (MMU) y el software (OS) colaboran para mantener en RAM solo las páginas actualmente necesarias, intercambiando el resto a disco.

#### ¿Cómo se eligió la política de reemplazo de página, y por qué es el mismo patrón que L1/L2?

Se usó **LRU basado en `ultimo_acceso`** — exactamente el mismo campo y la misma lógica que `elegir_via_victima` en L1 y en L2. Es el mismo problema de fondo: dado un conjunto de slots con capacidad limitada, elegir cuál desalojar cuando llega un elemento nuevo. Al usar el mismo patrón en las 4 capas (TLB, Page Table, L1, L2), el código es predecible y auditable. No es una coincidencia: es una decisión de diseño que muestra cómo el principio de LRU aparece en todos los niveles de la jerarquía de memoria.

#### ¿Qué hace la CPU cuando ocurre un page fault?

Cuando `traducir_direccion` devuelve `PageFault`, la `Mmu` instala la página nueva (o reemplaza una víctima LRU) y **reintenta la traducción**, que ahora tiene éxito. Esto modela el comportamiento real de un OS: luego de resolver el page fault, la instrucción que falló se reintenta automáticamente. El costo en ciclos (`penalidad_page_fault = 1_000_000`) ya quedó acumulado en `contador_ciclos`, haciéndolo visible en el AMAT extendido. Se eligió este comportamiento (reintentar en lugar de devolver 0) porque hace que los tests de integración sean correctos (el dato escrito con STORE sí persiste) y modela con mayor fidelidad el hardware real.

#### ¿Por qué el ASID se modela como un campo externo que cambia el runner?

La CPU simula **un proceso a la vez**. El ASID no forma parte de cada instrucción `LOAD`/`STORE` porque en hardware real tampoco lo está: el ASID es un registro del procesador que el OS actualiza en los cambios de contexto. Modelarlo como `mmu.asid_actual` (que el runner modifica entre corridas) replica ese contrato exacto: el hardware expone el mecanismo, el software (OS/runner) decide cuándo cambiar de proceso.

#### ¿Por qué `Mmu::leer_byte`/`escribir_byte` delegan en `JerarquiaCache`?

Separación de responsabilidades: `JerarquiaCache` resuelve _dónde está el dato_ dada una dirección **física**. `Mmu` resuelve _qué dirección física_ corresponde a una dirección virtual. Las dos responsabilidades son ortogonales. Si la `Mmu` reimplementara la lógica de L1/L2/RAM, cualquier cambio en la política de cache requeriría modificar dos lugares. Al delegar, garantizamos que toda la Fase 1 sigue funcionando sin cambios — los 33 tests de `cache-controller` lo verifican.

### 7.2. Parámetros de la Fase 2

| Parámetro | Valor | Justificación |
|---|---|---|
| Tamaño de página | 256 bytes | 8 bits de offset, alineado con dirección virtual de 16 bits |
| Marcos físicos | 16 | `TAMANO_RAM / TAMANO_PAGINA = 4096 / 256` |
| Páginas virtuales | 256 | `2^8`, los 8 bits altos de la dirección virtual de 16 bits |
| Entradas TLB | 8 | Totalmente asociativa, política LRU con `VecDeque<usize>` |
| `penalidad_tlb_miss` | 10 ciclos | Costo de consultar la Page Table en RAM |
| `penalidad_page_fault` | 1_000_000 ciclos | ~10 ms a 100 MHz (escala de juguete, pero representa el orden de magnitud real) |

### 7.3. Fórmula AMAT Extendido

```
AMAT_TLB   = T_TLB + TasaMiss_TLB × (T_PageTable + TasaPageFault × Penalidad_PageFault)
AMAT_total = AMAT_TLB + AMAT_L1

donde AMAT_L1 = T_L1 + TasaMiss_L1 × (T_L2 + TasaMiss_L2 × T_RAM)
```

### 7.4. Estructura del módulo `paginacion`

```
cache-controller/src/paginacion.rs
├── descomponer_direccion_virtual(u16) → (vpn: u8, offset: u8)
├── reconstruir_direccion_fisica(marco: u8, offset: u8) → u16
├── TablaDePaginas { entradas: Vec<EntradaPagina>, marcos_ocupados: [Option<u16>; 16] }
│   ├── buscar_marco_libre() → Option<u8>
│   └── elegir_marco_victima() → u8   [LRU]
├── Tlb { entradas, capacidad, hits, misses, orden_uso: VecDeque<usize> }
│   ├── buscar(vpn, asid, ciclo) → Option<u8>   [hit si vpn+asid coinciden]
│   └── insertar(vpn, asid, marco, ciclo)        [LRU eviction si TLB llena]
├── traducir_direccion(mmu, dir, tipo) → ResultadoTraduccion
│   ├── TLB hit → Exitosa (sin penalidad)
│   ├── TLB miss → Page Table lookup (+penalidad_tlb_miss)
│   │   ├── Página presente → Exitosa + insertar en TLB
│   │   ├── Escritura RO → ViolacionProteccion
│   │   └── No presente → PageFault + reemplazo LRU + instalar página
└── Mmu { tlb, page_table, jerarquia, asid_actual, ... }
    ├── leer_byte(vaddr, tipo) → Option<u8>   [reintenta tras page fault]
    └── escribir_byte(vaddr, dato) → bool     [reintenta tras page fault]
```

### 7.5. Tests de Fase 2

| Test | Crate | Qué verifica |
|---|---|---|
| `test_descomponer_direccion_virtual_vpn_y_offset_correctos` | `cache-controller` | Descomposición VPN/offset y reconstrucción física |
| `test_traduccion_pagina_presente_sin_pasar_por_tlb` | `cache-controller` | TLB miss → Page Table hit → éxito; 2do acceso es TLB hit |
| `test_traduccion_pagina_no_presente_dispara_page_fault` | `cache-controller` | Dirección sin mapear → PageFault |
| `test_page_fault_con_marco_libre_no_desaloja_nada` | `cache-controller` | Page fault con marco libre: 15 quedan libres |
| `test_page_fault_sin_marcos_libres_desaloja_lru` | `cache-controller` | 17ma página fuerza reemplazo LRU de VPN=0 |
| `test_escritura_en_pagina_solo_lectura_devuelve_violacion` | `cache-controller` | STORE en página RO → ViolacionProteccion; lectura sigue OK |
| `test_mmu_leer_byte_genera_page_fault_y_devuelve_cero` | `cache-controller` | `leer_byte` maneja PageFault y acumula penalidad en ciclos |
| `test_mmu_escribir_byte_en_pagina_solo_lectura_devuelve_false` | `cache-controller` | `escribir_byte` deniega escrituras sobre páginas de solo lectura |
| `test_tlb_hit_requiere_mismo_asid` | `cache-controller` | Hit solo si VPN y ASID coinciden exactamente |
| `test_tlb_miss_con_distinto_asid_mismo_vpn` | `cache-controller` | ASID=1 no ve traducción de ASID=0 para la misma VPN |
| `test_amat_incluye_penalidad_de_page_fault` | `cache-controller` / `sistema-integrado` | Fórmula AMAT con page fault produce ~50007 ciclos |
| `test_integracion_cpu_dispara_page_fault` | `cpu-pipeline` | 17 LOADs a 17 páginas virtuales → `mmu.page_faults >= 17` |
| `test_integracion_asid_aislamiento_de_traducciones` | `cpu-pipeline` | Procesos con ASID 1 y ASID 2 aislados con la misma dirección virtual |

> **Cobertura total del workspace**: 77 pruebas unitarias y de integración pasando exitosamente (`33` en `cache-controller`, `42` en `cpu-pipeline` y `2` en `sistema-integrado`).

---

## 8. Catálogo de Benchmarks del Sistema Integrado

El crate `sistema-integrado` ejecuta 6 escenarios didácticos que ponen a prueba todas las capas del hardware simulado:

| Programa | Fenómeno Analizado | Componentes Evaluados |
|---|---|---|
| **1. Load-Use Hazard + Cache Hit** | Stall de 1 ciclo por lectura pendiente de RAM y posterior acierto en caché al re-leer el bloque. | Hazard Unit + Forwarding + L1 Cache Hit |
| **2. Aritmética Pura (ALU)** | Operaciones consecutivas `ADD` y `SUB` sin stalls gracias al forwarding completo EX/MEM y MEM/WB. | Forwarding Unit (rendimiento óptimo CPI ≈ 1) |
| **3. Write-Back y Desalojo Dirty** | Múltiples escrituras y lecturas forzando el desalojo de una línea sucia de L1 hacia L2 y RAM. | Políticas Write-Back y Write-Allocate multinivel |
| **4. Salto Incondicional (`JUMP`)** | Branch penalty de 2 ciclos con vaciado (*flush*) de instrucciones especulativas en IF/ID e ID/EX. | Control Hazards + Redirección de PC |
| **5. Memoria Virtual: Page Faults y LRU** | Accesos a más de 16 páginas virtuales forzando desalojo de marcos físicos por LRU e invalidación en TLB. | MMU + Page Table + Reemplazo de marcos LRU |
| **6. Aislamiento por ASID** | Dos procesos (ASID 1 y ASID 2) escribiendo en la misma dirección virtual `0x0100` sin colisiones de memoria física ni traducciones cruzadas en la TLB. | TLB Tagging con ASID + Separación de procesos |

