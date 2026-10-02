use cache_controller::{ControladorMemoria, NivelL2, TAMANO_RAM};

fn separador(titulo: &str) {
    println!("\n{}", "─".repeat(60));
    println!("  {}", titulo);
    println!("{}", "─".repeat(60));
}

fn imprimir_estado_cache(mem: &ControladorMemoria) {
    println!("\n  Estado de la caché L1:");
    println!(
        "  {:^8} {:^6} {:^5} {:^5} {:^22} {:^8}",
        "Conjunto", "Vía", "V", "D", "Datos (hex)", "Último acc."
    );
    println!("  {}", "·".repeat(60));
    for (i, conjunto) in mem.cache.iter().enumerate() {
        for (j, via) in conjunto.vias.iter().enumerate() {
            let valido = if via.valido { "✓" } else { "✗" };
            let dirty = if via.dirty_bit { "D" } else { "-" };
            let datos: String = via
                .datos
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "  {:^8} {:^6} {:^5} {:^5} {:^22} {:^8}",
                format!("Set {}", i),
                format!("Vía {}", j),
                valido,
                dirty,
                datos,
                if via.valido {
                    via.ultimo_acceso.to_string()
                } else {
                    "-".to_string()
                }
            );
        }
    }
}

fn imprimir_estado_l2(l2: &NivelL2) {
    println!("\n  Estado de la caché L2 (8 conjuntos × 2 vías):");
    println!(
        "  {:^8} {:^6} {:^5} {:^5} {:^22} {:^8}",
        "Conjunto", "Vía", "V", "D", "Datos (hex)", "Último acc."
    );
    println!("  {}", "·".repeat(60));
    for (i, conjunto) in l2.cache.iter().enumerate() {
        for (j, via) in conjunto.vias.iter().enumerate() {
            let valido = if via.valido { "✓" } else { "✗" };
            let dirty = if via.dirty_bit { "D" } else { "-" };
            let datos: String = via
                .datos
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "  {:^8} {:^6} {:^5} {:^5} {:^22} {:^8}",
                format!("Set {}", i),
                format!("Vía {}", j),
                valido,
                dirty,
                datos,
                if via.valido {
                    via.ultimo_acceso.to_string()
                } else {
                    "-".to_string()
                }
            );
        }
    }
}

fn imprimir_estadisticas(mem: &ControladorMemoria) {
    let total = mem.estadisticas.hits + mem.estadisticas.misses;
    println!("\n  Estadísticas:");
    println!("    Hits           : {}", mem.estadisticas.hits);
    println!("    Misses         : {}", mem.estadisticas.misses);
    println!("    Total accesos  : {}", total);
    println!(
        "    Tasa aciertos  : {:.1}%",
        mem.estadisticas.tasa_de_aciertos() * 100.0
    );
    println!("    Desalojos dirty: {}", mem.estadisticas.desalojos_dirty);
    println!("    Ciclos totales : {}", mem.contador_ciclos);
}

fn imprimir_estadisticas_l2(l2: &NivelL2) {
    let total = l2.estadisticas.hits + l2.estadisticas.misses;
    println!("\n  Estadísticas L2:");
    println!("    Hits           : {}", l2.estadisticas.hits);
    println!("    Misses         : {}", l2.estadisticas.misses);
    println!("    Total accesos  : {}", total);
    println!(
        "    Tasa aciertos  : {:.1}%",
        l2.estadisticas.tasa_de_aciertos() * 100.0
    );
    println!("    Desalojos dirty: {}", l2.estadisticas.desalojos_dirty);
    println!("    Ciclos totales : {}", l2.contador_ciclos);
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║      Simulador de Caché Asociativa por Conjuntos         ║");
    println!("║  L1: 4 sets × 2 vías  ·  L2: 8 sets × 2 vías           ║");
    println!("║        LRU · Write-Back / Write-Allocate                 ║");
    println!("╚══════════════════════════════════════════════════════════╝");

    let mut mem = ControladorMemoria::nuevo();

    separador("Demo 1: Miss seguido de Hit (localidad temporal)");

    println!("\n  Inicializando RAM con patrón de datos...");
    for i in 0u8..=255u8 {
        mem.ram[i as usize] = i.wrapping_mul(3);
    }

    println!("  → leer_byte(0x10)  [tag=1, set=0, offset=0] — primer acceso: MISS esperado");
    let v1 = mem.leer_byte(0x10);
    println!(
        "    Valor leído: 0x{:02X}  |  hits={} misses={}",
        v1, mem.estadisticas.hits, mem.estadisticas.misses
    );

    println!("  → leer_byte(0x10)  — mismo bloque, segundo acceso: HIT esperado");
    let v2 = mem.leer_byte(0x10);
    println!(
        "    Valor leído: 0x{:02X}  |  hits={} misses={}",
        v2, mem.estadisticas.hits, mem.estadisticas.misses
    );

    println!("  → leer_byte(0x12)  [mismo bloque, offset=2] — HIT esperado (bloque ya cargado)");
    let v3 = mem.leer_byte(0x12);
    println!(
        "    Valor leído: 0x{:02X}  |  hits={} misses={}",
        v3, mem.estadisticas.hits, mem.estadisticas.misses
    );

    imprimir_estado_cache(&mem);

    separador("Demo 2: Write-Back — dato sucio volcado al desalojar");

    let mut mem = ControladorMemoria::nuevo();

    println!("  → escribir_byte(0x10, 0xAB)  — escribe en caché (dirty), RAM intacta");
    mem.escribir_byte(0x10, 0xAB);
    let base1 = mem.reconstruir_direccion_base(1, 0) as usize;
    println!(
        "    RAM[0x{:02X}] = 0x{:02X}  (debe ser 0x00, no llegó aún)",
        base1, mem.ram[base1]
    );

    println!("  → leer_byte(0x20)  — llena la vía 1 del Set 0");
    let _ = mem.leer_byte(0x20);

    println!("  → leer_byte(0x30)  — fuerza desalojo del bloque 0x10 (LRU), Write-Back activado");
    let _ = mem.leer_byte(0x30);
    println!(
        "    RAM[0x{:02X}] = 0x{:02X}  (debe ser 0xAB, ya volcado)",
        base1, mem.ram[base1]
    );
    println!("    Desalojos dirty: {}", mem.estadisticas.desalojos_dirty);

    imprimir_estado_cache(&mem);

    separador("Demo 3: Política LRU — se desaloja el menos usado recientemente");

    let mut mem = ControladorMemoria::nuevo();

    println!("  → leer_byte(0x10)  — carga tag=1 en Set 0, Vía 0  (ciclo 1)");
    let _ = mem.leer_byte(0x10);

    println!("  → leer_byte(0x20)  — carga tag=2 en Set 0, Vía 1  (ciclo 2)");
    let _ = mem.leer_byte(0x20);

    println!("  → leer_byte(0x10)  — HIT en tag=1, lo refresca      (ciclo 3)");
    let _ = mem.leer_byte(0x10);

    println!(
        "  → leer_byte(0x30)  — MISS tag=3, Set 0 lleno → LRU desaloja tag=2 (ciclo 4 < ciclo 3)"
    );
    let _ = mem.leer_byte(0x30);

    let (t1, s, _) = mem.decodificar_direccion(0x10);
    let (t2, _, _) = mem.decodificar_direccion(0x20);
    println!(
        "    tag=1 en caché: {}",
        if mem.buscar_via_hit(s, t1).is_some() {
            "SÍ ✓"
        } else {
            "NO ✗"
        }
    );
    println!(
        "    tag=2 en caché: {} (fue desalojado por LRU)",
        if mem.buscar_via_hit(s, t2).is_some() {
            "SÍ"
        } else {
            "NO ✓"
        }
    );

    imprimir_estado_cache(&mem);

    separador("Demo 4: flush() — vuelca dirty bits a RAM sin invalidar la caché");

    let mut mem = ControladorMemoria::nuevo();

    println!("  → Escribiendo en varias direcciones (quedan dirty en caché)...");
    mem.escribir_byte(0x10, 0xCA);
    mem.escribir_byte(0x11, 0xFE);
    mem.escribir_byte(0x44, 0xBE);
    mem.escribir_byte(0xA8, 0xEF);

    let base_10 = mem.reconstruir_direccion_base(1, 0) as usize;
    println!(
        "    Antes del flush → RAM[0x{:02X}] = 0x{:02X}  (no volcado)",
        base_10, mem.ram[base_10]
    );

    println!("  → flush()");
    mem.flush();

    println!(
        "    Después del flush → RAM[0x{:02X}] = 0x{:02X}  (volcado ✓)",
        base_10, mem.ram[base_10]
    );
    println!(
        "    La caché sigue válida: {}",
        if mem.buscar_via_hit(0, 1).is_some() {
            "SÍ ✓"
        } else {
            "NO ✗"
        }
    );

    let (tag, indice, _) = mem.decodificar_direccion(0x10);
    let via = mem.buscar_via_hit(indice, tag).unwrap();
    println!(
        "    dirty_bit tras flush: {}  (debe ser false)",
        mem.cache[indice].vias[via].dirty_bit
    );

    imprimir_estado_cache(&mem);

    separador("Demo 5: Localidad espacial — recorrer un array en memoria");

    let mut mem = ControladorMemoria::nuevo();

    println!("  Inicializando array de 16 bytes en RAM a partir de 0x20...");
    for i in 0u16..16 {
        mem.ram[0x20 + i as usize] = (i * 10) as u8;
    }

    println!("  → Sumando todos los elementos del array:");
    let mut suma: u32 = 0;
    for i in 0u16..16 {
        let val = mem.leer_byte(0x20 + i) as u32;
        suma += val;
    }
    println!(
        "    Suma = {}  (esperado: {})",
        suma,
        (0u32..16).map(|i| i * 10).sum::<u32>()
    );

    imprimir_estadisticas(&mem);
    imprimir_estado_cache(&mem);

    separador("Demo 6: NivelL2 standalone — 8 conjuntos × 2 vías, tag=11b/index=3b/offset=2b");

    println!();
    println!("  Esquema de bits de la dirección en L2 (16 bits):");
    println!("  ┌─────────────┬───────────┬──────────┐");
    println!("  │  Tag (11b)  │ Index (3b)│ Offset(2b)│");
    println!("  ├─────────────┼───────────┼──────────┤");
    println!("  │  Bit 15..5  │ Bit 4..2  │ Bit 1..0  │");
    println!("  └─────────────┴───────────┴──────────┘");
    println!("  (Distinto de L1: tag=12b / index=2b — cada nivel tiene su propio decodificador)");

    let mut l2 = NivelL2::nuevo();
    let mut ram = [0u8; TAMANO_RAM];

    println!("\n  Inicializando RAM con patrón i*7 para i en 0..256...");
    for i in 0u16..256 {
        ram[i as usize] = (i as u8).wrapping_mul(7);
    }

    println!("\n  [6a] Miss → Hit (localidad temporal)");
    let dir_a: u16 = 0x0060;
    let (tag_a, idx_a, off_a) = l2.decodificar_direccion(dir_a);
    println!(
        "  → leer_byte(0x{:04X})  [tag={}, set={}, offset={}] — MISS esperado",
        dir_a, tag_a, idx_a, off_a
    );
    let v_a1 = l2.leer_byte(dir_a, &mut ram);
    println!(
        "    Valor leído: 0x{:02X}  (RAM[0x60]=0x{:02X})  |  hits={} misses={}",
        v_a1, ram[0x60], l2.estadisticas.hits, l2.estadisticas.misses
    );

    println!("  → leer_byte(0x{:04X}) de nuevo — HIT esperado", dir_a);
    let v_a2 = l2.leer_byte(dir_a, &mut ram);
    println!(
        "    Valor leído: 0x{:02X}  |  hits={} misses={}",
        v_a2, l2.estadisticas.hits, l2.estadisticas.misses
    );

    let dir_a2: u16 = dir_a + 2;
    println!(
        "  → leer_byte(0x{:04X}) [mismo bloque, offset=2] — HIT esperado",
        dir_a2
    );
    let v_a3 = l2.leer_byte(dir_a2, &mut ram);
    println!(
        "    Valor leído: 0x{:02X}  |  hits={} misses={}",
        v_a3, l2.estadisticas.hits, l2.estadisticas.misses
    );

    println!("\n  [6b] Write-Back en L2 — dato dirty volcado al desalojar");
    let mut l2b = NivelL2::nuevo();
    let mut ram_b = [0u8; TAMANO_RAM];

    let dir_b1: u16 = 0x0060;
    let dir_b2: u16 = 0x0080;
    let dir_b3: u16 = 0x00A0;

    println!(
        "  → escribir_byte(0x{:04X}, 0xBB) — escribe en L2 (dirty), RAM intacta",
        dir_b1
    );
    l2b.escribir_byte(dir_b1, 0xBB, &mut ram_b);
    println!(
        "    RAM[0x60] = 0x{:02X}  (debe ser 0x00, no llegó aún)",
        ram_b[0x60]
    );

    println!(
        "  → leer_byte(0x{:04X}) — llena la vía 1 del Set 0 de L2",
        dir_b2
    );
    let _ = l2b.leer_byte(dir_b2, &mut ram_b);

    println!(
        "  → leer_byte(0x{:04X}) — Set 0 lleno → LRU desaloja 0x{:04X} (dirty) → Write-Back",
        dir_b3, dir_b1
    );
    let _ = l2b.leer_byte(dir_b3, &mut ram_b);
    println!(
        "    RAM[0x60] = 0x{:02X}  (debe ser 0xBB, ya volcado ✓)",
        ram_b[0x60]
    );
    println!(
        "    Desalojos dirty L2: {}",
        l2b.estadisticas.desalojos_dirty
    );

    println!("\n  [6c] Localidad espacial en L2 — leer 8 bytes consecutivos (2 bloques)");
    let mut l2c = NivelL2::nuevo();
    let mut ram_c = [0u8; TAMANO_RAM];
    for i in 0u16..8 {
        ram_c[0xC0 + i as usize] = (i as u8) * 11;
    }
    let mut suma_c: u32 = 0;
    for i in 0u16..8 {
        suma_c += l2c.leer_byte(0x00C0 + i, &mut ram_c) as u32;
    }
    println!(
        "    Suma = {}  (esperado: {})",
        suma_c,
        (0u32..8).map(|i| i * 11).sum::<u32>()
    );
    println!(
        "    Misses={} (esperado 2: un miss por bloque de 4 bytes), Hits={}",
        l2c.estadisticas.misses, l2c.estadisticas.hits
    );

    imprimir_estadisticas_l2(&l2);
    imprimir_estado_l2(&l2);

    separador("Resumen final de todas las demos");
    println!();
    println!("  El simulador demostró:");
    println!("   ✓  [L1] Miss en primer acceso, Hit en accesos repetidos (localidad temporal)");
    println!("   ✓  [L1] Write-Back: los datos dirty solo llegan a RAM al desalojar");
    println!("   ✓  [L1] LRU: el bloque menos usado recientemente es el desalojado");
    println!("   ✓  [L1] flush(): sincroniza la caché con RAM sin invalidar líneas");
    println!("   ✓  [L1] Localidad espacial: 16 bytes → solo 4 misses (1 por bloque de 4)");
    println!("   ✓  [L2] Decodificación propia: tag=11b / index=3b / offset=2b");
    println!("   ✓  [L2] Miss → Hit con localidad temporal");
    println!("   ✓  [L2] Write-Back: dato dirty volcado a RAM al desalojar");
    println!("   ✓  [L2] Localidad espacial: 8 bytes → solo 2 misses");
    println!("   ✓  [L2] RAM pasada por parámetro → lista para JerarquiaCache (Tarea 4)");
    println!();
}
