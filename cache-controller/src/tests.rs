use super::*;
use crate::hierarchy::JerarquiaCache;

#[test]
fn test_controlador_nuevo() {
    let mem = ControladorMemoria::nuevo();
    assert_eq!(mem.ram.len(), TAMANO_RAM);
    assert_eq!(mem.contador_ciclos, 0);

    for conjunto in &mem.cache {
        for via in &conjunto.vias {
            assert!(!via.valido, "linea debe arrancar invalida");
        }
    }
}

#[test]
fn test_decodificar_direccion() {
    let mem = ControladorMemoria::nuevo();

    // 0x5A = 0101 1010 -> tag=5, index=2, offset=2
    let (tag, index, offset) = mem.decodificar_direccion(0x5A);
    assert_eq!(tag, 5);
    assert_eq!(index, 2);
    assert_eq!(offset, 2);

    // 0x00 = 0000 0000 -> tag=0, index=0, offset=0
    let (tag, index, offset) = mem.decodificar_direccion(0x00);
    assert_eq!(tag, 0);
    assert_eq!(index, 0);
    assert_eq!(offset, 0);

    // 0xFF = 1111 1111 -> tag=15, index=3, offset=3
    let (tag, index, offset) = mem.decodificar_direccion(0xFF);
    assert_eq!(tag, 15);
    assert_eq!(index, 3);
    assert_eq!(offset, 3);
}

#[test]
fn test_reconstruir_direccion_base() {
    let mem = ControladorMemoria::nuevo();

    // tag=5, index=2 -> 0101 1000 = 0x58
    assert_eq!(mem.reconstruir_direccion_base(5, 2), 0x58);

    // tag=0, index=0 -> 0x00
    assert_eq!(mem.reconstruir_direccion_base(0, 0), 0x00);

    // tag=15, index=3 -> 1111 1100 = 0xFC
    assert_eq!(mem.reconstruir_direccion_base(15, 3), 0xFC);
}

#[test]
fn test_via_victima_lru() {
    let mut mem = ControladorMemoria::nuevo();
    let indice = 0;

    mem.cache[indice].vias[0] = LineaCache {
        tag: 1,
        valido: true,
        dirty_bit: false,
        datos: [0; BLOQUE_BYTES],
        ultimo_acceso: 10,
    };
    mem.cache[indice].vias[1] = LineaCache {
        tag: 2,
        valido: true,
        dirty_bit: false,
        datos: [0; BLOQUE_BYTES],
        ultimo_acceso: 20,
    };

    // LRU: via 0 (acceso 10) es la victima
    assert_eq!(mem.elegir_via_victima(indice), 0);

    mem.contador_ciclos = 30;
    mem.cache[indice].vias[0].ultimo_acceso = 30;
    // Ahora victima es via 1 (acceso 20 < 30)
    assert_eq!(mem.elegir_via_victima(indice), 1);

    mem.contador_ciclos = 40;
    mem.cache[indice].vias[1].ultimo_acceso = 40;
    // Victima vuelve a ser via 0 (acceso 30 < 40)
    assert_eq!(mem.elegir_via_victima(indice), 0);
}

#[test]
fn test_via_victima_con_via_invalida() {
    let mut mem = ControladorMemoria::nuevo();
    let indice = 1;

    mem.cache[indice].vias[0].valido = false;
    mem.cache[indice].vias[1].valido = true;
    mem.cache[indice].vias[1].ultimo_acceso = 10;

    // Via 0 invalida -> elegida directamente sin comparar accesos
    assert_eq!(mem.elegir_via_victima(indice), 0);
}

#[test]
fn test_miss_en_via_invalida_carga_datos() {
    let mut mem = ControladorMemoria::nuevo();
    let indice = 0;
    let tag: u16 = 3; // dir_base = (3 << 4) | (0 << 2) = 0x30 = 48

    mem.ram[48] = 0xAA;
    mem.ram[49] = 0xBB;
    mem.ram[50] = 0xCC;
    mem.ram[51] = 0xDD;

    let via = mem.manejar_miss(indice, tag);

    assert_eq!(via, 0);
    assert!(mem.cache[indice].vias[0].valido);
    assert_eq!(mem.cache[indice].vias[0].tag, tag);
    assert!(!mem.cache[indice].vias[0].dirty_bit);
    assert_eq!(mem.cache[indice].vias[0].datos, [0xAA, 0xBB, 0xCC, 0xDD]);
    assert_eq!(mem.estadisticas.desalojos_dirty, 0);
}

#[test]
fn test_miss_con_via_valida_limpia_no_hace_writeback() {
    let mut mem = ControladorMemoria::nuevo();
    let indice = 1;
    let tag_viejo: u16 = 2; // dir_base = (2<<4)|(1<<2) = 0x24 = 36
    let tag_nuevo: u16 = 5; // dir_base = (5<<4)|(1<<2) = 0x54 = 84

    mem.cache[indice].vias[0] = LineaCache {
        tag: tag_viejo,
        valido: true,
        dirty_bit: false,
        datos: [0x01, 0x02, 0x03, 0x04],
        ultimo_acceso: 5,
    };
    mem.cache[indice].vias[1] = LineaCache {
        tag: 9,
        valido: true,
        dirty_bit: false,
        datos: [0xFF; BLOQUE_BYTES],
        ultimo_acceso: 10,
    };

    mem.ram[84] = 0x11;
    mem.ram[85] = 0x22;
    mem.ram[86] = 0x33;
    mem.ram[87] = 0x44;

    let via = mem.manejar_miss(indice, tag_nuevo);

    assert_eq!(via, 0);
    assert_eq!(mem.cache[indice].vias[0].tag, tag_nuevo);
    assert_eq!(mem.cache[indice].vias[0].datos, [0x11, 0x22, 0x33, 0x44]);
    assert!(!mem.cache[indice].vias[0].dirty_bit);
    // Los datos limpios NO se volcaron a RAM
    assert_ne!(mem.ram[36..40], [0x01, 0x02, 0x03, 0x04]);
    assert_eq!(mem.estadisticas.desalojos_dirty, 0);
}

#[test]
fn test_miss_con_via_dirty_hace_writeback() {
    let mut mem = ControladorMemoria::nuevo();
    let indice = 2;
    let tag_sucio: u16 = 1; // dir_base = (1<<4)|(2<<2) = 0x18 = 24
    let tag_nuevo: u16 = 7; // dir_base = (7<<4)|(2<<2) = 0x78 = 120

    mem.cache[indice].vias[0] = LineaCache {
        tag: tag_sucio,
        valido: true,
        dirty_bit: true,
        datos: [0xDE, 0xAD, 0xBE, 0xEF],
        ultimo_acceso: 1,
    };
    mem.cache[indice].vias[1] = LineaCache {
        tag: 4,
        valido: true,
        dirty_bit: false,
        datos: [0x00; BLOQUE_BYTES],
        ultimo_acceso: 99,
    };

    mem.ram[120] = 0xCA;
    mem.ram[121] = 0xFE;
    mem.ram[122] = 0x00;
    mem.ram[123] = 0x01;

    let via = mem.manejar_miss(indice, tag_nuevo);

    // Write-back verificado
    assert_eq!(
        mem.ram[24..28],
        [0xDE, 0xAD, 0xBE, 0xEF],
        "Write-back a RAM"
    );
    assert_eq!(mem.estadisticas.desalojos_dirty, 1);

    assert_eq!(via, 0);
    assert_eq!(mem.cache[indice].vias[0].tag, tag_nuevo);
    assert_eq!(mem.cache[indice].vias[0].datos, [0xCA, 0xFE, 0x00, 0x01]);
    assert!(mem.cache[indice].vias[0].valido);
    assert!(!mem.cache[indice].vias[0].dirty_bit);
}

#[test]
fn test_miss_luego_hit() {
    let mut mem = ControladorMemoria::nuevo();
    let dir: u16 = 0x10; // tag=1, indice=0, offset=0

    // Primer acceso: miss (cache vacia)
    let _ = mem.leer_byte(dir);
    assert_eq!(mem.estadisticas.misses, 1);
    assert_eq!(mem.estadisticas.hits, 0);

    // Segundo acceso a la misma direccion: hit
    let _ = mem.leer_byte(dir);
    assert_eq!(mem.estadisticas.misses, 1);
    assert_eq!(mem.estadisticas.hits, 1);
}

#[test]
fn test_write_back_al_desalojar() {
    let mut mem = ControladorMemoria::nuevo();
    let dir1: u16 = 0x10; // tag=1, indice=0
    let dir2: u16 = 0x20; // tag=2, indice=0
    let dir3: u16 = 0x30; // tag=3, indice=0

    // Escritura en dir1 marcando dirty en cache y no en RAM
    mem.escribir_byte(dir1, 0xAB);
    let dir_base1 = mem.reconstruir_direccion_base(1, 0) as usize; // 0x10 = 16
    assert_ne!(
        mem.ram[dir_base1], 0xAB,
        "No debe estar en RAM antes del desalojo"
    );

    // Llenado de via 1 con dir2
    let _ = mem.leer_byte(dir2);

    // Acceso a dir3 con el mismo indice forzando desalojo de dir1 por LRU
    let _ = mem.leer_byte(dir3);

    // Comprobacion de que write-back volco 0xAB a RAM
    assert_eq!(
        mem.ram[dir_base1], 0xAB,
        "Write-back debe haber volcado el valor a RAM"
    );
    assert!(mem.estadisticas.desalojos_dirty >= 1);
}

#[test]
fn test_lru_desaloja_la_correcta() {
    let mut mem = ControladorMemoria::nuevo();
    let dir1: u16 = 0x10;
    let dir2: u16 = 0x20;
    let dir3: u16 = 0x30;

    // Llenar las 2 vias del conjunto 0
    let _ = mem.leer_byte(dir1); // via 0, ciclo=1
    let _ = mem.leer_byte(dir2); // via 1, ciclo=2

    // Refrescar dir1 -> dir2 queda como LRU
    let _ = mem.leer_byte(dir1); // via 0, ciclo=3

    // Acceder a dir3 -> debe desalojar dir2 (LRU)
    let _ = mem.leer_byte(dir3);

    let (tag2, indice, _) = mem.decodificar_direccion(dir2);
    assert!(
        mem.buscar_via_hit(indice, tag2).is_none(),
        "dir2 deberia ser desalojada (LRU)"
    );

    let (tag1, _, _) = mem.decodificar_direccion(dir1);
    assert!(
        mem.buscar_via_hit(indice, tag1).is_some(),
        "dir1 debe seguir en cache"
    );
}

#[test]
fn test_offset_correcto() {
    let mut mem = ControladorMemoria::nuevo();
    let dirs: [u16; 4] = [0x10, 0x11, 0x12, 0x13];
    let valores: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xDD];

    for (dir, &val) in dirs.iter().zip(valores.iter()) {
        mem.escribir_byte(*dir, val);
    }
    for (dir, &val) in dirs.iter().zip(valores.iter()) {
        let leido = mem.leer_byte(*dir);
        assert_eq!(
            leido, val,
            "Offset de dir {:#04X} debe devolver {:#04X}",
            dir, val
        );
    }
}

#[test]
fn test_preferencia_via_libre_sobre_lru() {
    let mut mem = ControladorMemoria::nuevo();
    let dir1: u16 = 0x10; // tag=1, indice=0
    let dir2: u16 = 0x20; // tag=2, indice=0

    // Solo un miss: ocupa via 0, via 1 queda invalida
    let _ = mem.leer_byte(dir1);

    let (tag1, indice, _) = mem.decodificar_direccion(dir1);

    // Acceso a dir2: debe usar via 1 (libre), no desalojar dir1
    let _ = mem.leer_byte(dir2);

    assert!(
        mem.buscar_via_hit(indice, tag1).is_some(),
        "La via ocupada no debe ser desalojada cuando hay una via libre"
    );
    assert_eq!(mem.estadisticas.desalojos_dirty, 0);
}

#[test]
fn test_tasa_de_aciertos() {
    let mut mem = ControladorMemoria::nuevo();
    let dir: u16 = 0x10;

    // Sin accesos -> tasa = 0.0
    assert_eq!(mem.estadisticas.tasa_de_aciertos(), 0.0);

    // 1 miss + 3 hits
    let _ = mem.leer_byte(dir);
    let _ = mem.leer_byte(dir);
    let _ = mem.leer_byte(dir);
    let _ = mem.leer_byte(dir);

    assert_eq!(mem.estadisticas.hits, 3);
    assert_eq!(mem.estadisticas.misses, 1);
    let tasa = mem.estadisticas.tasa_de_aciertos();
    assert!(
        (tasa - 0.75).abs() < f64::EPSILON,
        "Tasa esperada: 0.75, obtenida: {}",
        tasa
    );
}

#[test]
fn test_flush_sincroniza_sin_invalidar() {
    let mut mem = ControladorMemoria::nuevo();
    let dir: u16 = 0x10; // tag=1, indice=0, offset=0

    // Escribir un byte -> queda dirty en cache
    mem.escribir_byte(dir, 0xBE);
    let dir_base = mem.reconstruir_direccion_base(1, 0) as usize; // 0x10 = 16

    // Antes del flush: RAM todavia no tiene el valor
    assert_ne!(
        mem.ram[dir_base], 0xBE,
        "RAM no debe tener el valor antes de flush"
    );

    // La linea esta dirty
    let (tag, indice, _) = mem.decodificar_direccion(dir);
    let via_idx = mem
        .buscar_via_hit(indice, tag)
        .expect("debe estar en cache");
    assert!(mem.cache[indice].vias[via_idx].dirty_bit);

    // flush(): sincroniza sin invalidar
    mem.flush();

    // RAM ahora tiene el valor
    assert_eq!(
        mem.ram[dir_base], 0xBE,
        "RAM debe tener el valor despues de flush"
    );

    // La linea sigue valida y el dirty_bit fue limpiado
    let via_idx = mem
        .buscar_via_hit(indice, tag)
        .expect("linea debe seguir en cache tras flush");
    assert!(
        !mem.cache[indice].vias[via_idx].dirty_bit,
        "dirty_bit debe ser false tras flush"
    );
    assert!(
        mem.cache[indice].vias[via_idx].valido,
        "la linea debe seguir valida tras flush"
    );
}

// ---------------------------------------------------------------------------
// Tests NivelL2 — standalone (sin jerarquia conectada)
// ---------------------------------------------------------------------------

/// Carga un bloque en L2 directamente y luego verifica que un segundo acceso
/// a la misma direccion sea un HIT sin tocar la RAM en absoluto.
///
/// dir = 0x0060 → 0b0000_0000_0110_0000
///   offset = 0x60 & 0x03         = 0
///   indice = (0x60 & 0x1C) >> 2  = (0b0110_0000 & 0b0001_1100) >> 2 = 0
///   tag    = 0x60 >> 5           = 3
#[test]
fn test_l2_hit_no_consulta_ram() {
    use crate::storage::{LineaCache, NivelL2, TAMANO_RAM};

    let mut l2 = NivelL2::nuevo();
    let mut ram = [0u8; TAMANO_RAM];

    let dir: u16 = 0x0060; // tag=3, indice=0, offset=0
    let (tag, indice, _) = l2.decodificar_direccion(dir);

    // Inyectamos la linea directamente para simular una carga previa
    l2.cache[indice].vias[0] = LineaCache {
        tag,
        valido: true,
        dirty_bit: false,
        datos: [0xCA, 0xFE, 0xBA, 0xBE],
        ultimo_acceso: 1,
    };
    l2.contador_ciclos = 1;

    // RAM queda en cero — si L2 la consultara devolveria 0x00
    let byte_leido = l2.leer_byte(dir, &mut ram);

    assert_eq!(
        byte_leido, 0xCA,
        "debe devolver el dato de cache, no de RAM"
    );
    assert_eq!(l2.estadisticas.hits, 1, "debe contar un hit");
    assert_eq!(l2.estadisticas.misses, 0, "no debe contar miss");
    // RAM no fue tocada
    assert_eq!(ram[0x60], 0x00, "RAM no debe ser modificada en un hit");
}

/// Primer acceso a una direccion que no esta en L2 → miss.
/// Verifica que el bloque correcto sea traido desde RAM y que las
/// estadisticas sean correctas; el segundo acceso debe ser HIT.
///
/// dir = 0x00A0 → 0b0000_0000_1010_0000
///   offset = 0xA0 & 0x03         = 0
///   indice = (0xA0 & 0x1C) >> 2  = (0b1010_0000 & 0b0001_1100) >> 2 = 0
///   tag    = 0xA0 >> 5           = 5
///   dir_base = (5 << 5) | (0 << 2) = 160 = 0xA0  ✓
#[test]
fn test_l2_miss_trae_bloque_de_ram() {
    use crate::storage::{NivelL2, TAMANO_RAM};

    let mut l2 = NivelL2::nuevo();
    let mut ram = [0u8; TAMANO_RAM];

    let dir: u16 = 0x00A0;
    let dir_base: usize = 0xA0; // 160

    // Datos reconocibles en RAM
    ram[dir_base] = 0x11;
    ram[dir_base + 1] = 0x22;
    ram[dir_base + 2] = 0x33;
    ram[dir_base + 3] = 0x44;

    // L2 vacia → miss
    let byte_leido = l2.leer_byte(dir, &mut ram);

    assert_eq!(byte_leido, 0x11, "primer byte del bloque traido de RAM");
    assert_eq!(l2.estadisticas.misses, 1, "debe contar un miss");
    assert_eq!(l2.estadisticas.hits, 0, "no debe contar hit");

    // El bloque quedo cargado en cache con todos sus bytes
    let (tag, indice, _) = l2.decodificar_direccion(dir);
    let via_idx = l2
        .buscar_via_hit_l2(indice, tag)
        .expect("el bloque debe estar en cache despues del miss");

    let linea = &l2.cache[indice].vias[via_idx];
    assert!(linea.valido, "linea debe ser valida");
    assert!(!linea.dirty_bit, "linea recien cargada no es dirty");
    assert_eq!(
        linea.datos,
        [0x11, 0x22, 0x33, 0x44],
        "bloque completo debe coincidir con lo que habia en RAM"
    );

    // Segundo acceso → hit, sin nuevo miss
    let byte2 = l2.leer_byte(dir, &mut ram);
    assert_eq!(byte2, 0x11);
    assert_eq!(l2.estadisticas.hits, 1, "segundo acceso debe ser hit");
    assert_eq!(l2.estadisticas.misses, 1, "misses no debe crecer");
}

// ---------------------------------------------------------------------------
// Test de migracion de tipo: direcciones u16 en L1
// ---------------------------------------------------------------------------

/// Verifica que `decodificar_direccion` funcione correctamente con
/// direcciones u16 de rango alto (> 0xFF), garantizando que la migracion
/// de u8 a u16 para las instrucciones LOAD/STORE es correcta.
///
/// dir = 0x0190 = 0b0000_0001_1001_0000
///   offset = 0x0190 & 0x03         = 0
///   indice = (0x0190 & 0x0C) >> 2  = (0b0001_1001_0000 & 0b1100) >> 2 = 0
///   tag    = 0x0190 >> 4           = 25 = 0x19
#[test]
fn test_decodificar_direccion_u16_offsets_e_indice_correctos() {
    let mem = ControladorMemoria::nuevo();

    // Direccion en rango alto: 0x0190 = 400
    let dir: u16 = 0x0190;
    let (tag, indice, offset) = mem.decodificar_direccion(dir);
    assert_eq!(offset, 0, "offset de 0x0190 debe ser 0");
    assert_eq!(indice, 0, "indice de 0x0190 debe ser 0");
    assert_eq!(tag, 25, "tag de 0x0190 debe ser 25 (0x19)");

    // dir_base reconstruida debe redondear al inicio del bloque
    let dir_base = mem.reconstruir_direccion_base(tag, indice);
    assert_eq!(
        dir_base, dir,
        "dir_base reconstruida debe coincidir con dir original"
    );

    // Comprobacion simetrica con offset != 0
    let dir2: u16 = 0x0193; // mismo bloque, offset=3
    let (tag2, indice2, offset2) = mem.decodificar_direccion(dir2);
    assert_eq!(tag2, tag, "mismo tag que 0x0190");
    assert_eq!(indice2, indice, "mismo indice que 0x0190");
    assert_eq!(offset2, 3, "offset de 0x0193 debe ser 3");
}

// ---------------------------------------------------------------------------
// Tests de JerarquiaCache — L1 → L2 → RAM
// ---------------------------------------------------------------------------

/// Un HIT en L1 no debe consultar L2 ni modificar sus estadisticas.
///
/// Inyectamos el bloque directamente en L1. Si hay HIT, las estadisticas de L2
/// quedan en cero — prueba de que L2 no fue contactada.
#[test]
fn test_l1_hit_no_consulta_l2() {
    use crate::storage::LineaCache;

    let mut j = JerarquiaCache::nuevo();

    // dir = 0x0010 → tag=1, indice=0, offset=0  (esquema L1: 12/2/2)
    let dir: u16 = 0x0010;
    let (tag, indice, _) = j.l1.decodificar_direccion(dir);

    // Inyectar bloque directamente en L1
    j.l1.cache[indice].vias[0] = LineaCache {
        tag,
        valido: true,
        dirty_bit: false,
        datos: [0xAA, 0xBB, 0xCC, 0xDD],
        ultimo_acceso: 1,
    };
    j.l1.contador_ciclos = 1;

    let byte = j.leer_byte(dir);

    assert_eq!(byte, 0xAA, "debe devolver dato de L1");
    assert_eq!(j.l1.estadisticas.hits, 1, "L1 debe registrar hit");
    assert_eq!(j.l1.estadisticas.misses, 0);

    // L2 no debe haber sido consultada
    assert_eq!(
        j.l2.estadisticas.hits, 0,
        "L2 no debe ser consultada en un hit de L1"
    );
    assert_eq!(
        j.l2.estadisticas.misses, 0,
        "L2 no debe registrar miss en hit de L1"
    );
}

/// Un MISS en L1 con HIT en L2 debe traer el bloque a L1.
///
/// Inyectamos el bloque directamente en L2 y RAM en cero.
/// Luego de la lectura, el bloque debe estar en L1 y L2 debe tener un hit.
#[test]
fn test_l1_miss_l2_hit_trae_bloque_a_l1() {
    use crate::storage::LineaCache;

    let mut j = JerarquiaCache::nuevo();

    // dir = 0x0010  →  L1: tag=1, set=0, offset=0
    //                   L2: decodificar da tag diferente (11b), usamos dir_base
    let dir: u16 = 0x0010;
    let (tag_l1, indice_l1, _) = j.l1.decodificar_direccion(dir);
    let dir_base_l1 = j.l1.reconstruir_direccion_base(tag_l1, indice_l1);

    // Calcular tag/indice en L2 para dir_base_l1
    let (tag_l2, indice_l2, _) = j.l2.decodificar_direccion(dir_base_l1);

    // Inyectar bloque en L2 con datos conocidos
    j.l2.cache[indice_l2].vias[0] = LineaCache {
        tag: tag_l2,
        valido: true,
        dirty_bit: false,
        datos: [0x11, 0x22, 0x33, 0x44],
        ultimo_acceso: 1,
    };
    j.l2.contador_ciclos = 1;
    // RAM queda en cero

    let byte = j.leer_byte(dir);

    // Debe devolver el primer byte del bloque
    assert_eq!(byte, 0x11, "debe devolver dato de L2");

    // L1 debe haber tenido miss y ahora tener el bloque
    assert_eq!(j.l1.estadisticas.misses, 1, "L1 debe registrar miss");
    let via_hit = j.l1.buscar_via_hit(indice_l1, tag_l1);
    assert!(via_hit.is_some(), "el bloque debe haber llegado a L1");
    let via_idx = via_hit.unwrap();
    assert_eq!(
        j.l1.cache[indice_l1].vias[via_idx].datos,
        [0x11, 0x22, 0x33, 0x44]
    );

    // L2 debe haber tenido hit (lo teniamos inyectado)
    assert_eq!(j.l2.estadisticas.hits, 1, "L2 debe registrar hit");
    assert_eq!(j.l2.estadisticas.misses, 0, "L2 no debe tener miss");
}

/// MISS en L1 y MISS en L2: el bloque debe venir desde self.ram.
///
/// Ambas caches vacias. Los datos se colocan en j.ram directamente
/// y tras la lectura el bloque debe quedar tanto en L1 como en L2.
#[test]
fn test_l1_miss_l2_miss_trae_desde_ram() {
    let mut j = JerarquiaCache::nuevo();

    // dir = 0x0020 → L1: tag=2, set=0, offset=0 → dir_base = 0x20 = 32
    let dir: u16 = 0x0020;
    let dir_base: usize = 0x20;

    j.ram[dir_base] = 0xDE;
    j.ram[dir_base + 1] = 0xAD;
    j.ram[dir_base + 2] = 0xBE;
    j.ram[dir_base + 3] = 0xEF;

    let byte = j.leer_byte(dir);

    assert_eq!(byte, 0xDE, "primer byte del bloque desde RAM");

    // L1 debe tener el bloque
    let (tag_l1, indice_l1, _) = j.l1.decodificar_direccion(dir);
    let via =
        j.l1.buscar_via_hit(indice_l1, tag_l1)
            .expect("bloque en L1");
    assert_eq!(
        j.l1.cache[indice_l1].vias[via].datos,
        [0xDE, 0xAD, 0xBE, 0xEF],
        "L1 debe tener el bloque completo"
    );

    let dir_base_l1 = j.l1.reconstruir_direccion_base(tag_l1, indice_l1);
    let (tag_l2, indice_l2, _) = j.l2.decodificar_direccion(dir_base_l1);
    let via_l2 =
        j.l2.buscar_via_hit_l2(indice_l2, tag_l2)
            .expect("bloque en L2");
    assert_eq!(
        j.l2.cache[indice_l2].vias[via_l2].datos,
        [0xDE, 0xAD, 0xBE, 0xEF],
        "L2 debe tener el bloque completo"
    );

    assert_eq!(j.l1.estadisticas.misses, 1, "L1: 1 miss");
    assert_eq!(j.l2.estadisticas.misses, 1, "L2: 1 miss");
}

/// Un desalojo dirty de L1 debe escribirse en L2 y no directamente en RAM.
/// Escribiendo en dir_b1 queda dirty en L1. Al llenar la otra via del set y
/// forzar el desalojo accediendo a dir_b3, se comprueba que el volcado write-back
/// se dirige hacia L2 y no a RAM, dejando en L2 el bloque marcado dirty.
#[test]
fn test_desalojo_dirty_de_l1_escribe_en_l2_no_en_ram() {
    let mut j = JerarquiaCache::nuevo();

    // Tres direcciones que mapean al Set 0 de L1 (index bits 3-2 = 0b00)
    // tag 1 → dir_base = 0x10
    // tag 2 → dir_base = 0x20
    // tag 3 → dir_base = 0x30
    let dir_b1: u16 = 0x0010; // tag=1, set=0, offset=0 → dir_base=0x10
    let dir_b2: u16 = 0x0020; // tag=2, set=0, offset=0
    let dir_b3: u16 = 0x0030; // tag=3, set=0, offset=0 → fuerza desalojo de b1

    // Escritura en b1 marcando dirty en L1
    j.escribir_byte(dir_b1, 0xBB);
    // RAM no debe recibir el dato todavia
    assert_eq!(
        j.ram[0x10], 0x00,
        "RAM no debe ser escrita aun (write-back)"
    );

    // Llenado de via 1 del Set 0
    let _ = j.leer_byte(dir_b2);

    // Desalojo forzado de b1 mediante LRU
    let _ = j.leer_byte(dir_b3);

    // Comprobacion de que el write-back fue a L2 y no a RAM
    assert_eq!(
        j.ram[0x10], 0x00,
        "el desalojo dirty de L1 NO debe escribir directamente en RAM"
    );

    // L2 debe tener el bloque de b1 marcado dirty
    let (tag_l1, indice_l1, _) = j.l1.decodificar_direccion(dir_b1);
    let dir_base_b1 = j.l1.reconstruir_direccion_base(tag_l1, indice_l1);
    let (tag_l2, indice_l2, _) = j.l2.decodificar_direccion(dir_base_b1);
    let via_l2 =
        j.l2.buscar_via_hit_l2(indice_l2, tag_l2)
            .expect("el bloque desalojado de L1 debe estar en L2");
    assert!(
        j.l2.cache[indice_l2].vias[via_l2].dirty_bit,
        "el bloque en L2 debe estar marcado dirty"
    );
    assert_eq!(
        j.l2.cache[indice_l2].vias[via_l2].datos[0], 0xBB,
        "L2 debe tener el dato escrito"
    );
}
