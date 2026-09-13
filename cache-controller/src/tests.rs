use super::*;

#[test]
fn test_controlador_nuevo() {
    let mem = ControladorMemoria::nuevo();
    assert_eq!(mem.ram.len(), 256);
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
    let tag: u8 = 3; // dir_base = (3 << 4) | (0 << 2) = 0x30 = 48

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
    let tag_viejo: u8 = 2; // dir_base = (2<<4)|(1<<2) = 0x24 = 36
    let tag_nuevo: u8 = 5; // dir_base = (5<<4)|(1<<2) = 0x54 = 84

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
    let tag_sucio: u8 = 1; // dir_base = (1<<4)|(2<<2) = 0x18 = 24
    let tag_nuevo: u8 = 7; // dir_base = (7<<4)|(2<<2) = 0x78 = 120

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
    let dir: u8 = 0x10; // tag=1, indice=0, offset=0

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
    let dir1: u8 = 0x10; // tag=1, indice=0
    let dir2: u8 = 0x20; // tag=2, indice=0
    let dir3: u8 = 0x30; // tag=3, indice=0

    // 1. Escribir en dir1 -> dirty en cache, NO en RAM
    mem.escribir_byte(dir1, 0xAB);
    let dir_base1 = mem.reconstruir_direccion_base(1, 0) as usize; // 0x10 = 16
    assert_ne!(
        mem.ram[dir_base1], 0xAB,
        "No debe estar en RAM antes del desalojo"
    );

    // 2. Llenar via 1 con dir2
    let _ = mem.leer_byte(dir2);

    // 3. Acceder a dir3 (mismo indice=0) -> fuerza desalojo de dir1 (LRU)
    let _ = mem.leer_byte(dir3);

    // 4. Write-back debe haber volcado 0xAB a RAM
    assert_eq!(
        mem.ram[dir_base1], 0xAB,
        "Write-back debe haber volcado el valor a RAM"
    );
    assert!(mem.estadisticas.desalojos_dirty >= 1);
}

#[test]
fn test_lru_desaloja_la_correcta() {
    let mut mem = ControladorMemoria::nuevo();
    let dir1: u8 = 0x10;
    let dir2: u8 = 0x20;
    let dir3: u8 = 0x30;

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
    let dirs: [u8; 4] = [0x10, 0x11, 0x12, 0x13];
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
    let dir1: u8 = 0x10; // tag=1, indice=0
    let dir2: u8 = 0x20; // tag=2, indice=0

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
    let dir: u8 = 0x10;

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
    let dir: u8 = 0x10; // tag=1, indice=0, offset=0

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
