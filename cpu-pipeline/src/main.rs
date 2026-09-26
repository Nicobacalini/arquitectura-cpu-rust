/// Binario de prueba local para cpu-pipeline.
/// Toda la logica real vive en lib.rs y es accesible como `cpu_pipeline::*`.
/// Inicializa una CPU con registros de ejemplo, ejecuta un programa de dos
/// instrucciones y muestra el estado del pipeline ciclo a ciclo.
use cache_controller::ControladorMemoria;
use cpu_pipeline::{CpuSegmentada, Instruccion, Registro};

fn main() {
    // Estado inicial de la CPU con R1 en 10 y R2 en 20
    let mut cpu = CpuSegmentada {
        registros: [0, 10, 20, 0],
        ..CpuSegmentada::nueva()
    };

    // Programa de prueba con ADD y SUB dependiente resuelto por forwarding
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::SUB {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R1,
        },
    ];

    let mut memoria = ControladorMemoria::nuevo();

    println!("=== Demo pipeline cpu-pipeline ===\n");
    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut memoria);
        print!("{}", cpu);
    }

    println!("\nRegistros finales: {:?}", cpu.registros);
}
