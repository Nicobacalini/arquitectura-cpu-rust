use cache_controller::{Mmu, calcular_amat};
use cpu_pipeline::CpuSegmentada;
use std::fmt::Write;

/// Genera un reporte formateado con el estado final de la CPU,
/// las metricas de rendimiento del pipeline, las estadisticas separadas
/// de TLB, L1 y L2, y el AMAT extendido con penalidades de memoria virtual.
pub fn reporte_rendimiento(
    cpu: &CpuSegmentada,
    mmu: &Mmu,
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

    // Estadísticas TLB
    let tlb = &mmu.tlb;
    let total_tlb = tlb.hits + tlb.misses;
    let tasa_tlb = tlb.tasa_hits() * 100.0;
    let tasa_miss_tlb = if total_tlb > 0 {
        tlb.misses as f64 / total_tlb as f64
    } else {
        0.0
    };

    // Estadísticas L1
    let l1 = mmu.estadisticas_l1();
    let total_l1 = l1.hits + l1.misses;
    let tasa_l1 = if total_l1 > 0 {
        l1.hits as f64 / total_l1 as f64 * 100.0
    } else {
        0.0
    };
    let tasa_miss_l1 = if total_l1 > 0 {
        l1.misses as f64 / total_l1 as f64
    } else {
        0.0
    };

    // Estadísticas L2
    let l2 = mmu.estadisticas_l2();
    let total_l2 = l2.hits + l2.misses;
    let tasa_l2 = if total_l2 > 0 {
        l2.hits as f64 / total_l2 as f64 * 100.0
    } else {
        0.0
    };
    let tasa_miss_l2 = if total_l2 > 0 {
        l2.misses as f64 / total_l2 as f64
    } else {
        0.0
    };

    // AMAT L1 (formula de la Fase 1)
    // Tiempos representativos de juguete: L1=1, L2=10, RAM=100 ciclos
    let tiempo_l1: f64 = 1.0;
    let tiempo_l2: f64 = 10.0;
    let tiempo_ram: f64 = 100.0;
    let amat_l1 = tiempo_l1 + tasa_miss_l1 * (tiempo_l2 + tasa_miss_l2 * tiempo_ram);

    // AMAT extendido (formula de la Fase 2)
    // Tiempos TLB: 1 ciclo para el lookup, penalidad_tlb_miss para ir a Page Table
    let tiempo_tlb: f64 = 1.0;
    let tiempo_page_table: f64 = mmu.penalidad_tlb_miss as f64;
    let total_accesos = total_tlb;
    let tasa_page_fault = if total_accesos > 0 {
        mmu.page_faults as f64 / total_accesos as f64
    } else {
        0.0
    };
    let amat_total = calcular_amat(
        tiempo_tlb,
        tasa_miss_tlb,
        tiempo_page_table,
        tasa_page_fault,
        mmu.penalidad_page_fault as f64,
        amat_l1,
    );

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

    // ── Estadísticas TLB ─────────────────────────────────────────────────────
    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "                     Estadisticas TLB                       "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(out, "  Capacidad (entradas)      : {}", tlb.capacidad);
    let _ = writeln!(out, "  Hits                      : {}", tlb.hits);
    let _ = writeln!(out, "  Misses                    : {}", tlb.misses);
    let _ = writeln!(out, "  Total accesos             : {}", total_tlb);
    let _ = writeln!(out, "  Tasa de aciertos TLB      : {:.2}%", tasa_tlb);
    let _ = writeln!(out, "  Page faults               : {}", mmu.page_faults);
    let _ = writeln!(
        out,
        "  Violaciones de proteccion : {}",
        mmu.violaciones_proteccion
    );
    let _ = writeln!(out, "  ASID actual               : {}", mmu.asid_actual);

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
        mmu.jerarquia.l1.contador_ciclos
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

    // ── AMAT extendido (Fase 2) ───────────────────────────────────────────────
    let _ = writeln!(
        out,
        "\n============================================================"
    );
    let _ = writeln!(
        out,
        "              AMAT Extendido (TLB + Cache + RAM)             "
    );
    let _ = writeln!(
        out,
        "============================================================"
    );
    let _ = writeln!(out, "  T_TLB (ciclos)            : {:.1}", tiempo_tlb);
    let _ = writeln!(
        out,
        "  T_PageTable (ciclos)      : {:.1}",
        tiempo_page_table
    );
    let _ = writeln!(
        out,
        "  Penalidad PageFault (cic) : {}",
        mmu.penalidad_page_fault
    );
    let _ = writeln!(
        out,
        "  Tasa miss TLB             : {:.4}",
        tasa_miss_tlb
    );
    let _ = writeln!(
        out,
        "  Tasa page fault           : {:.6}",
        tasa_page_fault
    );
    let _ = writeln!(out, "  AMAT_L1 (ciclos)          : {:.4}", amat_l1);
    let _ = writeln!(
        out,
        "  AMAT_total (ciclos)       : {:.4}",
        amat_total
    );
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

        let mut mmu = Mmu::nueva();
        // Simular 3 hits y 1 miss en L1
        mmu.jerarquia.l1.estadisticas.hits = 3;
        mmu.jerarquia.l1.estadisticas.misses = 1;
        mmu.jerarquia.l1.contador_ciclos = 40;
        // Simular 1 hit y 1 miss en L2
        mmu.jerarquia.l2.estadisticas.hits = 1;
        mmu.jerarquia.l2.estadisticas.misses = 1;
        // Simular 2 hits y 1 miss en TLB
        mmu.tlb.hits = 2;
        mmu.tlb.misses = 1;
        mmu.page_faults = 1;

        let reporte = reporte_rendimiento(&cpu, &mmu, 100.0);

        assert!(reporte.contains("Ciclos totales de CPU     : 10"));
        assert!(reporte.contains("Instrucciones completadas : 5"));
        assert!(reporte.contains("CPI (Ciclos / Instruccion): 2.00"));
        assert!(reporte.contains("IPC (Instrucciones / Ciclo): 0.50"));
        assert!(reporte.contains("Tiempo de ejecucion       : 100.00 ns"));
        // Verificar secciones separadas de TLB, L1 y L2
        assert!(reporte.contains("Estadisticas TLB"));
        assert!(reporte.contains("Estadisticas L1"));
        assert!(reporte.contains("Estadisticas L2"));
        assert!(reporte.contains("AMAT Extendido"));
        // Verificar datos TLB
        assert!(reporte.contains("Page faults               : 1"));
        // Verificar datos L1
        assert!(reporte.contains("Tasa de aciertos          : 75.00%"));
    }

    #[test]
    fn test_amat_incluye_penalidad_de_page_fault() {
        // AMAT_TLB = 1 + 0.5 * (10 + 0.1 * 1_000_000) = 1 + 0.5 * 100010 = 50006
        // AMAT_total = 50006 + 1 = 50007
        let amat = calcular_amat(
            1.0,         // tiempo_tlb
            0.5,         // tasa_miss_tlb
            10.0,        // tiempo_page_table
            0.1,         // tasa_page_fault
            1_000_000.0, // penalidad_page_fault
            1.0,         // amat_l1 (simplificado)
        );
        // 1 + 0.5*(10 + 0.1*1_000_000) + 1 = 2 + 0.5*100010 = 2 + 50005 = 50007
        assert!(
            (amat - 50007.0).abs() < 0.01,
            "AMAT esperado ~50007, obtenido {}",
            amat
        );
    }
}
