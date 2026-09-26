use cache_controller::JerarquiaCache;
use cpu_pipeline::CpuSegmentada;
use std::fmt::Write;

/// Genera un reporte formateado con el estado final de la CPU,
/// las metricas de rendimiento del pipeline y las estadisticas separadas
/// de L1 y L2 de la jerarquia de cache.
pub fn reporte_rendimiento(
    cpu: &CpuSegmentada,
    jerarquia: &JerarquiaCache,
    frecuencia_mhz: f64,
) -> String {
    let ciclos_totales = cpu.contador_ciclos;
    let instrucciones = cpu.instrucciones_completadas;

    let cpi = if instrucciones > 0 {
        ciclos_totales as f64 / instrucciones as f64
    } else {
        0.0
    };

    let ipc = if ciclos_totales > 0 {
        instrucciones as f64 / ciclos_totales as f64
    } else {
        0.0
    };

    let tiempo_ns = if frecuencia_mhz > 0.0 {
        (ciclos_totales as f64 / (frecuencia_mhz * 1_000_000.0)) * 1_000_000_000.0
    } else {
        0.0
    };

    // Estadísticas L1 (viven en jerarquia.l1.estadisticas)
    let l1 = &jerarquia.l1.estadisticas;
    let total_l1 = l1.hits + l1.misses;
    let tasa_l1 = if total_l1 > 0 {
        l1.hits as f64 / total_l1 as f64 * 100.0
    } else {
        0.0
    };

    // Estadísticas L2 (viven en jerarquia.l2.estadisticas)
    let l2 = &jerarquia.l2.estadisticas;
    let total_l2 = l2.hits + l2.misses;
    let tasa_l2 = if total_l2 > 0 {
        l2.hits as f64 / total_l2 as f64 * 100.0
    } else {
        0.0
    };

    let mut out = String::new();

    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "                    Estado Final de la CPU                  "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(out, "  Ciclos totales de CPU     : {}", ciclos_totales);
    let _ = writeln!(out, "  Instrucciones completadas : {}", instrucciones);
    let _ = writeln!(out, "  Banco de registros        : {:?}", cpu.registros);
    let _ = writeln!(out, "    R0 = {} (hardwired zero)", cpu.registros[0]);
    let _ = writeln!(out, "    R1 = {}", cpu.registros[1]);
    let _ = writeln!(out, "    R2 = {}", cpu.registros[2]);
    let _ = writeln!(out, "    R3 = {}", cpu.registros[3]);

    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "                   Metricas de Rendimiento                  "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(
        out,
        "  Frecuencia configurada    : {:.2} MHz",
        frecuencia_mhz
    );
    let _ = writeln!(out, "  CPI (Ciclos / Instruccion): {:.2}", cpi);
    let _ = writeln!(out, "  IPC (Instrucciones / Ciclo): {:.2}", ipc);
    let _ = writeln!(
        out,
        "  Tiempo de ejecucion       : {:.2} ns ({:.4} µs)",
        tiempo_ns,
        tiempo_ns / 1_000.0
    );

    // ── Estadísticas L1 ──────────────────────────────────────────────────────
    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "                     Estadisticas L1                        "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(out, "  Hits                      : {}", l1.hits);
    let _ = writeln!(out, "  Misses                    : {}", l1.misses);
    let _ = writeln!(out, "  Total accesos             : {}", total_l1);
    let _ = writeln!(out, "  Tasa de aciertos          : {:.2}%", tasa_l1);
    let _ = writeln!(out, "  Desalojos dirty           : {}", l1.desalojos_dirty);
    let _ = writeln!(
        out,
        "  Ciclos de L1              : {}",
        jerarquia.l1.contador_ciclos
    );

    // ── Estadísticas L2 ──────────────────────────────────────────────────────
    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "                     Estadisticas L2                        "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(out, "  Hits                      : {}", l2.hits);
    let _ = writeln!(out, "  Misses                    : {}", l2.misses);
    let _ = writeln!(out, "  Total accesos             : {}", total_l2);
    let _ = writeln!(out, "  Tasa de aciertos          : {:.2}%", tasa_l2);
    let _ = writeln!(out, "  Desalojos dirty           : {}", l2.desalojos_dirty);
    let _ = writeln!(
        out,
        "============================================================\n"
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reporte_rendimiento_metricas() {
        let cpu = CpuSegmentada {
            registros: [0, 15, 25, 10],
            program_counter: 5,
            contador_ciclos: 10,
            instrucciones_completadas: 5,
            ..CpuSegmentada::nueva()
        };

        let mut jerarquia = JerarquiaCache::nuevo();
        // Simular 3 hits y 1 miss en L1
        jerarquia.l1.estadisticas.hits = 3;
        jerarquia.l1.estadisticas.misses = 1;
        jerarquia.l1.contador_ciclos = 40;
        // Simular 1 hit y 1 miss en L2
        jerarquia.l2.estadisticas.hits = 1;
        jerarquia.l2.estadisticas.misses = 1;

        let reporte = reporte_rendimiento(&cpu, &jerarquia, 100.0);

        assert!(reporte.contains("Ciclos totales de CPU     : 10"));
        assert!(reporte.contains("Instrucciones completadas : 5"));
        assert!(reporte.contains("CPI (Ciclos / Instruccion): 2.00"));
        assert!(reporte.contains("IPC (Instrucciones / Ciclo): 0.50"));
        assert!(reporte.contains("Tiempo de ejecucion       : 100.00 ns"));
        // Verificar secciones separadas de L1 y L2
        assert!(reporte.contains("Estadisticas L1"));
        assert!(reporte.contains("Estadisticas L2"));
        // Verificar datos L1
        assert!(reporte.contains("Hits                      : 3"));
        assert!(reporte.contains("Misses                    : 1"));
        assert!(reporte.contains("Tasa de aciertos          : 75.00%"));
    }
}
