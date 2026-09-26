# Sistema Integrado — CPU Segmentada + Jerarquía de Caché

> Integración completa de una CPU segmentada de 5 etapas con una jerarquía de memoria caché de dos niveles (L1 → L2 → RAM), política LRU, Write-Back / Write-Allocate.

---

## 1. Contexto y Objetivo

En los proyectos anteriores implementamos y testeamos de forma aislada:

1. **`cpu-pipeline`:** CPU de 5 etapas (IF, ID, EX, MEM, WB) con forwarding, Load-Use hazards y penalización de JUMP.
2. **`cache-controller`:** Jerarquía de dos niveles:
   - **L1:** 4 conjuntos × 2 vías, 32 bytes efectivos, tag=12b (decodificador propio).
   - **L2:** 8 conjuntos × 2 vías, 64 bytes efectivos, tag=11b (decodificador propio).
   - **Write-back en cadena:** desalojo dirty de L1 → L2; desalojo dirty de L2 → RAM.

El objetivo de `sistema-integrado` es **unir ambos subsistemas en una máquina unificada**. Cada instrucción `LOAD` o `STORE` en la etapa MEM pasa por la jerarquía real de caché, registrando hits, misses y tráfico hacia la RAM.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                       CPU SEGMENTADA (5 ETAPAS)                         │
│                                                                         │
│   ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      ┌──────┐      │
│   │  IF  │ ───▶ │  ID  │ ───▶ │  EX  │ ───▶ │ MEM  │ ───▶ │  WB  │      │
│   └──────┘      └──────┘      └──────┘      └──────┘      └──────┘      │
│    Fetch         Decode        ALU          Memoria       Write Back    │
└────────────────────────────────────────────────┬────────────────────────┘
                                                 │ leer_byte(dir: u16)
                                                 │ escribir_byte(dir: u16, dato: u8)
                                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                   CONTROLADOR DE MEMORIA (ControladorMemoria)           │
│                                                                         │
│   ┌─────────────────────────────────────────────────────────────────┐   │
│   │  CACHÉ L1 — 4 Sets × 2 Vías = 32 bytes   tag=12b / idx=2b      │   │
│   │  Write-Back · Write-Allocate · LRU                             │   │
│   └────────────────────────────┬────────────────────────────────────┘   │
│                                │ Miss / Desalojo Dirty                  │
│                                ▼                                        │
│   ┌─────────────────────────────────────────────────────────────────┐   │
│   │                 MEMORIA RAM PRINCIPAL                           │   │
│   │         (4096 bytes, direccionamiento u16)                      │   │
│   └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

> **Nota sobre la jerarquía de dos niveles:** `sistema-integrado` usa actualmente `ControladorMemoria` (L1 standalone) como la memoria del pipeline, al igual que antes. La `JerarquiaCache` (L1→L2→RAM con write-back en cadena) está implementada y testeada en `cache-controller` y puede integrarse en el pipeline sin cambios de firma — `leer_byte`/`escribir_byte` tienen la misma API en ambos tipos.

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
    match instruccion.instruccion {
        Instruccion::LOAD { direccion_ram, .. } => {
            // Lee a través de la caché: hit en 1 ciclo, miss carga bloque de 4 bytes
            let dato = memoria.leer_byte(direccion_ram);
            RegistroSegmentacion { resultado: Some(dato as u16), ..instruccion }
        }
        Instruccion::STORE { direccion_ram, .. } => {
            // Write-Back: escribe en caché (dirty_bit=true), no toca RAM todavía
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

Con política Write-Back, las escrituras de `STORE` residen en caché con `dirty_bit = true`. Al finalizar la ejecución, el simulador invoca:

```rust
memoria.flush();
```

Esto vuelca todas las líneas sucias a la RAM principal sin invalidarlas — equivalente a un `fsync` de nivel de aplicación.

### 2.3 Módulo de Reporte y Métricas (`display.rs`)

```rust
pub fn reporte_rendimiento(
    cpu: &CpuSegmentada,
    mem: &ControladorMemoria,
    frecuencia_mhz: f64,
) -> String
```

Indica los indicadores estándar derivados de los contadores internos:

| Métrica | Fórmula |
|---|---|
| **CPI** | `ciclos_totales / instrucciones_completadas` |
| **IPC** | `instrucciones_completadas / ciclos_totales` |
| **Tiempo estimado** | `ciclos / (frecuencia_mhz × 10⁶)` × 10⁹ ns |
| **Tasa de aciertos L1** | `hits / (hits + misses)` |

---

## 3. Programas de Demostración

El binario [`src/main.rs`](src/main.rs) itera sobre el catálogo de [`src/ejemplos.rs`](src/ejemplos.rs). Para agregar un nuevo ejemplo, solo se modifica `ejemplos.rs`.

```rust
pub struct Ejemplo {
    pub nombre:               &'static str,
    pub descripcion:          &'static str,
    pub registros_iniciales:  [u16; 4],
    pub ram_inicial:          &'static [(u8, u8)],
    pub programa:             fn() -> Vec<Instruccion>,
}
```

La CPU se inicializa con la **sintaxis de actualización de Rust**:

```rust
let mut cpu = CpuSegmentada {
    registros: ej.registros_iniciales,
    ..CpuSegmentada::nueva()  // pipeline en NOP, PC=0, contadores=0
};
```

### Ejemplo 1 — Load-Use Hazard + Cache Hit

**Estado inicial:** `R2=10`, `RAM[0x10]=15`

```
[0] LOAD R1,0x10    → R1 = 15        (Miss: carga bloque 0x10..0x13)
[1] ADD  R2,R1,R2   → R2 = 15+10=25  (Load-Use Hazard: stall 1 ciclo)
[2] STORE R2,0x20   → RAM[0x20] = 25 (Miss + Write-Allocate, dirty_bit=true)
[3] LOAD R3,0x20    → R3 = 25        (Hit: mismo bloque recién cargado)
[4] SUB  R3,R3,R1   → R3 = 25-15=10  (Load-Use Hazard adicional)
```

**Resultado:** `R1=15  R2=25  R3=10` | **CPI:** 2.20 | **Cache:** 1 hit / 2 misses (33%)

### Ejemplo 2 — Aritmética pura (sin memoria)

**Estado inicial:** `R1=10`, `R2=20`

```
[0] ADD R3,R1,R2   → R3 = 30
[1] SUB R2,R3,R1   → R2 = 20
[2] ADD R1,R2,R1   → R1 = 30
[3] ADD R3,R3,R2   → R3 = 50
[4] SUB R2,R1,R2   → R2 = 10
```

Solo instrucciones ALU: el forwarding elimina todos los stalls. La caché no se ejercita.

**Resultado:** `R1=30  R2=10  R3=50` | **CPI:** 1.80 | **Cache:** 0 accesos

### Ejemplo 3 — JUMP con penalización de pipeline

**Estado inicial:** `R1=5`, `R2=3`

```
[0] ADD  R1,R1,R2  → R1 = 8       (ejecutada)
[1] JUMP 0x04      → salta a I4   (flush de I2 e I3 → branch penalty: 2 ciclos)
[2] ADD  R2,R2,R1  → (DESCARTADA)
[3] SUB  R3,R3,R2  → (DESCARTADA)
[4] ADD  R3,R1,R3  → R3 = 8+0=8  (primera post-salto)
[5] STORE R3,0x30  → RAM[0x30] = 8
```

**Resultado:** `R1=8  R3=8` | **CPI:** 2.50 | **Instrucciones completadas:** 4 (no 6)

### Ejemplo 4 — Múltiples accesos a memoria (Hit/Miss)

**Estado inicial:** `RAM[0x10]=5`, `RAM[0x11]=8`

```
[0] LOAD R1,0x10   → R1 = 5   (Miss: carga bloque 0x10..0x13)
[1] LOAD R2,0x11   → R2 = 8   (Hit: 0x11 está en el mismo bloque que 0x10)
[2] ADD  R3,R1,R2  → R3 = 13  (Load-Use Hazard desde I1)
[3] STORE R3,0x20  → RAM[0x20] = 13
[4] LOAD R1,0x20   → R1 = 13  (Hit: dato recién escrito, aún en caché)
[5] SUB  R2,R1,R2  → R2 = 5   (Load-Use Hazard desde I4)
[6] STORE R2,0x21  → RAM[0x21] = 5
```

**Resultado:** `R1=13  R2=5  R3=13` | **CPI:** 1.86 | **Cache:** 3 hits / 2 misses (60%)

---

## 4. Traza de Ejecución — Ejemplo 1

| Ciclo | IF/ID | ID/EX | EX/MEM | MEM/WB | Evento clave |
|:---:|:---:|:---:|:---:|:---:|---|
| 1 | `LOAD R1` | `--` | `--` | `--` | Fetch de LOAD |
| 2 | `ADD R2` | `LOAD R1` | `--` | `--` | ADD entra, LOAD pasa a decode |
| 3 | `ADD R2` ❄️ | `--` | `LOAD R1` | `--` | **Stall.** LOAD en MEM: **Cache Miss** en 0x10. Bloque cargado |
| 4 | `STORE R2` | `ADD R2` | `--` | `LOAD(15)` | LOAD consolida R1=15 en WB. ADD descongelado |
| 5 | `LOAD R3` | `STORE R2` | `ADD R2(25)` | `--` | ADD calcula 15+10=25 |
| 6 | `SUB R3` | `LOAD R3` | `STORE R2` | `ADD(25)` | **Cache Miss** 0x20 (Write-Allocate). dirty\_bit=true |
| 7 | `SUB R3` ❄️ | `--` | `LOAD R3` | `STORE` | **Stall.** LOAD en MEM: **Cache Hit** en 0x20 |
| 8 | `--` | `SUB R3` | `--` | `LOAD(25)` | LOAD escribe R3=25 en WB |
| 9 | `--` | `--` | `SUB R3(10)` | `--` | SUB calcula 25-15=10 |
| 10 | `--` | `--` | `--` | `SUB(10)` | Pass-through MEM |
| 11 | `--` | `--` | `--` | `--` | **WB** escribe R3=10. Pipeline drenado ✅ |

---

## 5. Traza de Ejecución — Ejemplo 3 (JUMP)

```
Ciclo 1 | IF/ID: ADD R1,R1,R2  | ID/EX: --           | EX/MEM: --           | MEM/WB: --
Ciclo 2 | IF/ID: JUMP 0x04     | ID/EX: ADD R1,R1,R2 | EX/MEM: --           | MEM/WB: --
Ciclo 3 | IF/ID: ADD R2,R2,R1  | ID/EX: JUMP 0x04    | EX/MEM: ADD R1,R1,R2 | MEM/WB: --
Ciclo 4 | IF/ID: --            | ID/EX: --            | EX/MEM: JUMP 0x04    | MEM/WB: ADD(8)
Ciclo 5 | IF/ID: ADD R3,R1,R3  | ID/EX: --            | EX/MEM: --           | MEM/WB: JUMP
Ciclo 6 | IF/ID: STORE R3,0x30 | ID/EX: ADD R3,R1,R3 | EX/MEM: --           | MEM/WB: --
...
```

En el Ciclo 4, el JUMP llega a EX y dispara el flush: `if_id` y `id_ex` se convierten en burbujas. PC salta a 4. En el Ciclo 5 comienza `ADD R3,R1,R3` como primera instrucción legítima post-salto.

---

## 6. Reporte de Rendimiento Comparativo

| Métrica | Ej. 1 (Hazards) | Ej. 2 (ALU pura) | Ej. 3 (JUMP) | Ej. 4 (Memoria) |
|---|:---:|:---:|:---:|:---:|
| Ciclos totales | 11 | 9 | 10 | 13 |
| Instrucciones completadas | 5 | 5 | 4 | 7 |
| CPI | 2.20 | **1.80** | 2.50 | 1.86 |
| IPC | 0.45 | 0.56 | 0.40 | 0.54 |
| Tiempo @ 100 MHz | 110 ns | 90 ns | 100 ns | 130 ns |
| Cache L1 hits | 1 | 0 | 0 | 3 |
| Cache L1 misses | 2 | 0 | 1 | 2 |
| Tasa de aciertos L1 | 33% | — | — | **60%** |

**Observaciones:**
- **CPI más bajo (1.80):** Ejemplo 2, sin memoria ni stalls — solo forwarding entre instrucciones ALU.
- **CPI más alto (2.50):** Ejemplo 3, penalidad del JUMP (2 ciclos flush) sobre 4 instrucciones.
- **Tasa de aciertos más alta (60%):** Ejemplo 4, gracias a localidad espacial (0x10 y 0x11 en el mismo bloque) y Write-Back que preserva datos propios en caché.

---

## 7. Decisiones de Diseño

### 7.1 Interacción entre Stalls del Pipeline y Latencia de Caché

En una CPU real, un miss a RAM toma decenas o cientos de ciclos. En este simulador pedagógico la latencia del miss es inmediata — el Load-Use stall de 1 ciclo resuelve la dependencia temporal del datapath (el dato no está disponible hasta el final de MEM), independientemente de si la caché tardó 1 o 100 ciclos en servir el bloque.

### 7.2 Eficiencia de Write-Back con Write-Allocate

- Con Write-Through, `STORE R2,0x20` en el Ejemplo 1 habría forzado escritura síncrona a RAM en el ciclo 6.
- Con Write-Back, el dato quedó en caché (`dirty_bit=true`). Cuando `LOAD R3,0x20` pidió ese bloque en el ciclo 7, obtuvo un **Hit** directo sin que la RAM interviniera.

### 7.3 Write-back en cadena (L1 → L2 → RAM)

Aunque `sistema-integrado` usa L1 standalone (`ControladorMemoria`), la jerarquía completa (`JerarquiaCache`) implementa una cadena de write-back: los desalojos dirty de L1 van a L2, y solo los desalojos dirty de L2 llegan a RAM. Esto reduce el tráfico hacia la memoria principal incluso más que con un solo nivel.

### 7.4 Separación de responsabilidades

- `ejemplos.rs` define el catálogo — agregar un ejemplo no toca `main.rs`.
- `display.rs` centraliza el formateo de métricas — fácil de extender (ej. agregar estadísticas de L2).
- `main.rs` solo itera el catálogo e invoca `ejecutar_ejemplo()`.

---

## 8. Compilación y Ejecución

```bash
# Ejecutar todos los ejemplos en secuencia
cargo run --package sistema-integrado

# Verificar todos los tests del workspace completo
cargo test --workspace
```

### Resultado de tests del workspace

| Crate | Tests | Cobertura |
|---|---|---|
| `cache-controller` | **22** | L1 standalone (LRU, Write-Back, decodificación u16) · L2 standalone · JerarquiaCache (L1→L2→RAM, write-back en cadena) · migración de tipo u16 |
| `cpu-pipeline` | **39** (+1 doctest) | Forwarding, Load-Use hazards, JUMP, R0 hardwired-zero, aritmética wrapping, métricas, Display |
| `sistema-integrado` | **1** | Cálculo y formateo del reporte de rendimiento (CPI, IPC, tiempos, caché) |
| **Total** | **63** | **100% pasando** |

---

## 9. Estructura de Archivos

```
arquitectura-cpu-rust/
├── Cargo.toml               # workspace: cpu-pipeline, cache-controller, sistema-integrado
│
├── cache-controller/
│   └── src/
│       ├── lib.rs           # Re-exports: ControladorMemoria, NivelL2, JerarquiaCache
│       ├── storage.rs       # Structs, constantes, decodificadores L1/L2
│       ├── policy.rs        # elegir_via_victima + manejar_miss + tasa_de_aciertos
│       ├── bus.rs           # leer_byte, escribir_byte, flush (L1 standalone)
│       ├── hierarchy.rs     # JerarquiaCache: orquestador L1→L2→RAM
│       ├── main.rs          # Demo interactivo (6 demos L1 y L2)
│       └── tests.rs         # 22 tests
│
├── cpu-pipeline/
│   └── src/
│       ├── lib.rs           # Pipeline, forwarding, hazard detection, API pública
│       ├── main.rs          # Demo de pipeline standalone
│       └── tests.rs         # 39 tests
│
└── sistema-integrado/
    └── src/
        ├── display.rs       # reporte_rendimiento() + test de métricas
        ├── ejemplos.rs      # Catálogo de programas (struct Ejemplo + catalogo())
        └── main.rs          # Runner: itera catálogo y ejecuta cada ejemplo
```
