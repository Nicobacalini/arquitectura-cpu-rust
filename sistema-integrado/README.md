# Proyecto 3: Sistema Integrado (CPU Segmentada + Controlador de Caché)

> Integración completa de una CPU segmentada de 5 etapas con un controlador de memoria caché asociativa por conjuntos (2 vías, LRU, Write-Back / Write-Allocate).

---

## 1. Contexto y Objetivo

En los proyectos anteriores implementamos y testeamos de forma aislada:
1. **`cpu-pipeline` (Proyecto 1):** Un procesador de 5 etapas (IF, ID, EX, MEM, WB) con resolución de hazards de datos mediante **forwarding**, detección de **Load-Use hazards** (con inyección de burbujas/stalls) y penalización de saltos (`JUMP`).
2. **`cache-controller` (Proyecto 2):** Un subsistema de memoria con RAM de 256 bytes intermediada por una **caché asociativa por conjuntos de 2 vías** (4 conjuntos × 2 vías = 32 bytes), con reemplazo **LRU**, política **Write-Back** y **Write-Allocate**.

El objetivo de este proyecto (`sistema-integrado`) es **unir ambos subsistemas en una máquina unificada**. La memoria provisoria del pipeline se reemplaza por el controlador de caché real: cada instrucción `LOAD` o `STORE` que llega a la etapa **MEM** interactúa directamente con la jerarquía de memoria caché/RAM, registrando aciertos (*hits*), fallos (*misses*) y tráfico hacia la memoria principal.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                       CPU SEGMENTADA (5 ETAPAS)                         │
│                                                                         │
│   ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      │
│   │  IF  │ ───▶ │  ID  │ ───▶ │  EX  │ ───▶ │ MEM  │ ───▶ │  WB  │      │
│   └──────┘      └──────┘      └──────┘      └──────┘      └──────┘      │
│    Fetch         Decode        ALU          Memoria       Write Back    │
└────────────────────────────────────────────────┬────────────────────────┘
                                                 │
                                                 │ leer_byte(dir)
                                                 │ escribir_byte(dir, dato)
                                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    CONTROLADOR DE MEMORIA CACHÉ                         │
│                                                                         │
│   ┌─────────────────────────────────────────────────────────────────┐   │
│   │ CACHÉ L1 ASOCIATIVA POR CONJUNTOS (4 Sets × 2 Vías = 32 bytes)  │   │
│   │ Política: Write-Back, Write-Allocate, Reemplazo LRU             │   │
│   └────────────────────────────────┬────────────────────────────────┘   │
│                                    │                                    │
│                                    │ Tráfico en Miss y Desalojo Dirty   │
│                                    ▼                                    │
│   ┌─────────────────────────────────────────────────────────────────┐   │
│   │                      MEMORIA RAM PRINCIPAL                      │   │
│   │                     (256 bytes direccionables)                  │   │
│   └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Arquitectura de la Interconexión

### 2.1 Conexión de la etapa MEM con el Bus de Caché

En `cpu-pipeline/src/lib.rs`, la etapa `ejecutar_mem` recibe una referencia mutable al subsistema de memoria:

```rust
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
            // Lee a través de la caché: hit devuelve el byte en 1 ciclo,
            // miss carga el bloque de 4 bytes desde RAM.
            let dato = memoria.leer_byte(direccion_ram);
            RegistroSegmentacion {
                resultado: Some(dato as u16),
                ..instruccion
            }
        }
        Instruccion::STORE { direccion_ram, .. } => {
            // Escribe en caché: Write-Allocate asegura que el bloque esté cargado,
            // Write-Back marca dirty_bit = true sin tocar la RAM.
            if let Some(valor) = instruccion.resultado {
                memoria.escribir_byte(direccion_ram, valor as u8);
            }
            instruccion
        }
        _ => instruccion,
    }
}
```

### 2.2 Sincronización al Drenar el Pipeline (`flush`)

Debido a que la caché opera con política **Write-Back**, las escrituras realizadas por las instrucciones `STORE` residen inicialmente solo en las líneas de la caché con su bit `dirty_bit = true`.
Al completarse la ejecución del programa y vaciarse el pipeline, el simulador invoca:

```rust
memoria.flush();
```

Esto garantiza que todas las líneas sucias sean volcadas a la memoria RAM sin invalidarlas, asegurando la consistencia física de la memoria.

### 2.3 Módulo de Reporte y Métricas de Rendimiento (`display.rs`)

Para mantener el principio de responsabilidad única y desacoplar la simulación del formateo y análisis de resultados, la lógica de presentación vive en [`src/display.rs`](src/display.rs) mediante la función:

```rust
pub fn reporte_rendimiento(
    cpu: &CpuSegmentada,
    mem: &ControladorMemoria,
    frecuencia_mhz: f64,
) -> String
```

Este módulo procesa los contadores internos de la CPU y de la memoria para derivar indicadores estándar de rendimiento:
* **CPI (Ciclos por Instrucción):** Relación entre los ciclos totales del procesador y las instrucciones reales retiradas en la etapa WB.
* **IPC (Instrucciones por Ciclo):** Inverso del CPI, representa el *throughput* efectivo de la CPU.
* **Tiempo de Ejecución Estimado:** Proyectado a partir de una frecuencia de reloj configurable (en MHz).
* **Tasa de Aciertos de Caché (%):** Porcentaje de accesos a memoria resueltos directamente en la caché L1 sin requerir acceso a RAM.

---

## 3. Programas de Demostración

El binario principal [`src/main.rs`](src/main.rs) itera sobre el catálogo definido en [`src/ejemplos.rs`](src/ejemplos.rs) y ejecuta cada ejemplo en secuencia. Para agregar un nuevo ejemplo basta con editar solo `ejemplos.rs`.

Cada ejemplo se define mediante la estructura `Ejemplo`:

```rust
pub struct Ejemplo {
    pub nombre: &'static str,
    pub descripcion: &'static str,
    /// Valores iniciales [R0, R1, R2, R3]
    pub registros_iniciales: [u16; 4],
    /// Precarga de RAM: lista de (dirección, valor)
    pub ram_inicial: &'static [(u8, u8)],
    pub programa: fn() -> Vec<Instruccion>,
}
```

La CPU se inicializa con la **sintaxis de actualización de Rust (`..`)**:

```rust
let mut cpu = CpuSegmentada {
    registros: ej.registros_iniciales,
    ..CpuSegmentada::nueva()  // pipeline en NOP, PC en 0, contadores en 0
};
```

### Ejemplo 1 — Load-Use Hazard + Cache Hit

**Estado inicial:** `R2=10`, `RAM[0x10]=15`

```
[0] LOAD R1,0x10    → R1 = 15        (Miss de caché)
[1] ADD  R2,R1,R2   → R2 = 15+10=25  (Load-Use Hazard: stall 1 ciclo)
[2] STORE R2,0x20   → RAM[0x20] = 25 (Miss + Write-Allocate)
[3] LOAD R3,0x20    → R3 = 25        (Hit: mismo bloque recién cargado)
[4] SUB  R3,R3,R1   → R3 = 25-15=10  (Load-Use Hazard adicional)
```

**Resultado:** `R1=15  R2=25  R3=10` | **CPI:** 2.20 | **Cache:** 1 hit / 2 misses (33%)

### Ejemplo 2 — Aritmética pura (sin memoria)

**Estado inicial:** `R1=10`, `R2=20`

```
[0] ADD R3,R1,R2   → R3 = 10+20=30
[1] SUB R2,R3,R1   → R2 = 30-10=20
[2] ADD R1,R2,R1   → R1 = 20+10=30
[3] ADD R3,R3,R2   → R3 = 30+20=50
[4] SUB R2,R1,R2   → R2 = 30-20=10
```

Solo instrucciones ALU: el forwarding elimina todos los stalls. La caché no se ejercita.

**Resultado:** `R1=30  R2=10  R3=50` | **CPI:** 1.80 | **Cache:** 0 accesos

### Ejemplo 3 — JUMP con penalización de pipeline

**Estado inicial:** `R1=5`, `R2=3`

```
[0] ADD  R1,R1,R2  → R1 = 5+3=8      (ejecutada)
[1] JUMP 0x04      → salta a I4       (flush de I2 e I3)
[2] ADD  R2,R2,R1  → (DESCARTADA)
[3] SUB  R3,R3,R2  → (DESCARTADA)
[4] ADD  R3,R1,R3  → R3 = 8+0=8      (primera post-salto)
[5] STORE R3,0x30  → RAM[0x30] = 8
```

El JUMP se resuelve en EX. Para ese momento ya entraron I2 e I3 al pipeline — ambas se descartan con flush (branch penalty: 2 ciclos). El contador de instrucciones completadas marca **4**, no 6.

**Resultado:** `R1=8  R3=8` | **CPI:** 2.50 | **Instrucciones completadas:** 4

### Ejemplo 4 — Múltiples accesos a memoria (Hit/Miss)

**Estado inicial:** `RAM[0x10]=5`, `RAM[0x11]=8`

```
[0] LOAD R1,0x10   → R1 = 5   (Miss: carga el bloque 0x10..0x13)
[1] LOAD R2,0x11   → R2 = 8   (Hit: 0x11 está en el mismo bloque que 0x10)
[2] ADD  R3,R1,R2  → R3 = 13  (Load-Use Hazard desde I1)
[3] STORE R3,0x20  → RAM[0x20] = 13
[4] LOAD R1,0x20   → R1 = 13  (Hit: dato recién escrito, aún en caché)
[5] SUB  R2,R1,R2  → R2 = 5   (Load-Use Hazard desde I4)
[6] STORE R2,0x21  → RAM[0x21] = 5
```

Demuestra localidad espacial (0x10 y 0x11 en el mismo bloque), Write-Back y re-lectura de datos propios.

**Resultado:** `R1=13  R2=5  R3=13` | **CPI:** 1.86 | **Cache:** 3 hits / 2 misses (60%)

---

## 4. Traza de Ejecución — Ejemplo 1

La traza completa del Ejemplo 1 (Load-Use Hazard + Cache Hit) es la siguiente:

```text
Ciclo 1  | IF/ID: LOAD R1,0x10 | ID/EX: --            | EX/MEM: --            | MEM/WB: --
Ciclo 2  | IF/ID: ADD R2,R1,R2 | ID/EX: LOAD R1,0x10  | EX/MEM: --            | MEM/WB: --
Ciclo 3  | IF/ID: ADD R2,R1,R2 | ID/EX: --            | EX/MEM: LOAD R1,0x10  | MEM/WB: --
Ciclo 4  | IF/ID: STORE R2,0x20| ID/EX: ADD R2,R1,R2  | EX/MEM: --            | MEM/WB: LOAD R1,0x10
Ciclo 5  | IF/ID: LOAD R3,0x20 | ID/EX: STORE R2,0x20 | EX/MEM: ADD R2,R1,R2  | MEM/WB: --
Ciclo 6  | IF/ID: SUB R3,R3,R1 | ID/EX: LOAD R3,0x20  | EX/MEM: STORE R2,0x20 | MEM/WB: ADD R2,R1,R2
Ciclo 7  | IF/ID: SUB R3,R3,R1 | ID/EX: --            | EX/MEM: LOAD R3,0x20  | MEM/WB: STORE R2,0x20
Ciclo 8  | IF/ID: --           | ID/EX: SUB R3,R3,R1  | EX/MEM: --            | MEM/WB: LOAD R3,0x20
Ciclo 9  | IF/ID: --           | ID/EX: --            | EX/MEM: SUB R3,R3,R1  | MEM/WB: --
Ciclo 10 | IF/ID: --           | ID/EX: --            | EX/MEM: --            | MEM/WB: SUB R3,R3,R1
Ciclo 11 | IF/ID: --           | ID/EX: --            | EX/MEM: --            | MEM/WB: --
```

### Análisis de eventos clave:

| Ciclo | Etapa | Evento en Pipeline | Evento en Caché |
|:---:|:---:|---|---|
| **1** | IF | Fetch de `LOAD R1,0x10`. | — |
| **2** | ID / EX | `LOAD` pasa a ID/EX. Se busca `ADD`. La CPU detecta que `ADD` usa R1. | — |
| **3** | **HAZARD / MEM** | **Load-Use Stall:** burbuja en `ID/EX`. `IF/ID` y `PC` congelados. | `LOAD` en MEM: **Cache Miss** en `0x10`. Bloque cargado desde RAM. |
| **4** | WB / EX | `LOAD` consolida en WB (`R1=15`). `ADD` avanza a EX. | — |
| **5** | EX / MEM | `ADD` calcula `R2 = 15+10 = 25`. | — |
| **6** | MEM | `STORE R2,0x20` con valor `25`. | **Cache Miss** (Write-Allocate): carga bloque `0x20`, escribe `25`, `dirty_bit=true`. |
| **7** | **HAZARD / MEM** | **Segundo Load-Use Stall:** `SUB` necesita `R3` que recién llega de MEM. | `LOAD R3,0x20`: **Cache Hit** (bloque cargado en ciclo anterior). |
| **8** | WB / EX | `LOAD` escribe `R3=25`. `SUB` avanza a EX. | — |
| **9–11** | Drenado | `SUB` calcula `25-15=10`, avanza y consolida en WB. Pipeline vacío en ciclo 11. | — |

---

## 5. Traza de Ejecución — Ejemplo 3 (JUMP)

La traza del Ejemplo 3 ilustra visualmente el branch penalty:

```text
Ciclo 1 | IF/ID: ADD R1,R1,R2  | ID/EX: --           | EX/MEM: --           | MEM/WB: --
Ciclo 2 | IF/ID: JUMP 0x04     | ID/EX: ADD R1,R1,R2 | EX/MEM: --           | MEM/WB: --
Ciclo 3 | IF/ID: ADD R2,R2,R1  | ID/EX: JUMP 0x04    | EX/MEM: ADD R1,R1,R2 | MEM/WB: --
Ciclo 4 | IF/ID: --            | ID/EX: --            | EX/MEM: JUMP 0x04    | MEM/WB: ADD R1,R1,R2
Ciclo 5 | IF/ID: ADD R3,R1,R3  | ID/EX: --            | EX/MEM: --           | MEM/WB: JUMP 0x04
Ciclo 6 | IF/ID: STORE R3,0x30 | ID/EX: ADD R3,R1,R3 | EX/MEM: --           | MEM/WB: --
...
```

En el **Ciclo 3**, `ADD R2,R2,R1` (I2) ya entró al pipeline. El JUMP llega a EX en el **Ciclo 4** y dispara el flush: tanto `if_id` (I2) como `id_ex` (vacío especulativo) se convierten en burbujas `--`. El `PC` salta a 4. En el **Ciclo 5** comienza a fluir `ADD R3,R1,R3` (I4) como primera instrucción legítima post-salto.

---

## 6. Reporte de Rendimiento Comparativo

| Métrica | Ej. 1 (Hazards) | Ej. 2 (ALU pura) | Ej. 3 (JUMP) | Ej. 4 (Memoria) |
|---|:---:|:---:|:---:|:---:|
| Ciclos totales | 11 | 9 | 10 | 13 |
| Instrucciones completadas | 5 | 5 | 4 | 7 |
| CPI | 2.20 | **1.80** | 2.50 | 1.86 |
| IPC | 0.45 | 0.56 | 0.40 | 0.54 |
| Tiempo @ 100 MHz | 110 ns | 90 ns | 100 ns | 130 ns |
| Cache hits | 1 | 0 | 0 | 3 |
| Cache misses | 2 | 0 | 1 | 2 |
| Tasa de aciertos | 33% | — | — | **60%** |

**Observaciones:**
- El Ejemplo 2 tiene el **CPI más bajo (1.80)** porque no tiene accesos a memoria ni stalls: solo forwarding entre instrucciones ALU consecutivas.
- El Ejemplo 3 tiene el **CPI más alto (2.50)** por la penalidad del JUMP (2 ciclos de flush) sobre solo 4 instrucciones completadas.
- El Ejemplo 4 tiene la **tasa de aciertos más alta (60%)** gracias a la localidad espacial (0x10 y 0x11 en el mismo bloque) y al Write-Back que preserva datos propios en caché.

---

## 7. Decisiones de Diseño

### 7.1 Interacción entre Stalls del Pipeline y la Latencia de Caché

En una CPU real, un fallo de caché (*cache miss*) hacia la memoria RAM toma decenas o cientos de ciclos. En este simulador pedagógico:
- La lógica de detección de hazards del pipeline desacopla el control de datos: el **Load-Use stall** de 1 ciclo resuelve la dependencia temporal inherente al datapath (el dato no está disponible hasta el final de MEM).
- Si la caché produjera un retardo variable por miss a RAM, la CPU podría congelarse agregando ciclos de *memory stall* sin alterar la corrección del forwarding ni la detección de riesgos.

### 7.2 Eficiencia de Write-Back con Write-Allocate

- Si hubiésemos utilizado **Write-Through**, la instrucción `STORE` del Ejemplo 1 (ciclo 6) habría forzado una escritura síncrona a la memoria RAM externa.
- Con **Write-Back**, la CPU escribió el dato en la caché inmediatamente. Al requerir la instrucción siguiente (`LOAD R3,0x20`) ese mismo dato, se obtuvo un **Hit** directo en caché sin que la RAM externa interviniera.

### 7.3 Separación de responsabilidades: `ejemplos.rs` vs `main.rs`

Los programas de ejemplo se definen íntegramente en [`src/ejemplos.rs`](src/ejemplos.rs) a través de la función pública `catalogo()`. `main.rs` se limita a iterar sobre ese catálogo e invocar `ejecutar_ejemplo()`. Esto garantiza que:
- Agregar un nuevo ejemplo **no requiere modificar `main.rs`**.
- La lógica de ejecución (loop de ciclos, flush, reporte) está en un único lugar reutilizable.
- Cada módulo tiene una única razón para cambiar (*single responsibility*).

---

## 8. Compilación y Ejecución

Para compilar y ejecutar la simulación integrada (todos los ejemplos en secuencia):

```bash
cargo run --package sistema-integrado --bin sistema-integrado
```

Para verificar la integridad de todos los tests unitarios del espacio de trabajo completo:

```bash
cargo test --workspace
```

| Crate | Tests | Cobertura |
|---|---|---|
| `cache-controller` | 15 | Políticas LRU, Write-Back, Write-Allocate, decodificación de direcciones |
| `cpu-pipeline` | 38 | Forwarding, Load-Use hazards, saltos, `nueva()`, sintaxis de actualización, métricas (+1 doctest) |
| `sistema-integrado` | 1 | Cálculo y formateo del reporte de rendimiento (CPI, IPC, tiempos, caché) |
| **Total** | **54** | **100% pasando** |

---

## 9. Estructura de Archivos

```text
sistema-integrado/
├── Cargo.toml       # Declara dependencias hacia cpu-pipeline y cache-controller
├── README.md        # Documentación de la arquitectura integrada y resultados
└── src/
    ├── display.rs   # Módulo de formateo y cálculo de métricas (CPI, IPC, etc.)
    ├── ejemplos.rs  # Catálogo de programas de ejemplo (struct Ejemplo + catalogo())
    └── main.rs      # Binario: itera el catálogo y ejecuta cada ejemplo
```
