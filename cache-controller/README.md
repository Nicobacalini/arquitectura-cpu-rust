# Proyecto 2: Controlador de Caché Asociativa por Conjuntos

**Configuración:** 4 conjuntos, 2 vías por conjunto, reemplazo LRU, política Write-Back con Write-Allocate.

Este documento describe el diseño completo del subsistema de memoria: la estructura física de la caché, el algoritmo de decodificación de direcciones, el flujo de decisión ante hit/miss, y las decisiones de arquitectura tomadas (con su justificación) para el simulador de jerarquía de memoria del proyecto `arquitectura-cpu-rust`.

---

## 1. Contexto y Objetivo

Simulamos una memoria RAM de 256 bytes intermediada por una caché asociativa por conjuntos de 2 vías. El objetivo es demostrar, con datos reales y medibles, por qué la localidad de referencia (temporal y espacial) hace que una caché pequeña acelere drásticamente el acceso a una memoria más grande y lenta — y por qué la política de escritura elegida (Write-Back) reduce el tráfico hacia la RAM frente a la alternativa (Write-Through).

## 2. Decodificación de Direcciones (8 bits)

Cada dirección de memoria se descompone en tres campos:

```
+------------+-------------+---------------+
| Tag (4b)   | Index (2b)  | Offset (2b)   |
+------------+-------------+---------------+
| Bit 7...4  | Bit 3...2   | Bit 1...0     |
+------------+-------------+---------------+
```

| Campo | Bits | Tamaño | Propósito |
|---|---|---|---|
| **Offset** | 1-0 | 2 bits → 4 valores | Posición del byte dentro de un bloque de 4 bytes (`2² = 4`) |
| **Index** | 3-2 | 2 bits → 4 valores | Selecciona a cuál de los 4 conjuntos mapea la dirección (`2² = 4`) |
| **Tag** | 7-4 | 4 bits | Identifica de forma única qué bloque de RAM está cargado en esa línea |

### Fórmulas (máscaras y desplazamientos)

```rust
let offset = (direccion & 0b0000_0011) as usize;
let indice = ((direccion & 0b0000_1100) >> 2) as usize;
let tag    = (direccion & 0b1111_0000) >> 4;

// Reconstrucción de la dirección base de un bloque (offset = 0),
// necesaria para saber a qué dirección de RAM corresponde una línea
// al momento de desalojarla:
let direccion_base = (tag << 4) | ((indice as u8) << 2);
```

### Ejemplo numérico concreto

Dirección `0x5A` (`0b0101_1010`):

| | Binario | Decimal |
|---|---|---|
| Byte completo | `0101 10 10` | `0x5A` (90) |
| Tag (bits 7-4) | `0101` | `5` |
| Index (bits 3-2) | `10` | `2` |
| Offset (bits 1-0) | `10` | `2` |

Esta dirección busca en el **Conjunto 2**, el bloque identificado con **Tag 5**, y dentro de ese bloque, el **byte en la posición 2** (tercer byte del bloque de 4).

## 3. Estructura Física de la Caché

```
+-----------+---------------------------------------+---------------------------------------+
| CONJUNTO  |                 VÍA 0                  |                 VÍA 1                  |
+-----------+---------------------------------------+---------------------------------------+
| Set 0(00) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 1(01) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 2(10) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
| Set 3(11) | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso | [V][D][Tag][B0|B1|B2|B3] ultimo_acceso |
+-----------+---------------------------------------+---------------------------------------+
[V] = Válido (bool)   [D] = Dirty Bit (bool)
```

### Tipos de Rust correspondientes

```rust
const TAMANO_RAM: usize = 256;
const BLOQUE_BYTES: usize = 4;
const CANTIDAD_CONJUNTOS: usize = 4;

pub struct LineaCache {
    pub tag: u8,
    pub valido: bool,
    pub dirty_bit: bool,
    pub datos: [u8; BLOQUE_BYTES],
    pub ultimo_acceso: u64,
}

pub struct ConjuntoCache {
    pub vias: [LineaCache; 2],
}

pub struct EstadisticasCache {
    pub hits: u64,
    pub misses: u64,
    pub desalojos_dirty: u64,
}

pub struct ControladorMemoria {
    pub ram: [u8; TAMANO_RAM],
    pub cache: [ConjuntoCache; CANTIDAD_CONJUNTOS],
    pub contador_ciclos: u64,
    pub estadisticas: EstadisticasCache,
}
```

**Nota de diseño:** todo el estado vive en el stack (arrays de tamaño fijo, sin `Vec`), consistente con la restricción `#![no_std]`-style del proyecto — nada de memoria dinámica del heap para simular hardware real.

## 4. Algoritmo de Flujo Lógico Completo

```
                      ┌──────────────────────────────┐
                      │    Llamar leer / escribir    │
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │    contador_ciclos += 1      │  ◄── garantiza marcas temporales únicas
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │ Recibir Dirección (8 bits)   │
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │    Decodificar Dirección     │ ──► Extrae: Tag, Index, Offset
                      └──────────────┬───────────────┘
                                     │
                                     ▼
                      ┌──────────────────────────────┐
                      │ Buscar en Conjunto [Index]   │
                      └──────────────┬───────────────┘
                                     │
                       ¿Vía válida con mismo Tag?
                                     │
                      ┌──────────────┴──────────────┐
                     SÍ                            NO
                      │                             │
                      ▼                             ▼
              ┌─────────────┐               ┌─────────────┐
              │   ¡HIT!     │               │   ¡MISS!    │
              └──────┬──────┘               └──────┬──────┘
                     │                             │
                     ▼                             ▼
       ┌───────────────────────────┐ ┌───────────────────────────┐
       │  estadisticas.hits += 1   │ │ estadisticas.misses += 1  │
       └─────────────┬─────────────┘ └─────────────┬─────────────┘
                     │                             │
                     │                             ▼
                     │              ┌─────────────────────────────┐
                     │              │    llamar manejar_miss()    │
                     │              └──────────────┬──────────────┘
                     │                             │
                     │                             ▼
                     │              ┌─────────────────────────────┐
                     │              │ 1. ELEGIR VÍCTIMA (LRU):    │
                     │              │  • ¿Vía 0 libre? ──► Vía 0  │
                     │              │  • ¿Vía 1 libre? ──► Vía 1  │
                     │              │  • ¿Ambas ocupadas?         │
                     │              │    ──► Menor ultimo_acceso  │
                     │              └──────────────┬──────────────┘
                     │                             │
                     │                             ▼
                     │               ¿La vía víctima tiene
                     │             valido==true Y dirty==true?
                     │                             │
                     │              ┌──────────────┴──────────────┐
                     │             SÍ                            NO
                     │              │                             │
                     │              ▼                             ▼
                     │      ┌──────────────┐              ┌──────────────┐
                     │      │   DESALOJO   │              │  DESCARTAR   │
                     │      │  WRITE-BACK  │              │    LÍNEA     │
                     │      │ •Reconstruir │              │ (no escribe  │
                     │      │  dir. vieja  │              │   en RAM)    │
                     │      │  con el TAG  │              └──────┬───────┘
                     │      │  VIEJO de la │                     │
                     │      │  vía víctima │                     │
                     │      │ •Escribir 4  │                     │
                     │      │  bytes a RAM │                     │
                     │      │ •desalojos_  │                     │
                     │      │  dirty += 1  │                     │
                     │      └──────┬───────┘                     │
                     │             │                             │
                     │             └──────────────┬──────────────┘
                     │                            │
                     │                            ▼
                     │              ┌─────────────────────────────┐
                     │              │     TRAER NUEVO BLOQUE      │
                     │              │ • Leer 4 bytes desde RAM    │
                     │              │   (dirección base NUEVA)    │
                     │              │ • Actualizar tag = nuevo_tag│
                     │              │ • valido=true, dirty=false  │
                     │              └──────────────┬──────────────┘
                     │                             │
                     │                             ▼ (retorna índice de vía)
                     └─────────────────────┬───────┘
                                           │
                                           ▼
                      ┌──────────────────────────────────────────┐
                      │        Actualizar último acceso          │
                      │  ──► via.ultimo_acceso = contador_ciclos │
                      └────────────────────┬─────────────────────┘
                                           │
                                  ¿Qué operación es?
                                           │
                      ┌────────────────────┴─────────────────────┐
                   LECTURA                                   ESCRITURA
                      │                                          │
                      ▼                                          ▼
       ┌─────────────────────────────┐            ┌─────────────────────────────┐
       │        DEVOLVER BYTE        │            │       MODIFICAR BYTE        │
       │  Retorna datos[offset]      │            │  • datos[offset] = dato     │
       │                             │            │  • dirty_bit = true         │
       └─────────────────────────────┘            └─────────────────────────────┘
```

## 5. Decisiones de Diseño y su Justificación

### 5.1 Write-Back en vez de Write-Through

Con **Write-Back**, una escritura solo modifica la caché (marcando `dirty_bit = true`); el dato se propaga a la RAM únicamente cuando la línea se desaloja (o se sincroniza explícitamente con `flush()`). Con **Write-Through**, cada escritura iría inmediatamente tanto a la caché como a la RAM.

**Por qué elegimos Write-Back:** porque reduce drásticamente el tráfico hacia la RAM en programas con múltiples escrituras sobre la misma dirección en un período corto (ej. un contador que se incrementa en un loop) — cada incremento intermedio nunca llega a tocar la RAM, solo el valor final, en el momento del desalojo.

**Costo aceptado:** si el sistema pierde energía o crashea con líneas `dirty` sin sincronizar, esos datos se pierden. Es el trade-off clásico velocidad-vs-durabilidad — el mismo motivo por el que sistemas de archivos reales usan *journaling* o *fsync* explícitos en puntos críticos (nuestro equivalente es `flush()`).

### 5.2 Write-Allocate: un miss de escritura también trae el bloque a caché

Cuando `escribir_byte` falla (miss), en vez de escribir directo en la RAM y no cachear nada, **traemos el bloque completo a la caché** (mismo camino que un miss de lectura) y luego escribimos sobre la copia en caché.

**Por qué:** asumimos localidad espacial — si el programa está escribiendo en una dirección, es probable que vuelva a leer o escribir cerca de ahí pronto (ej: llenar un array). La alternativa (*no-write-allocate*) tendría más sentido si el patrón de acceso fuera predominantemente "escribir una vez y nunca releer" (ej: un buffer de log de solo escritura) — ahí cachear el bloque sería desperdiciar una línea de caché para un dato que no se va a reutilizar.

### 5.3 Regla de Desempate del LRU: vía libre siempre gana

Ante un miss, si **alguna** vía del conjunto tiene `valido == false`, esa se usa **siempre**, sin comparar `ultimo_acceso` — incluso si la otra vía, ocupada, tiene un `ultimo_acceso` numéricamente menor (lo que la haría parecer "más vieja" si se comparara ciegamente).

**Por qué:** no tiene sentido desalojar una línea con datos útiles cuando hay espacio libre sin usar en el mismo conjunto. Comparar `ultimo_acceso` solo es necesario cuando **ambas** vías están ocupadas y hay que decidir cuál sacrificar.

**Desempate secundario (vía libre vs. vía libre):** si ambas vías de un conjunto están libres simultáneamente (caché recién inicializada), se prefiere la Vía 0 por convención de orden de evaluación — decisión arbitraria pero consciente y documentada en el código, no un accidente del `if`.

### 5.4 Momento exacto del incremento de `contador_ciclos`

`contador_ciclos` se incrementa **antes** de cualquier otra operación de la llamada (primer paso del diagrama), y `ultimo_acceso` se asigna usando ese valor ya incrementado. Esto garantiza que cada acceso tenga una marca temporal estrictamente distinta al anterior — crítico para que el desempate LRU sea determinístico en los tests (dos accesos consecutivos nunca pueden terminar con el mismo `ultimo_acceso`).

## 6. API Pública

```rust
impl ControladorMemoria {
    pub fn nuevo() -> Self;

    pub fn decodificar_direccion(&self, direccion: u8) -> (u8, usize, usize); // (tag, indice, offset)
    pub fn reconstruir_direccion_base(&self, tag: u8, indice: usize) -> u8;

    pub fn leer_byte(&mut self, direccion: u8) -> u8;
    pub fn escribir_byte(&mut self, direccion: u8, dato: u8);

    /// Sincroniza todas las líneas dirty a RAM sin invalidarlas (Tarea 3.1).
    pub fn flush(&mut self);
}

impl EstadisticasCache {
    pub fn tasa_de_aciertos(&self) -> f64; // hits / (hits + misses)
}
```

## 7. Tests

El crate cuenta con **15 tests** en `src/tests.rs`, verificados de punta a punta (`rustc --test` sobre el código real):

```
running 15 tests
test tests::test_controlador_nuevo ... ok
test tests::test_decodificar_direccion ... ok
test tests::test_flush_sincroniza_sin_invalidar ... ok
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

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

| Categoría | Tests | Qué verifican |
|---|---|---|
| **Unitarios internos ("caja blanca")** | 8 | `decodificar_direccion`, `reconstruir_direccion_base`, `elegir_via_victima` (LRU y vía libre), `manejar_miss` (carga simple, desalojo limpio, desalojo dirty) llamados directamente, sin pasar por `leer_byte`/`escribir_byte` |
| **Obligatorios (API pública)** | 6 | `test_miss_luego_hit`, `test_write_back_al_desalojar`, `test_lru_desaloja_la_correcta`, `test_offset_correcto`, `test_preferencia_via_libre_sobre_lru`, `test_tasa_de_aciertos` |
| **Extensión (Tarea 3.1)** | 1 | `test_flush_sincroniza_sin_invalidar` |

Correr la suite:
```bash
cargo test --package cache-controller
```


## 8. Errores Comunes al Implementar (Gotchas)

- **Reconstruir la dirección con el tag equivocado:** al desalojar una línea dirty, hay que usar el **tag viejo de la vía víctima** (el que tenía antes de sobreescribirse), nunca el tag de la nueva dirección entrante — son direcciones distintas por definición (si fueran la misma, habría sido un hit, no un miss).
- **Olvidar los contadores de `hits`/`misses`:** son fáciles de pasar por alto porque no afectan el resultado funcional de `leer_byte`/`escribir_byte`, solo rompen `tasa_de_aciertos()` — y el error no se nota hasta correr ese test específico.
- **Escribir directamente a `self.ram` desde `escribir_byte`:** rompe por completo la semántica de Write-Back. La única función que debe tocar `self.ram` en una escritura es la lógica de desalojo dentro de `manejar_miss` (o `flush`).

## 9. Estructura de Archivos de este Crate

```
cache-controller/
├── Cargo.toml
└── src/
    ├── lib.rs        # re-exports públicos + `mod tests;`
    ├── storage.rs     # LineaCache, ConjuntoCache, ControladorMemoria (campos),
    │                  # decodificar_direccion, reconstruir_direccion_base, buscar_via_hit
    ├── policy.rs      # elegir_via_victima, manejar_miss, EstadisticasCache + tasa_de_aciertos
    ├── bus.rs         # leer_byte, escribir_byte, flush (API pública de acceso)
    └── tests.rs       # 15 tests (`#[cfg(test)] mod tests;` declarado en lib.rs)
```