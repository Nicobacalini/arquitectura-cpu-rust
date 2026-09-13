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

---

## 3. Programa de Demostración

El binario principal [`src/main.rs`](src/main.rs) define y ejecuta un programa integral que ejerce simultáneamente los mecanismos clave de la CPU y de la memoria:

```rust
let programa = vec![
    // I0: Cargar dato desde RAM[0x10] (15) en R1
    Instruccion::LOAD {
        dest: Registro::R1,
        direccion_ram: 0x10,
    },
    // I1: R2 = R1 + R2 -> Genera Load-Use Hazard (stall de 1 ciclo)
    Instruccion::ADD {
        dest: Registro::R2,
        src1: Registro::R1,
        src2: Registro::R2,
    },
    // I2: Guardar R2 en RAM[0x20] -> Instruccion STORE
    Instruccion::STORE {
        src: Registro::R2,
        direccion_ram: 0x20,
    },
    // I3: Cargar RAM[0x20] en R3 -> Cache Hit garantizado (Write-Allocate previo)
    Instruccion::LOAD {
        dest: Registro::R3,
        direccion_ram: 0x20,
    },
    // I4: R3 = R3 - R1 -> Genera un segundo Load-Use Hazard
    Instruccion::SUB {
        dest: Registro::R3,
        src1: Registro::R3,
        src2: Registro::R1,
    },
];
```

### Condiciones Iniciales:
- **Banco de registros:** `R0 = 0` (fijo), `R1 = 0`, `R2 = 10`, `R3 = 0`.
- **RAM:** `RAM[0x10] = 15`.
- **Caché:** Completamente vacía (todas las líneas inválidas).

---

## 4. Traza de Ejecución Ciclo a Ciclo

A continuación se detalla la traza exacta producida por el formateador `Display` en cada ciclo de reloj:

```text
Ciclo 1 | IF/ID: LOAD R1,0x10 | ID/EX: -- | EX/MEM: -- | MEM/WB: --
Ciclo 2 | IF/ID: ADD R2,R1,R2 | ID/EX: LOAD R1,0x10 | EX/MEM: -- | MEM/WB: --
Ciclo 3 | IF/ID: ADD R2,R1,R2 | ID/EX: -- | EX/MEM: LOAD R1,0x10 | MEM/WB: --
Ciclo 4 | IF/ID: STORE R2,0x20 | ID/EX: ADD R2,R1,R2 | EX/MEM: -- | MEM/WB: LOAD R1,0x10
Ciclo 5 | IF/ID: LOAD R3,0x20 | ID/EX: STORE R2,0x20 | EX/MEM: ADD R2,R1,R2 | MEM/WB: --
Ciclo 6 | IF/ID: SUB R3,R3,R1 | ID/EX: LOAD R3,0x20 | EX/MEM: STORE R2,0x20 | MEM/WB: ADD R2,R1,R2
Ciclo 7 | IF/ID: SUB R3,R3,R1 | ID/EX: -- | EX/MEM: LOAD R3,0x20 | MEM/WB: STORE R2,0x20
Ciclo 8 | IF/ID: -- | ID/EX: SUB R3,R3,R1 | EX/MEM: -- | MEM/WB: LOAD R3,0x20
Ciclo 9 | IF/ID: -- | ID/EX: -- | EX/MEM: SUB R3,R3,R1 | MEM/WB: --
Ciclo 10 | IF/ID: -- | ID/EX: -- | EX/MEM: -- | MEM/WB: SUB R3,R3,R1
Ciclo 11 | IF/ID: -- | ID/EX: -- | EX/MEM: -- | MEM/WB: --
```

### Análisis detallado de los eventos clave:

| Ciclo | Etapa afectada | Evento en Pipeline | Evento en Caché / Memoria |
|:---:|:---:|---|---|
| **1** | **IF** | Se busca `LOAD R1,0x10`. | Sin acceso a memoria de datos. |
| **2** | **ID / EX** | `LOAD` pasa a ID/EX. Se busca `ADD R2,R1,R2`. | La CPU detecta que `ADD` necesita `R1`, cuyo dato aún no existe. |
| **3** | **HAZARD / MEM** | **Load-Use Stall:** se inserta burbuja `--` en `ID/EX`. `IF/ID` y `PC` se congelan. | `LOAD` ejecuta en MEM: **Cache Miss** en `0x10`. Se carga el bloque de RAM a la caché. |
| **4** | **WB / EX** | `LOAD` consolida en WB (`R1 = 15`). `ADD` avanza a EX y usa el dato cargado. | Sin acceso a memoria de datos. |
| **5** | **EX / MEM** | `ADD` calcula `R2 = 15 + 10 = 25`. `STORE R2,0x20` avanza a ID/EX. | Sin acceso a memoria de datos. |
| **6** | **MEM** | `STORE R2,0x20` ejecuta en MEM con valor `25` (empaquetado desde EX). | **Cache Miss (Write-Allocate):** carga bloque `0x20`, escribe `25` y marca `dirty_bit = true`. |
| **7** | **HAZARD / MEM** | **Segundo Load-Use Stall:** `SUB` necesita `R3` que recién está en MEM por el `LOAD`. | `LOAD R3,0x20` ejecuta en MEM: **Cache Hit (100% acierto)** porque el bloque fue cargado en el ciclo anterior. |
| **8** | **WB / EX** | `LOAD` escribe `R3 = 25` en WB. `SUB` avanza a EX. | Sin acceso a memoria de datos. |
| **9-11**| **Drenado** | `SUB` calcula `25 - 15 = 10`, avanza por MEM y consolida en WB en el ciclo 10. | Pipeline se vacía por completo en el ciclo 11. |

---

## 5. Reporte Final de Ejecución

Al concluir el ciclo 11, el sistema emite el reporte consolidado de estado del procesador y métricas del subsistema de memoria:

```text
============================================================
                    Estado Final de la CPU                  
============================================================
  Ciclos totales de CPU : 11
  Banco de registros    : [0, 15, 25, 10]
    R0 = 0 (hardwired zero)
    R1 = 15
    R2 = 25
    R3 = 10

============================================================
                    Estadisticas de Cache                   
============================================================
  Hits            : 1
  Misses          : 2
  Total accesos   : 3
  Tasa de aciertos: 33.33%
  Desalojos dirty : 0
  Ciclos de cache : 3
============================================================
```

### Verificación matemática de los registros:
- **`R0` = 0:** Registro hardwired-zero inmutable.
- **`R1` = 15:** Cargado de `RAM[0x10]`.
- **`R2` = 25:** Resultado de `R1 (15) + R2 (10)`.
- **`R3` = 10:** Leído desde `0x20` (donde se guardó `25`) y restado con `R1 (15)`: $25 - 15 = 10$.

### Desglose de accesos a la caché:
1. **Acceso 1 (Ciclo 3):** `leer_byte(0x10)` $\rightarrow$ **Miss** (línea fría, se trae el bloque `0x10..0x13` a la Vía 0 del Set 0).
2. **Acceso 2 (Ciclo 6):** `escribir_byte(0x20, 25)` $\rightarrow$ **Miss** (Write-Allocate: se trae bloque `0x20..0x23` a la Vía 1 del Set 0 y se modifica el byte `0x20`).
3. **Acceso 3 (Ciclo 7):** `leer_byte(0x20)` $\rightarrow$ **Hit** (el bloque ya está en la Vía 1 del Set 0).

---

## 6. Decisiones de Diseño y Sinergia de Arquitectura

### 6.1 Interacción entre Stalls del Pipeline y la Latencia de Caché
En una CPU real, un fallo de caché (*cache miss*) hacia la memoria RAM toma decenas o cientos de ciclos. En este simulador pedagógico:
- La lógica de detección de hazards del pipeline desacopla el control de datos: el **Load-Use stall** de 1 ciclo resuelve la dependencia temporal inherente al datapath (el dato no está disponible hasta el final de MEM).
- Si la caché produjera un retardo variable por miss a RAM, la CPU podría congelarse agregando ciclos de *memory stall* sin alterar la corrección del forwarding ni la detección de riesgos.

### 6.2 Eficiencia de Write-Back con Write-Allocate
- Si hubiésemos utilizado **Write-Through**, la instrucción `STORE` del ciclo 6 habría forzado una escritura síncrona a la memoria RAM externa.
- Con **Write-Back**, la CPU escribió el dato en la caché inmediatamente. Al requerir la instrucción siguiente (`LOAD R3, 0x20`) ese mismo dato, se obtuvo un **Hit** directo en caché sin que la RAM externa interviniera, optimizando drásticamente el ancho de banda del bus.

---

## 7. Compilación y Ejecución

Para compilar y ejecutar la simulación integrada:

```bash
cargo run --package sistema-integrado --bin sistema-integrado
```

Para verificar la integridad de todos los tests unitarios del espacio de trabajo completo (51 tests):

```bash
cargo test --workspace
```

---

## 8. Estructura de Archivos

```text
sistema-integrado/
├── Cargo.toml       # Declara dependencias hacia cpu-pipeline y cache-controller
├── README.md        # Documentación de la arquitectura integrada y resultados
└── src/
    └── main.rs      # Binario demostrativo con simulación integrada paso a paso
```
