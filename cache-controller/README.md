# cache-controller

**L1:** 4 conjuntos × 2 vías · LRU · Write-Back / Write-Allocate · tag=12b  
**L2:** 8 conjuntos × 2 vías · LRU · Write-Back / Write-Allocate · tag=11b  
**RAM:** 4096 bytes, direccionamiento `u16`

Este documento describe el diseño completo del subsistema de memoria de dos niveles: estructura física de cada caché, esquemas de bits de dirección (uno por nivel), flujo de decisión ante hit/miss, write-back en cadena y las decisiones de arquitectura tomadas (con su justificación) para el simulador del proyecto `arquitectura-cpu-rust`.

---

## 1. Contexto y Objetivo

Simulamos una memoria RAM de 4096 bytes intermediada por **dos niveles de caché** asociativa por conjuntos (2 vías cada uno). El objetivo es demostrar, con datos reales y medibles, por qué la localidad de referencia hace que cachés pequeñas aceleren drásticamente los accesos, y por qué la política Write-Back reduce el tráfico hacia la RAM frente a Write-Through.

---

## 2. Decodificación de Direcciones (16 bits)

Cada nivel tiene su **propio esquema de bits** — los anchos son distintos porque L2 tiene el doble de conjuntos que L1.

### 2.1 L1 — `ControladorMemoria`

```
+-------------+-------------+---------------+
| Tag (12b)   | Index (2b)  | Offset (2b)   |
+-------------+-------------+---------------+
| Bit 15...4  | Bit 3...2   | Bit 1...0     |
+-------------+-------------+---------------+
```

| Campo | Bits | Propósito |
|---|---|---|
| **Offset** | 1-0 | Posición del byte dentro del bloque de 4 bytes |
| **Index** | 3-2 | Selecciona uno de los **4 conjuntos** |
| **Tag** | 15-4 | **12 bits** — identifica el bloque en caché |

```rust
let offset = (direccion & 0b0000_0011) as usize;
let indice = ((direccion & 0b0000_1100) >> 2) as usize;
let tag    = (direccion >> 4) & ((1u16 << 12) - 1);
// Reconstrucción de la dirección base (para write-back al desalojar):
let dir_base = (tag << 4) | ((indice as u16) << 2);
```

**Ejemplo:** `0x005A` → tag=5, index=2, offset=2.

### 2.2 L2 — `NivelL2`

```
+-------------+-----------+----------+
|  Tag (11b)  | Index (3b)| Offset(2b)|
+-------------+-----------+----------+
|  Bit 15..5  | Bit 4..2  | Bit 1..0  |
+-------------+-----------+----------+
```

| Campo | Bits | Propósito |
|---|---|---|
| **Offset** | 1-0 | Posición del byte dentro del bloque de 4 bytes (igual que L1) |
| **Index** | 4-2 | Selecciona uno de los **8 conjuntos** |
| **Tag** | 15-5 | **11 bits** — `16 - 3 - 2 = 11` (≠ 12 bits de L1) |

```rust
let offset = (direccion & 0b0000_0000_0000_0011) as usize;
let indice = ((direccion & 0b0000_0000_0001_1100) >> 2) as usize;
let tag    = (direccion >> 5) & ((1u16 << 11) - 1);
// Reconstrucción de la dirección base en L2:
let dir_base = (tag << 5) | ((indice as u16) << 2);
```

> **Por qué dos decodificadores distintos:** L2 tiene 3 bits de índice frente a los 2 de L1. Si se usara el mismo decodificador, la mitad del espacio de conjuntos de L2 quedaría inalcanzable.

---

## 3. Estructura Física de la Caché

### L1 (`ControladorMemoria`) — 32 bytes efectivos

```
+-----------+---------------------------------------+---------------------------------------+
| CONJUNTO  |                 VÍA 0                  |                 VÍA 1                  |
+-----------+---------------------------------------+---------------------------------------+
| Set 0(00) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 1(01) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 2(10) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 3(11) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
+-----------+---------------------------------------+---------------------------------------+
[V] = Válido   [D] = Dirty Bit
```

### L2 (`NivelL2`) — 64 bytes efectivos

```
+-----------+---------------------------------------+---------------------------------------+
| Set 0(000)| [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 1(001)| ...                                    | ...                                    |
| ...       |                                        |                                        |
| Set 7(111)| [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
+-----------+---------------------------------------+---------------------------------------+
```

### Tipos de Rust compartidos

```rust
const TAMANO_RAM: usize = 4096;
const BLOQUE_BYTES: usize = 4;
const CANTIDAD_CONJUNTOS: usize = 4;
const CANTIDAD_CONJUNTOS_L2: usize = 8;

pub struct LineaCache {
    pub tag: u16,
    pub valido: bool,
    pub dirty_bit: bool,
    pub datos: [u8; BLOQUE_BYTES],
    pub ultimo_acceso: u64,
}
pub struct ConjuntoCache { pub vias: [LineaCache; 2] }
pub struct EstadisticasCache { pub hits: u64, pub misses: u64, pub desalojos_dirty: u64 }

pub struct ControladorMemoria {          // L1 (standalone)
    pub ram: [u8; TAMANO_RAM],
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS],
    pub contador_ciclos: u64,
    pub estadisticas: EstadisticasCache,
}

pub struct NivelL2 {                     // L2 (standalone, sin RAM propia)
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS_L2],
    pub contador_ciclos: u64,
    pub estadisticas: EstadisticasCache,
}

pub struct JerarquiaCache {              // Orquestador L1 → L2 → RAM
    pub l1: ControladorMemoria,
    pub l2: NivelL2,
    pub ram: [u8; TAMANO_RAM],
}
```

**Nota:** todo el estado vive en el stack (arrays de tamaño fijo, sin `Vec`) — coherente con restricciones `#![no_std]`-style, sin heap.

---

## 4. Decisión de Diseño Clave: `NivelL2` sin RAM propia

```rust
pub fn leer_byte(&mut self, direccion: u16, ram: &mut [u8; TAMANO_RAM]) -> u8
pub fn escribir_byte(&mut self, direccion: u16, dato: u8, ram: &mut [u8; TAMANO_RAM])
```

`NivelL2` recibe la RAM como parámetro mutable en cada llamada, sin guardar un campo `ram` propio. Esto permite que `JerarquiaCache` sea el único dueño de la RAM, pasándosela a L2 según la necesite, sin cambiar nada de la lógica interna de L2.

---

## 5. Flujo de Lectura/Escritura en la Jerarquía

```
leer_byte(dir) sobre JerarquiaCache
       │
       ▼
┌─────────────┐
│  L1 hit?    │──SÍ──► devolver byte  (actualizar LRU L1, hits L1++)
└──────┬──────┘
       NO
       ▼
┌─────────────┐
│  L2 hit?    │──SÍ──► traer bloque L2→L1 (possible desalojo L1 con WB→L2)
└──────┬──────┘        devolver byte
       NO
       ▼
  traer bloque RAM→L2 (possible desalojo L2 con WB→RAM)
  traer bloque L2→L1  (possible desalojo L1 con WB→L2)
  devolver byte
```

### Write-back en cadena

```
Desalojo dirty de L1  →  volcado a L2   (NO a RAM directamente)
Desalojo dirty de L2  →  volcado a RAM
```

Esto garantiza que la RAM solo recibe escrituras cuando L2 expulsa una línea sucia — nunca por un desalojo de L1.

---

## 6. API Pública

```rust
// ── L1 standalone ──────────────────────────────────────────────────────────
impl ControladorMemoria {
    pub fn nuevo() -> Self;
    pub fn decodificar_direccion(&self, dir: u16) -> (u16, usize, usize); // tag/12b, idx/2b, off/2b
    pub fn reconstruir_direccion_base(&self, tag: u16, indice: usize) -> u16;
    pub fn buscar_via_hit(&self, indice: usize, tag: u16) -> Option<usize>;
    pub fn leer_byte(&mut self, dir: u16) -> u8;
    pub fn escribir_byte(&mut self, dir: u16, dato: u8);
    pub fn flush(&mut self);   // vuelca dirty bits a RAM sin invalidar
}

// ── L2 standalone (RAM por parámetro) ──────────────────────────────────────
impl NivelL2 {
    pub fn nuevo() -> Self;
    pub fn decodificar_direccion(&self, dir: u16) -> (u16, usize, usize); // tag/11b, idx/3b, off/2b
    pub fn buscar_via_hit_l2(&self, indice: usize, tag: u16) -> Option<usize>;
    pub fn leer_byte(&mut self, dir: u16, ram: &mut [u8; TAMANO_RAM]) -> u8;
    pub fn escribir_byte(&mut self, dir: u16, dato: u8, ram: &mut [u8; TAMANO_RAM]);
}

// ── Jerarquía conectada ────────────────────────────────────────────────────
impl JerarquiaCache {
    pub fn nuevo() -> Self;
    pub fn leer_byte(&mut self, dir: u16) -> u8;     // L1→L2→RAM
    pub fn escribir_byte(&mut self, dir: u16, dato: u8);
    pub fn flush(&mut self);   // L1→L2 y luego L2→RAM
}

// ── Estadísticas ───────────────────────────────────────────────────────────
impl EstadisticasCache {
    pub fn tasa_de_aciertos(&self) -> f64;   // hits / (hits + misses)
}
```

---

## 7. Algoritmo de Flujo Completo (por nivel)

```
                      ┌──────────────────────────────┐
                      │    Llamar leer / escribir    │
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │    contador_ciclos += 1      │
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │    Decodificar Dirección     │ ──► Tag / Index / Offset
                      │  (L1: 12/2/2 · L2: 11/3/2)  │     (cada nivel: su fn)
                      └──────────────┬───────────────┘
                                     │
                       ¿Vía válida con mismo Tag?
                                     │
                      ┌─────────────┴──────────────┐
                     SÍ → HIT                     NO → MISS
                      │                             │
              hits++, LRU update           misses++, manejar_miss()
              devolver byte                   │
                                              ▼
                                    ┌──────────────────┐
                                    │ Elegir víctima   │
                                    │ (libre > LRU)    │
                                    └────────┬─────────┘
                                             │
                              ¿Víctima dirty?
                                             │
                              SÍ  →  Write-back   NO  →  Descartar
                              (L1→L2, L2→RAM)
                                             │
                                    ┌────────▼─────────┐
                                    │ Traer nuevo      │
                                    │ bloque (4 bytes) │
                                    │ valido=true      │
                                    │ dirty=false      │
                                    └────────┬─────────┘
                                             │
                                    devolver byte / escribir dato
```

---

## 8. Suite de Tests

```bash
cargo test --package cache-controller
```

**Resultado: `22 passed; 0 failed`**

```
running 22 tests
test tests::test_controlador_nuevo ... ok
test tests::test_decodificar_direccion ... ok
test tests::test_decodificar_direccion_u16_offsets_e_indice_correctos ... ok
test tests::test_desalojo_dirty_de_l1_escribe_en_l2_no_en_ram ... ok
test tests::test_flush_sincroniza_sin_invalidar ... ok
test tests::test_l1_hit_no_consulta_l2 ... ok
test tests::test_l1_miss_l2_hit_trae_bloque_a_l1 ... ok
test tests::test_l1_miss_l2_miss_trae_desde_ram ... ok
test tests::test_l2_hit_no_consulta_ram ... ok
test tests::test_l2_miss_trae_bloque_de_ram ... ok
test tests::test_lru_desaloja_la_correcta ... ok
test tests::test_miss_con_via_dirty_hace_writeback ... ok
test tests::test_miss_con_via_valida_limpia_no_hace_writeback ... ok
test tests::test_miss_en_via_invalida_carga_datos ... ok
test tests::test_miss_luego_hit ... ok
test tests::test_offset_correcto ... ok
test tests::test_preferencia_via_libre_sobre_lru ... ok
test tests::test_reconstruir_direccion_base ... ok
test tests::test_tasa_de_aciertos ... ok
test tests::test_via_victima_con_via_invalida ... ok
test tests::test_via_victima_lru ... ok
test tests::test_write_back_al_desalojar ... ok
```

| Categoría | Tests | Qué verifican |
|---|---|---|
| **Unitarios L1 (caja blanca)** | 8 | `decodificar_direccion`, `reconstruir_direccion_base`, LRU, `manejar_miss` (carga, desalojo limpio, desalojo dirty) |
| **API pública L1** | 6 | Miss→Hit, write-back al desalojar, LRU correcto, offset, preferencia vía libre, tasa de aciertos |
| **Extensión L1 (`flush`)** | 1 | `test_flush_sincroniza_sin_invalidar` |
| **Migración de tipo u16** | 1 | `test_decodificar_direccion_u16_offsets_e_indice_correctos` |
| **L2 standalone** | 2 | Hit no consulta RAM · Miss trae bloque de RAM |
| **Jerarquía conectada** | 4 | L1 hit no consulta L2 · MISS L1 HIT L2 trae a L1 · MISS L1 MISS L2 desde RAM · desalojo dirty L1→L2 (no RAM) |

---

## 9. Decisiones de Diseño y su Justificación

### 9.1 Write-Back en vez de Write-Through

Con **Write-Back**, una escritura solo modifica la caché (`dirty_bit = true`); el dato se propaga a la RAM únicamente al desalojar. Reduce drásticamente el tráfico en patrones de múltiples escrituras sobre la misma dirección.

**Costo aceptado:** datos sin sincronizar pueden perderse si el sistema falla. Equivalente a *journaling* / *fsync* en sistemas de archivos — nuestro `flush()` cumple esa función.

### 9.2 Write-Allocate

Un miss de escritura trae el bloque completo a caché antes de modificarlo. Asume localidad espacial — si el programa escribe en una dirección, probablemente vuelva a leer/escribir cerca.

### 9.3 LRU: vía libre siempre gana

Si alguna vía del conjunto tiene `valido == false`, se usa directamente sin comparar `ultimo_acceso`. Comparar LRU solo aplica cuando **ambas** vías están ocupadas. Desempate (ambas libres): vía 0 por convención.

### 9.4 Write-back en cadena L1 → L2 → RAM

Un desalojo dirty de L1 se vuelca a L2 (no a RAM). Solo si ese volcado a su vez provoca un desalojo dirty en L2, ese bloque llega a RAM. Esta cadena garantiza que la RAM siempre recibe la versión más reciente de los datos, independientemente de cuántas escrituras intermedias hubo.

### 9.5 `contador_ciclos` incrementa antes de operar

Garantiza marcas temporales estrictamente crecientes entre accesos consecutivos — crítico para que el desempate LRU sea determinístico en los tests.

---

## 10. Errores Comunes al Implementar (Gotchas)

- **Reconstruir la dirección con el tag equivocado:** al desalojar dirty, usar el **tag viejo de la vía víctima**, nunca el de la nueva dirección.
- **Usar el mismo decodificador para L1 y L2:** los anchos de campo son distintos — cada nivel necesita su propia función.
- **Dar RAM propia a `NivelL2`:** fuerza a reescribir la firma al integrar la jerarquía. Pasar la RAM por parámetro evita ese trabajo.
- **Write-back de L1 directo a RAM:** viola la semántica de la jerarquía — L1 debe escribir en L2, no saltear el nivel.
- **Olvidar los contadores `hits`/`misses`:** no afectan la corrección funcional, pero rompen `tasa_de_aciertos()`.

---

## 11. Estructura de Archivos del Crate

```
cache-controller/
├── Cargo.toml
└── src/
    ├── lib.rs        # Re-exports públicos: ControladorMemoria, NivelL2, JerarquiaCache, …
    ├── storage.rs    # Structs, constantes, constructores, decodificadores, API NivelL2
    ├── policy.rs     # elegir_via_victima + manejar_miss (L1) + tasa_de_aciertos
    ├── bus.rs        # leer_byte, escribir_byte, flush (API pública L1 standalone)
    ├── hierarchy.rs  # JerarquiaCache: orquestador L1→L2→RAM, write-back en cadena
    ├── main.rs       # Demo interactivo: 6 demos (L1 y L2 standalone)
    └── tests.rs      # 22 tests unitarios e integración
```
---

## 12. Fase 2 — Módulo `paginacion`: MMU con TLB y Page Table

### 12.1 Resumen

El módulo `paginacion.rs` agrega una capa de **traducción de direcciones virtuales** delante de `JerarquiaCache`. La `JerarquiaCache` no se modifica: sigue operando con direcciones físicas exactamente igual que en la Fase 1. Lo nuevo es la `Mmu` que interpone la traducción.

### 12.2 Descomposición de la dirección virtual (16 bits)

```
+---------- VPN (8 bits) ----------+------- OFFSET (8 bits) ------+
| Bit 15                      Bit 8 | Bit 7                  Bit 0 |
+-----------------------------------+------------------------------+

dir_fisica = (marco_fisico as u16) << 8 | offset as u16
```

Con `marco_fisico ∈ [0..16)`, `dir_fisica ∈ [0..4096)` siempre — por diseño.

### 12.3 Flujo de traducción

```
leer_byte(vaddr) / escribir_byte(vaddr, dato)
  │
  └─► traducir_direccion(mmu, vaddr, tipo)
        │
        ├─ TLB hit (VPN + ASID) ──────────────────► reconstruir_direccion_fisica
        │                                             └► jerarquia.leer_byte / escribir_byte
        │
        └─ TLB miss (+penalidad_tlb_miss)
              │
              ├─ Page Table valida+presente
              │   ├─ Escritura en solo_lectura ──► ViolacionProteccion
              │   └─ OK ─────────────────────────► insertar TLB + Exitosa
              │
              └─ No presente / inválida ──────────► PageFault
                    ├─ Marco libre → instalar
                    └─ Sin marcos → desalojar LRU → instalar
                    (luego reintentar → Exitosa + jerarquia.leer_byte/escribir_byte)
```

### 12.4 TLB con ASID

La TLB es **totalmente asociativa** (todos los conjuntos en un `Vec`). Un hit requiere que coincidan tanto `vpn` como `asid`. Se usa `VecDeque<usize>` para mantener el orden de uso LRU (índices al Vec de entradas). Al llenarse, se desaloja la entrada al fondo de la cola (la menos recientemente usada).

La separación por ASID garantiza que dos procesos con la misma VPN no compartan traducciones en la TLB, aunque sí puedan estar mapeados a marcos distintos de la misma RAM física.


