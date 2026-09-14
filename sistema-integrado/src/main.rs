mod display;
mod ejemplos;

use cache_controller::ControladorMemoria;
use cpu_pipeline::CpuSegmentada;
use display::reporte_rendimiento;
use ejemplos::Ejemplo;

// ─── Runner de un ejemplo ─────────────────────────────────────────────────────

fn ejecutar_ejemplo(numero: usize, ej: &Ejemplo) {
    let sep = "=".repeat(60);
    println!("{sep}");
    println!("  Ejemplo {numero}: {}", ej.nombre);
    println!("  {}", ej.descripcion);
    println!("{sep}\n");

    // Inicializar CPU con los registros del ejemplo.
    // `..CpuSegmentada::nueva()` rellena el resto del estado con valores de stock:
    // buffers del pipeline en NOP inactivo, PC en 0, contadores en 0.
    let mut cpu = CpuSegmentada {
        registros: ej.registros_iniciales,
        ..CpuSegmentada::nueva()
    };

    // Inicializar memoria y precargar valores RAM del ejemplo
    let mut memoria = ControladorMemoria::nuevo();
    for &(addr, val) in ej.ram_inicial {
        memoria.ram[addr as usize] = val;
    }

    let programa = (ej.programa)();

    println!("Programa a ejecutar:");
    for (i, inst) in programa.iter().enumerate() {
        println!("  [{}] {}", i, inst);
    }
    println!("\nEjecucion ciclo a ciclo del pipeline:\n");

    // Ejecutar hasta que el programa termine y el pipeline se vacíe por completo
    while cpu.program_counter < programa.len()
        || cpu.if_id.activa
        || cpu.id_ex.activa
        || cpu.ex_mem.activa
        || cpu.mem_wb.activa
    {
        cpu.ciclo_reloj(&programa, &mut memoria);
        print!("{}", cpu);
    }

    // Sincronizar caché con RAM al finalizar (write-back de líneas sucias)
    memoria.flush();

    println!("{}", reporte_rendimiento(&cpu, &memoria, 100.0));
}

// ─── Función principal ────────────────────────────────────────────────────────
fn main() {
    let lista = ejemplos::catalogo();

    println!("{}", "=".repeat(60));
    println!("     Simulador Integrado: CPU Segmentada + Memoria Cache    ");
    println!("{}", "=".repeat(60));
    println!();
    println!("Ejemplos disponibles ({} en total):", lista.len());
    for (i, ej) in lista.iter().enumerate() {
        println!("  [{}] {}", i + 1, ej.nombre);
    }
    println!();

    // Ejecutar todos los ejemplos en secuencia
    for (i, ej) in lista.iter().enumerate() {
        ejecutar_ejemplo(i + 1, ej);
    }
}
