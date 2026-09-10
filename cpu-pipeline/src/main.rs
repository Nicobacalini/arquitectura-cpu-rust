/// Binario de prueba local para cpu-pipeline.
/// Toda la logica real vive en lib.rs y es accesible como `cpu_pipeline::*`.
/// Inicializa una CPU con registros de ejemplo, ejecuta un programa de dos
/// instrucciones y muestra el estado del pipeline ciclo a ciclo.
use cpu_pipeline::{
    CpuSegmentada, Instruccion, MemoriaProvisoria, Registro, RegistroSegmentacion,
};

fn main() {
    let burbuja = RegistroSegmentacion {
        instruccion: Instruccion::NOP,
        activa: false,
        resultado: None,
    };

    // Estado inicial de la CPU: R1=10, R2=20, todos los demas en 0.
    let mut cpu = CpuSegmentada {
        if_id: burbuja,
        id_ex: burbuja,
        ex_mem: burbuja,
        mem_wb: burbuja,
        registros: [0, 10, 20, 0],
        program_counter: 0,
        contador_ciclos: 0,
    };

    // Programa de ejemplo:
    // I1: ADD R3, R1, R2  -> R3 = 10 + 20 = 30
    // I2: SUB R3, R3, R1  -> R3 = 30 - 10 = 20 (con forwarding de EX/MEM)
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

    let mut memoria = MemoriaProvisoria::new();

    println!("=== Demo pipeline cpu-pipeline ===\n");
    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut memoria);
        print!("{}", cpu);
    }

    println!("\nRegistros finales: {:?}", cpu.registros);
}
