use super::*;

fn cpu_vacia() -> CpuSegmentada {
    let burbuja = RegistroSegmentacion {
        instruccion: Instruccion::NOP,
        activa: false,
        resultado: None,
    };
    CpuSegmentada {
        if_id: burbuja,
        id_ex: burbuja,
        ex_mem: burbuja,
        mem_wb: burbuja,
        registros: [0; 4],
        program_counter: 0,
        contador_ciclos: 0,
    }
}

// ─── Tests de R0 Hardwired Zero ─────────────────────────────────────────────

/// ADD R0, R1, R2  ->  ADD R3, R0, R0
///
/// La primera instruccion intenta escribir en R0 (se descartara en WB).
/// La segunda lee R0 como fuente: aunque la primera este en EX/MEM con
/// resultado = 7, el forwarding debe ignorarla porque dest == R0.
/// R3 debe quedar en 0 (hardwired zero), no en 14 (7 + 7 fantasma).
#[test]
fn forwarding_no_anticipa_r0() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 3; // R1 = 3
    cpu.registros[2] = 4; // R2 = 4

    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R0,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R0,
            src2: Registro::R0,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    // Vaciar el pipeline (5 etapas -> 5 ciclos + un par mas para WB)
    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    // R0 debe seguir siendo 0 (hardwired zero)
    assert_eq!(cpu.registros[0], 0, "R0 debe ser siempre 0");
    // R3 debe ser 0 + 0 = 0, no 7 + 7 = 14
    assert_eq!(cpu.registros[3], 0, "R3 debe ser 0 (R0 + R0 hardwired)");
}

#[test]
fn test_r0_no_se_modifica_con_load_ni_sub() {
    let mut cpu = cpu_vacia();
    let mut mem = MemoriaProvisoria::new();
    mem.escribir_byte(0x10, 99);
    cpu.registros[1] = 50;

    let programa = vec![
        // Intentar cargar 99 en R0
        Instruccion::LOAD {
            dest: Registro::R0,
            direccion_ram: 0x10,
        },
        // Intentar restar y guardar en R0
        Instruccion::SUB {
            dest: Registro::R0,
            src1: Registro::R1,
            src2: Registro::R1,
        },
    ];

    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[0], 0, "R0 no debe cambiar");
}

// ─── Tests de Forwarding (prioridad EX/MEM vs MEM/WB) ───────────────────────

#[test]
fn test_forwarding_ex_mem_tiene_prioridad() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 5;
    cpu.registros[2] = 5;
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R0,
        },
    ];
    let mut mem = MemoriaProvisoria::new();
    for _ in 0..8 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }
    assert_eq!(cpu.registros[1], 15, "R1 final debe ser 15");
    assert_eq!(
        cpu.registros[3], 15,
        "R3 debe haber tomado el valor de EX/MEM (15), no el de MEM/WB (10)"
    );
}

// ─── Tests de Memoria (MemoriaProvisoria) ────────────────────────────────────

#[test]
fn test_memoria_lectura_escritura() {
    let mut mem = MemoriaProvisoria::new();
    assert_eq!(mem.leer_byte(0x00), 0);
    assert_eq!(mem.leer_byte(0xFF), 0);

    mem.escribir_byte(0x00, 42);
    mem.escribir_byte(0x42, 128);
    mem.escribir_byte(0xFF, 255);

    assert_eq!(mem.leer_byte(0x00), 42);
    assert_eq!(mem.leer_byte(0x42), 128);
    assert_eq!(mem.leer_byte(0xFF), 255);
}

#[test]
fn test_memoria_default() {
    let mem = MemoriaProvisoria::default();
    for b in mem.ram.iter() {
        assert_eq!(*b, 0);
    }
}

// ─── Tests de Operaciones Basicas (ADD / SUB / Overflow) ─────────────────────

#[test]
fn test_add_y_sub_sin_hazards() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 20;
    cpu.registros[2] = 5;

    // I1: ADD R3, R1, R2 (20 + 5 = 25)
    // I2..I4: NOP para evitar hazards
    // I5: SUB R1, R3, R2 (25 - 5 = 20)
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::NOP,
        Instruccion::NOP,
        Instruccion::NOP,
        Instruccion::SUB {
            dest: Registro::R1,
            src1: Registro::R3,
            src2: Registro::R2,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..10 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[3], 25);
    assert_eq!(cpu.registros[1], 20);
}

#[test]
fn test_overflow_wrapping() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 0;
    cpu.registros[2] = 1;
    cpu.registros[3] = 65535;

    let programa = vec![
        // 0 - 1 = 65535 (u16::MAX con wrapping)
        Instruccion::SUB {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        // 65535 + 1 = 0 (con wrapping)
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R3,
            src2: Registro::R2,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..8 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 65535);
    assert_eq!(cpu.registros[3], 0);
}

// ─── Tests de Forwarding (integracion via ciclo_reloj) ───────────────────────

#[test]
fn test_forwarding_ex_mem_a_ex() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 10;
    cpu.registros[2] = 20;

    // I1: ADD R3, R1, R2 (R3 = 30) -> en ciclo 4 esta en EX/MEM
    // I2: ADD R1, R3, R2 (R1 = 30 + 20 = 50) -> en ciclo 4 esta en ID/EX (lee R3 desde EX/MEM)
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R3,
            src2: Registro::R2,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[3], 30);
    assert_eq!(cpu.registros[1], 50);
}

#[test]
fn test_forwarding_mem_wb_a_ex() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 10;
    cpu.registros[2] = 20;

    // I1: ADD R3, R1, R2 (R3 = 30)
    // I2: NOP
    // I3: ADD R1, R3, R2 (R1 = 30 + 20 = 50) -> cuando I3 esta en EX, I1 esta en MEM/WB
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::NOP,
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R3,
            src2: Registro::R2,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..8 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[3], 30);
    assert_eq!(cpu.registros[1], 50);
}

#[test]
fn test_forwarding_ambos_operandos_src1_y_src2() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 10;
    cpu.registros[2] = 20;

    // I1: ADD R1, R1, R1 (R1 = 20) -> estara en MEM/WB
    // I2: ADD R2, R2, R2 (R2 = 40) -> estara en EX/MEM
    // I3: ADD R3, R1, R2 (R3 = 20 + 40 = 60) -> lee R1 de MEM/WB y R2 de EX/MEM
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R1,
        },
        Instruccion::ADD {
            dest: Registro::R2,
            src1: Registro::R2,
            src2: Registro::R2,
        },
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..8 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 20);
    assert_eq!(cpu.registros[2], 40);
    assert_eq!(cpu.registros[3], 60);
}

// ─── Tests de Memoria (LOAD / STORE y Hazards) ───────────────────────────────

#[test]
fn test_load_y_store_en_memoria() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 0xAA; // 170

    // I1: STORE R1 en direccion 0x50
    // I2..I4: NOP
    // I5: LOAD R2 desde direccion 0x50
    let programa = vec![
        Instruccion::STORE {
            src: Registro::R1,
            direccion_ram: 0x50,
        },
        Instruccion::NOP,
        Instruccion::NOP,
        Instruccion::NOP,
        Instruccion::LOAD {
            dest: Registro::R2,
            direccion_ram: 0x50,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..10 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(mem.leer_byte(0x50), 0xAA);
    assert_eq!(cpu.registros[2], 0xAA);
}

#[test]
fn test_load_use_hazard_con_stall() {
    let mut cpu = cpu_vacia();
    let mut mem = MemoriaProvisoria::new();
    mem.escribir_byte(0x20, 15);
    cpu.registros[2] = 5;

    // I1: LOAD R1, 0x20  (R1 = 15)
    // I2: ADD R3, R1, R2 (R3 = 15 + 5 = 20) -> Load-Use Hazard! Requiere 1 ciclo de stall
    let programa = vec![
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x20,
        },
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
    ];

    // Se necesitan 7 ciclos para ejecutar completamente con 1 burbuja
    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 15, "LOAD debe haber escrito 15 en R1");
    assert_eq!(cpu.registros[3], 20, "ADD debe haber sumado 15 + 5 = 20");
}

#[test]
fn test_load_use_hazard_con_store() {
    let mut cpu = cpu_vacia();
    let mut mem = MemoriaProvisoria::new();
    mem.escribir_byte(0x10, 77);

    // I1: LOAD R1, 0x10
    // I2: STORE R1, 0x20 (dependencia en STORE src -> Load-Use Stall)
    let programa = vec![
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        Instruccion::STORE {
            src: Registro::R1,
            direccion_ram: 0x20,
        },
    ];

    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 77);
    assert_eq!(mem.leer_byte(0x20), 77);
}

#[test]
fn test_load_use_hazard_con_sub() {
    let mut cpu = cpu_vacia();
    let mut mem = MemoriaProvisoria::new();
    mem.escribir_byte(0x30, 30);
    cpu.registros[2] = 10;

    // I1: LOAD R1, 0x30 (R1 = 30)
    // I2: SUB R3, R1, R2 (R3 = 30 - 10 = 20) -> Load-Use Stall
    let programa = vec![
        Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x30,
        },
        Instruccion::SUB {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
    ];

    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 30, "LOAD debe haber escrito 30 en R1");
    assert_eq!(
        cpu.registros[3], 20,
        "SUB debe haber calculado 30 - 10 = 20"
    );
}

#[test]
fn test_store_con_forwarding_desde_ex() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 10;
    cpu.registros[2] = 15;

    // I1: ADD R3, R1, R2 (R3 = 25)
    // I2: STORE R3, 0x30 -> Forwarding directo de EX/MEM a STORE
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R3,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        Instruccion::STORE {
            src: Registro::R3,
            direccion_ram: 0x30,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..7 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[3], 25);
    assert_eq!(mem.leer_byte(0x30), 25);
}

#[test]
fn test_store_sin_hazard_usa_banco_de_registros() {
    let mut cpu = cpu_vacia();
    // R1 ya esta en el banco, no hay instrucciones previas que produzcan R1
    cpu.registros[1] = 0xBB;

    let programa = vec![Instruccion::STORE {
        src: Registro::R1,
        direccion_ram: 0x60,
    }];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..5 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(mem.leer_byte(0x60), 0xBB);
}

// ─── Tests de Control Hazards (JUMP y Branch Flush) ──────────────────────────

#[test]
fn test_jump_con_flush_de_instrucciones_especulativas() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 10;

    // 0: JUMP 3 (salta a la instruccion 3)
    // 1: ADD R1, R1, R1 (debe ser flusheada, R1 no debe duplicarse)
    // 2: ADD R1, R1, R1 (debe ser flusheada, R1 no debe duplicarse)
    // 3: ADD R2, R1, R0 (R2 = 10 + 0 = 10)
    let programa = vec![
        Instruccion::JUMP {
            direccion_destino: 3,
        },
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R1,
        },
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R1,
            src2: Registro::R1,
        },
        Instruccion::ADD {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R0,
        },
    ];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..9 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(
        cpu.registros[1], 10,
        "Instrucciones intermedias debieron descartarse"
    );
    assert_eq!(cpu.registros[2], 10, "R2 debe recibir R1 tras el JUMP");
}

#[test]
fn test_jump_a_inicio_del_programa() {
    let mut cpu = cpu_vacia();

    // Pipeline trace (programa de 2 instrucciones):
    // Ciclo 1: IF/ID ← ADD[0]  (PC=1)
    // Ciclo 2: ID/EX ← ADD[0], IF/ID ← JUMP[1] (PC=2)
    // Ciclo 3: JUMP detectado en ID/EX ->
    //            EX/MEM ← ejecutar_alu(JUMP) = burbuja (resultado=None)
    //            ID/EX  ← burbuja (flush)
    //            IF/ID  ← burbuja (flush)
    //            PC     ← 0
    //          ADD[0] se pierde: fue flusheado antes de terminar EX.
    // -> El ADD NO llega a WB, R1 permanece en 0.
    //
    // Verificamos: el PC se resetea a 0 y el ADD especulativo fue descartado.
    let programa = vec![
        Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R2, // R2 = 99, R3 = 0 -> resultado seria 99
            src2: Registro::R3,
        },
        Instruccion::JUMP {
            direccion_destino: 0,
        },
    ];
    cpu.registros[2] = 99; // Si ADD llega a WB, R1 seria 99

    let mut mem = MemoriaProvisoria::new();

    // 4 ciclos: el JUMP se resuelve y flushea el ADD especulativo
    for _ in 0..4 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    // ADD fue flusheado -> R1 NO debe haberse modificado
    assert_eq!(cpu.registros[1], 0, "ADD flusheado no debe escribir en R1");
    // El PC debe apuntar al inicio del programa tras el JUMP
    assert_eq!(cpu.program_counter, 0, "PC debe resetearse a 0 tras JUMP");
}

// ─── Tests de pipeline vacio / NOP-only ──────────────────────────────────────

#[test]
fn test_pipeline_nop_no_modifica_registros() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 100;
    cpu.registros[2] = 200;
    cpu.registros[3] = 300;

    let programa = vec![Instruccion::NOP, Instruccion::NOP, Instruccion::NOP];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..8 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[0], 0);
    assert_eq!(cpu.registros[1], 100);
    assert_eq!(cpu.registros[2], 200);
    assert_eq!(cpu.registros[3], 300);
}

#[test]
fn test_programa_vacio_no_modifica_nada() {
    let mut cpu = cpu_vacia();
    cpu.registros[1] = 42;
    let programa: Vec<Instruccion> = vec![];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..5 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    assert_eq!(cpu.registros[1], 42);
}

// ─── Tests del contador de ciclos ────────────────────────────────────────────

#[test]
fn test_contador_ciclos_avanza() {
    let mut cpu = cpu_vacia();
    let programa = vec![Instruccion::NOP];
    let mut mem = MemoriaProvisoria::new();

    assert_eq!(cpu.contador_ciclos, 0);
    for i in 1..=10 {
        cpu.ciclo_reloj(&programa, &mut mem);
        assert_eq!(cpu.contador_ciclos, i);
    }
}

// ─── Tests unitarios de calcular_forwarding ──────────────────────────────────

#[test]
fn test_forwarding_sin_pipeline_activo_devuelve_none() {
    let cpu = cpu_vacia();
    // Ninguna etapa activa -> debe leer del banco de registros (None)
    assert_eq!(cpu.calcular_forwarding(Registro::R1), None);
    assert_eq!(cpu.calcular_forwarding(Registro::R2), None);
    assert_eq!(cpu.calcular_forwarding(Registro::R3), None);
}

#[test]
fn test_forwarding_r0_siempre_devuelve_none() {
    let mut cpu = cpu_vacia();
    // Aunque EX/MEM tenga dest=R0 con resultado, forwarding de R0 siempre es None
    cpu.ex_mem = RegistroSegmentacion {
        instruccion: Instruccion::ADD {
            dest: Registro::R0,
            src1: Registro::R1,
            src2: Registro::R2,
        },
        activa: true,
        resultado: Some(999),
    };
    assert_eq!(cpu.calcular_forwarding(Registro::R0), None);
}

#[test]
fn test_forwarding_ex_mem_activo_devuelve_resultado() {
    let mut cpu = cpu_vacia();
    cpu.ex_mem = RegistroSegmentacion {
        instruccion: Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R2,
            src2: Registro::R3,
        },
        activa: true,
        resultado: Some(42),
    };
    assert_eq!(cpu.calcular_forwarding(Registro::R1), Some(42));
    // R2 y R3 no son dest, no hay forwarding
    assert_eq!(cpu.calcular_forwarding(Registro::R2), None);
    assert_eq!(cpu.calcular_forwarding(Registro::R3), None);
}

#[test]
fn test_forwarding_mem_wb_activo_devuelve_resultado() {
    let mut cpu = cpu_vacia();
    // EX/MEM inactivo -> debe caer a MEM/WB
    cpu.mem_wb = RegistroSegmentacion {
        instruccion: Instruccion::SUB {
            dest: Registro::R2,
            src1: Registro::R1,
            src2: Registro::R3,
        },
        activa: true,
        resultado: Some(77),
    };
    assert_eq!(cpu.calcular_forwarding(Registro::R2), Some(77));
    assert_eq!(cpu.calcular_forwarding(Registro::R1), None);
}

#[test]
fn test_forwarding_ex_mem_sin_resultado_no_cae_a_mem_wb() {
    // LOAD en EX/MEM tiene resultado=None (dato aun no leido).
    // No debe transparentar MEM/WB aunque MEM/WB tambien produzca el mismo dest.
    let mut cpu = cpu_vacia();
    cpu.ex_mem = RegistroSegmentacion {
        instruccion: Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x00,
        },
        activa: true,
        resultado: None, // todavia no tiene dato
    };
    cpu.mem_wb = RegistroSegmentacion {
        instruccion: Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R2,
            src2: Registro::R3,
        },
        activa: true,
        resultado: Some(55),
    };
    // EX/MEM matchea (dest==R1) y devuelve None -> no cae a MEM/WB
    assert_eq!(cpu.calcular_forwarding(Registro::R1), None);
}

// ─── Tests unitarios de detectar_load_use_hazard ─────────────────────────────

#[test]
fn test_hazard_load_seguido_de_add() {
    let mut cpu = cpu_vacia();
    cpu.id_ex = RegistroSegmentacion {
        instruccion: Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        activa: true,
        resultado: None,
    };
    let siguiente = Instruccion::ADD {
        dest: Registro::R3,
        src1: Registro::R1, // dependencia
        src2: Registro::R2,
    };
    assert!(cpu.detectar_load_use_hazard(&siguiente));
}

#[test]
fn test_hazard_load_seguido_de_sub() {
    let mut cpu = cpu_vacia();
    cpu.id_ex = RegistroSegmentacion {
        instruccion: Instruccion::LOAD {
            dest: Registro::R2,
            direccion_ram: 0x20,
        },
        activa: true,
        resultado: None,
    };
    let siguiente = Instruccion::SUB {
        dest: Registro::R3,
        src1: Registro::R1,
        src2: Registro::R2, // dependencia en src2
    };
    assert!(cpu.detectar_load_use_hazard(&siguiente));
}

#[test]
fn test_hazard_load_seguido_de_load_no_detecta_stall() {
    // LOAD no lee registros fuente -> no hay hazard
    let mut cpu = cpu_vacia();
    cpu.id_ex = RegistroSegmentacion {
        instruccion: Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        activa: true,
        resultado: None,
    };
    let siguiente = Instruccion::LOAD {
        dest: Registro::R3,
        direccion_ram: 0x20,
    };
    assert!(!cpu.detectar_load_use_hazard(&siguiente));
}

#[test]
fn test_hazard_load_seguido_de_jump_no_detecta_stall() {
    let mut cpu = cpu_vacia();
    cpu.id_ex = RegistroSegmentacion {
        instruccion: Instruccion::LOAD {
            dest: Registro::R1,
            direccion_ram: 0x10,
        },
        activa: true,
        resultado: None,
    };
    let siguiente = Instruccion::JUMP {
        direccion_destino: 5,
    };
    assert!(!cpu.detectar_load_use_hazard(&siguiente));
}

#[test]
fn test_no_hazard_si_id_ex_inactivo() {
    let cpu = cpu_vacia(); // id_ex inactivo
    let siguiente = Instruccion::ADD {
        dest: Registro::R3,
        src1: Registro::R1,
        src2: Registro::R2,
    };
    assert!(!cpu.detectar_load_use_hazard(&siguiente));
}

#[test]
fn test_no_hazard_add_en_id_ex() {
    // Solo LOAD en id_ex genera hazard, un ADD no
    let mut cpu = cpu_vacia();
    cpu.id_ex = RegistroSegmentacion {
        instruccion: Instruccion::ADD {
            dest: Registro::R1,
            src1: Registro::R2,
            src2: Registro::R3,
        },
        activa: true,
        resultado: Some(10),
    };
    let siguiente = Instruccion::ADD {
        dest: Registro::R3,
        src1: Registro::R1,
        src2: Registro::R2,
    };
    assert!(!cpu.detectar_load_use_hazard(&siguiente));
}

// ─── Tests de Formateo / Display ─────────────────────────────────────────────

#[test]
fn test_display_formato() {
    let inst_add = Instruccion::ADD {
        dest: Registro::R1,
        src1: Registro::R2,
        src2: Registro::R3,
    };
    assert_eq!(format!("{}", inst_add), "ADD R1,R2,R3");

    let inst_sub = Instruccion::SUB {
        dest: Registro::R0,
        src1: Registro::R1,
        src2: Registro::R2,
    };
    assert_eq!(format!("{}", inst_sub), "SUB R0,R1,R2");

    let inst_load = Instruccion::LOAD {
        dest: Registro::R1,
        direccion_ram: 0x2A,
    };
    assert_eq!(format!("{}", inst_load), "LOAD R1,0x2A");

    let inst_store = Instruccion::STORE {
        src: Registro::R2,
        direccion_ram: 0x1F,
    };
    assert_eq!(format!("{}", inst_store), "STORE R2,0x1F");

    let inst_jump = Instruccion::JUMP {
        direccion_destino: 0x05,
    };
    assert_eq!(format!("{}", inst_jump), "JUMP 0x05");

    let inst_nop = Instruccion::NOP;
    assert_eq!(format!("{}", inst_nop), "NOP");
}

#[test]
fn test_display_registro_activo_muestra_instruccion() {
    let inst = Instruccion::ADD {
        dest: Registro::R1,
        src1: Registro::R2,
        src2: Registro::R3,
    };

    let reg_activo = RegistroSegmentacion {
        instruccion: inst,
        activa: true,
        resultado: None,
    };
    // Activo -> muestra la instruccion, no "--"
    assert_eq!(format!("{}", reg_activo), "ADD R1,R2,R3");

    let reg_inactivo = RegistroSegmentacion {
        instruccion: inst,
        activa: false,
        resultado: None,
    };
    // Inactivo -> siempre "--"
    assert_eq!(format!("{}", reg_inactivo), "--");
}

#[test]
fn test_display_cpu_muestra_ciclo_0_inicial() {
    let cpu = cpu_vacia();
    let s = format!("{}", cpu);
    assert!(s.contains("Ciclo 0"));
}

#[test]
fn test_display_cpu_muestra_contador_actualizado() {
    let mut cpu = cpu_vacia();
    let programa = vec![Instruccion::NOP];
    let mut mem = MemoriaProvisoria::new();

    for _ in 0..3 {
        cpu.ciclo_reloj(&programa, &mut mem);
    }

    let s = format!("{}", cpu);
    assert!(
        s.contains("Ciclo 3"),
        "Display debe mostrar el ciclo actual"
    );
}
