use cache_controller::ControladorMemoria;

fn separador(titulo: &str) {
    println!("\n{}", "─".repeat(60));
    println!("  {}", titulo);
    println!("{}", "─".repeat(60));
}

fn imprimir_estado_cache(mem: &ControladorMemoria) {
    println!("\n  Estado de la caché:");
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

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║        Simulador de Caché Asociativa por Conjuntos       ║");
    println!("║   4 conjuntos × 2 vías · LRU · Write-Back/Write-Allocate║");
    println!("╚══════════════════════════════════════════════════════════╝");

    let mut mem = ControladorMemoria::nuevo();

    // -----------------------------------------------------------------------
    // Demo 1: Miss → Hit (localidad temporal)
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Demo 2: Write-Back en desalojo
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Demo 3: Politica LRU
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Demo 4: flush() — sincroniza sin invalidar
    // -----------------------------------------------------------------------
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

    // Verificar que dirty_bit se limpio
    let (tag, indice, _) = mem.decodificar_direccion(0x10);
    let via = mem.buscar_via_hit(indice, tag).unwrap();
    println!(
        "    dirty_bit tras flush: {}  (debe ser false)",
        mem.cache[indice].vias[via].dirty_bit
    );

    imprimir_estado_cache(&mem);

    // -----------------------------------------------------------------------
    // Demo 5: patron de acceso secuencial (localidad espacial)
    // -----------------------------------------------------------------------
    separador("Demo 5: Localidad espacial — recorrer un array en memoria");

    let mut mem = ControladorMemoria::nuevo();

    // Simular array de 16 bytes a partir de 0x20 (tag=2, set=0..3)
    println!("  Inicializando array de 16 bytes en RAM a partir de 0x20...");
    for i in 0u8..16 {
        mem.ram[0x20 + i as usize] = i * 10;
    }

    println!("  → Sumando todos los elementos del array:");
    let mut suma: u32 = 0;
    for i in 0u8..16 {
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

    // -----------------------------------------------------------------------
    // Resumen final
    // -----------------------------------------------------------------------
    separador("Resumen final de todas las demos");
    println!();
    println!("  El simulador demostró:");
    println!("   ✓  Miss en primer acceso, Hit en accesos repetidos (localidad temporal)");
    println!("   ✓  Write-Back: los datos dirty solo llegan a RAM al desalojar");
    println!("   ✓  LRU: el bloque menos usado recientemente es el desalojado");
    println!("   ✓  flush(): sincroniza la caché con RAM sin invalidar líneas");
    println!("   ✓  Localidad espacial: 16 bytes → solo 4 misses (1 por bloque de 4)");
    println!();
}
