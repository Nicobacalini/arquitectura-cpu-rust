mod display;

use cache_controller::ControladorMemoria;
use cpu_pipeline::{CpuSegmentada, Instruccion, Registro};
use display::reporte_rendimiento;

fn main() {
    println!("============================================================");
    println!("     Simulador Integrado: CPU Segmentada + Memoria Cache    ");
    println!("============================================================\n");

    // Inicialización de la CPU utilizando la sintaxis de actualización de Rust (..):
    // Solo especificamos los campos que deseamos personalizar (en este caso, precargar R2 = 10),
    // mientras que `..CpuSegmentada::nueva()` rellena automáticamente todo el estado de stock
    // (los 4 buffers del pipeline como burbujas NOP inactivas, PC en 0 y contadores en 0).
    // Ventaja: si en el futuro se agregan más campos a CpuSegmentada, este código nunca se romperá.
    let mut cpu = CpuSegmentada {
        registros: [0, 0, 10, 0], // R0=0, R1=0, R2=10, R3=0
        ..CpuSegmentada::nueva()
    };

    let mut memoria = ControladorMemoria::nuevo();

    // Precargar un valor en la memoria principal RAM
    // Direccion 0x10: contiene el valor 15
    memoria.ram[0x10] = 15;

    // Programa de prueba con:
    // 1. Load-Use Hazard: I0 carga R1, e inmediatamente I1 usa R1 como fuente.
    // 2. STORE: I2 guarda el resultado de R2 en la direccion 0x20.
    // 3. Re-lectura para evidenciar Hit en cache: I3 carga desde 0x20.
    // 4. Operacion aritmetica final: I4 resta R1 a R3.
    let programa = vec![
        // I0: Cargar RAM[0x10] (15) en R1
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        // I1: R2 = R1 + R2 -> Genera Load-Use Hazard (stall de 1 ciclo)
        Instruccion::ADD {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // I2: Guardar R2 en RAM[0x20] -> Instruccion STORE
        Instruccion::STORE {
            src: Registro::R2,
            direccion_ram: 0x20,
        },
        // I3: Cargar RAM[0x20] en R3 -> Cache Hit esperado
        Instruccion::LOAD {
            dest: Registro::R3,
            direccion_ram: 0x20,
        },
        // I4: R3 = R3 - R1 -> Genera Load-Use Hazard adicional
        Instruccion::SUB {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R1,
        },
    ];

    println!("Programa a ejecutar:");
    for (i, inst) in programa.iter().enumerate() {
        println!("  [{}] {}", i, inst);
    }
    println!("\nEjecucion ciclo a ciclo del pipeline:\n");

    // Ejecutar hasta que el programa termine y el pipeline se vacie por completo
    while cpu.program_counter < programa.len()
        || cpu.if_id.activa
        || cpu.id_ex.activa
        || cpu.ex_mem.activa
        || cpu.mem_wb.activa
    {
        cpu.ciclo_reloj(&programa, &mut memoria);
        print!("{}", cpu);
    }

    // Sincronizar cache con RAM al finalizar
    memoria.flush();

    println!("{}", reporte_rendimiento(&cpu, &memoria, 100.0));
}
