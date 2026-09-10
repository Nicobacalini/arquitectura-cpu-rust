# Arquitectura CPU en Rust

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
| **Espacio de Direccionamiento RAM** | `[u8; 256]` | **8 bits** (`u8`) | **256 bytes** | Rango `0x00` a `0xFF` direccionable por instrucciones `LOAD`/`STORE`. |
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
* `LOAD { dest: Registro, direccion_ram: u8 }`: Carga un dato de RAM en un registro (`dest = RAM[direccion]`).
* `STORE { src: Registro, direccion_ram: u8 }`: Almacena el contenido de un registro en RAM (`RAM[direccion] = src`).
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
}
```

### `pub struct MemoriaProvisoria`
Modela la RAM principal del sistema como un arreglo plano de 256 bytes, direccionables con un índice `u8` (rango `0x00`..`0xFF`):
```rust
pub struct MemoriaProvisoria {
    pub ram: [u8; 256],
}
```
| Método | Firma | Descripción |
|--------|-------|-------------|
| `new()` | `fn new() -> Self` | Inicializa la RAM con todos los bytes en `0`. |
| `leer_byte` | `fn leer_byte(&self, direccion: u8) -> u8` | Lee el byte en `ram[direccion]`. |
| `escribir_byte` | `fn escribir_byte(&mut self, direccion: u8, dato: u8)` | Escribe `dato` en `ram[direccion]`. |

Implementa también `Default`, que es equivalente a llamar `MemoriaProvisoria::new()` y es la convención idiomática en Rust para tipos con una construcción vacía bien definida.

---


## 3. Diagrama General de la Arquitectura y Ruta de Datos (Datapath)

```text
                                +--------------------------- Camino de WB (Dato escrito) ------------------------------+
                                |                                                                                       |
                                v                                                                                       |
  [ PC ] ---> [ Mem. Prog ] ---> | IF/ID | ---> [ Banco Regs ] ---> | ID/EX | ---> [ MUX ] ---> [  ALU  ] ---> | EX/MEM | ---> [ Caché / RAM ] ---> | MEM/WB | --+
    ^              |                 |               |                 |             ^           ^                |                |                  |
    |              v                 |               v                 |             |           |                |                v                  |
    |         (Instrucción)          |          (Lectura R)            |             |           v                |             (Dato leído)          |
    |                                |                                 |             |        [ MUX ]             |                                   |
    |                                |                                 |             |           ^                |                                   |
    +----[ Hazard Detection Unit ]<--+                                 |             |           |                |                                   |
    |      - Congela PC              |                                 |             |           |                |                                   |
    |      - Congela IF/ID           +---------------------------------+             |           |                |                                   |
    |      - Inserta burbuja (NOP)                                                   |           |                |                                   |
    +-----> en ID/EX                                                                 |           |                |                                   |
                                                                                     |           |                |                                   |
                                             +---------------------------------------+-----------+                |                                   |
                                             |                                                                    |                                   |
                                             |                     Unidad de Forwarding                           |                                   |
                                             |     (Resuelve operandos anticipando desde EX/MEM y MEM/WB)         |                                   |
                                             +--------------------------------------------------------------------+                                   |
                                                                 ^                                                                                    |
                                                                 |------------- Forwarding desde EX/MEM ------------------+                           |
                                                                 |                                                        |                           |
                                                                 +------------- Forwarding desde MEM/WB --------------------------------------------+
```

---

## 4. Estructura del Proyecto Workspace

```text
arquitectura-cpu-rust/
├── Cargo.toml                       # Configuración raíz del Cargo Workspace
├── README.md                        # Documentación teórica y técnica del sistema
│
├── cpu-pipeline/                    # Crate: Simulación del pipeline del procesador
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   # API pública: MemoriaProvisoria, CpuSegmentada, ISA,
│       │                            # pipeline, hazard detection, forwarding, Display
│       └── main.rs                  # Binario de demo: instancia una CPU y corre un programa
│       └── tests.rs                 # tests de ejemplo importando todo desde cpu_pipeline::*
│
├── cache-controller/                # Crate: Controlador de memoria caché (pendiente — Proyecto 2)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   # Definición de la estructura CacheController (pendiente)
│       ├── storage.rs               # Representación de líneas, bloques y tags (pendiente)
│       ├── policy.rs                # Políticas de reemplazo (LRU/FIFO) y escritura (pendiente)
│       └── bus.rs                   # Interfaz de bus de memoria y penalizaciones (pendiente)
│
└── sistema-integrado/               # Crate ejecutable: Integración final y driver (pendiente — Integración)
    ├── Cargo.toml
    └── src/
        ├── main.rs                  # Bucle de simulación ciclo a ciclo (pendiente)
        └── display.rs               # Visualizador en consola del estado del pipeline (pendiente)
```

---

## 5. Cómo Ejecutar el Proyecto

```bash
# Compilar y ejecutar el crate cpu-pipeline
cargo run -p cpu-pipeline

# Ejecutar todos los tests del workspace
cargo test

# Compilar todo el workspace y verificar que no hay errores
cargo build

# Ver la documentación generada a partir de los doc comments (///)
cargo doc --open
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
