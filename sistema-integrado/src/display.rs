use cache_controller::ControladorMemoria;
use cpu_pipeline::CpuSegmentada;
use std::fmt::Write;

/// Genera un reporte formateado con el estado final de la CPU,
/// las metricas de rendimiento del pipeline y las estadisticas de la cache.
pub fn reporte_rendimiento(
    cpu: &CpuSegmentada,
    mem: &ControladorMemoria,
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

    let total_accesos = mem.estadisticas.hits + mem.estadisticas.misses;
    let tasa_aciertos = mem.estadisticas.tasa_de_aciertos() * 100.0;

    let mut out = String::new();

    let _ = writeln!(out, "\n============================================================");
    let _ = writeln!(out, "                    Estado Final de la CPU                  ");
    let _ = writeln!(out, "============================================================");
    let _ = writeln!(out, "  Ciclos totales de CPU     : {}", ciclos_totales);
    let _ = writeln!(out, "  Instrucciones completadas : {}", instrucciones);
    let _ = writeln!(out, "  Banco de registros        : {:?}", cpu.registros);
    let _ = writeln!(out, "    R0 = {} (hardwired zero)", cpu.registros[0]);
    let _ = writeln!(out, "    R1 = {}", cpu.registros[1]);
    let _ = writeln!(out, "    R2 = {}", cpu.registros[2]);
    let _ = writeln!(out, "    R3 = {}", cpu.registros[3]);

    let _ = writeln!(out, "\n============================================================");
    let _ = writeln!(out, "                   Metricas de Rendimiento                  ");
    let _ = writeln!(out, "============================================================");
    let _ = writeln!(out, "  Frecuencia configurada    : {:.2} MHz", frecuencia_mhz);
    let _ = writeln!(out, "  CPI (Ciclos / Instruccion): {:.2}", cpi);
    let _ = writeln!(out, "  IPC (Instrucciones / Ciclo): {:.2}", ipc);
    let _ = writeln!(
        out,
        "  Tiempo de ejecucion       : {:.2} ns ({:.4} µs)",
        tiempo_ns,
        tiempo_ns / 1_000.0
    );

    let _ = writeln!(out, "\n============================================================");
    let _ = writeln!(out, "                    Estadisticas de Cache                   ");
    let _ = writeln!(out, "============================================================");
    let _ = writeln!(out, "  Hits                      : {}", mem.estadisticas.hits);
    let _ = writeln!(out, "  Misses                    : {}", mem.estadisticas.misses);
    let _ = writeln!(out, "  Total accesos             : {}", total_accesos);
    let _ = writeln!(out, "  Tasa de aciertos          : {:.2}%", tasa_aciertos);
    let _ = writeln!(
        out,
        "  Desalojos dirty           : {}",
        mem.estadisticas.desalojos_dirty
    );
    let _ = writeln!(out, "  Ciclos de cache           : {}", mem.contador_ciclos);
    let _ = writeln!(out, "============================================================\n");

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

        let mut mem = ControladorMemoria::nuevo();
        mem.estadisticas.hits = 3;
        mem.estadisticas.misses = 1;
        mem.contador_ciclos = 40;

        let reporte = reporte_rendimiento(&cpu, &mem, 100.0);

        assert!(reporte.contains("Ciclos totales de CPU     : 10"));
        assert!(reporte.contains("Instrucciones completadas : 5"));
        assert!(reporte.contains("CPI (Ciclos / Instruccion): 2.00"));
        assert!(reporte.contains("IPC (Instrucciones / Ciclo): 0.50"));
        assert!(reporte.contains("Tiempo de ejecucion       : 100.00 ns"));
        assert!(reporte.contains("Hits                      : 3"));
        assert!(reporte.contains("Misses                    : 1"));
        assert!(reporte.contains("Tasa de aciertos          : 75.00%"));
    }
}
