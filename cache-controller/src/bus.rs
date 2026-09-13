use crate::storage::{BLOQUE_BYTES, CANTIDAD_CONJUNTOS, ControladorMemoria};

impl ControladorMemoria {
    /// Lee un byte de la direccion dada
    ///
    /// - Hit: actualiza ultimo_acceso, incrementa hits, devuelve datos[offset]
    /// - Miss: incrementa misses, carga el bloque (manejar_miss), luego devuelve datos[offset]
    pub fn leer_byte(&mut self, direccion: u8) -> u8 {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        if let Some(via_idx) = self.buscar_via_hit(indice, tag) {
            // HIT
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.estadisticas.hits += 1;
            self.cache[indice].vias[via_idx].datos[offset]
        } else {
            // MISS
            self.estadisticas.misses += 1;
            let via_idx = self.manejar_miss(indice, tag);
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.cache[indice].vias[via_idx].datos[offset]
        }
    }

    /// Escribe un byte en la direccion dada (Write-Back / Write-Allocate)
    ///
    /// - Hit: actualiza el byte en cache, marca dirty_bit = true, actualiza ultimo_acceso
    /// - Miss: carga primero el bloque completo (Write-Allocate), luego escribe el byte
    ///   y marca dirty_bit = true
    ///
    /// Nunca escribe directamente en self.ram — el volcado ocurre al desalojar en manejar_miss
    /// o al llamar a flush()
    pub fn escribir_byte(&mut self, direccion: u8, dato: u8) {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        let via_idx = if let Some(via_idx) = self.buscar_via_hit(indice, tag) {
            // HIT
            self.estadisticas.hits += 1;
            via_idx
        } else {
            // MISS: traer bloque antes de modificarlo (Write-Allocate)
            self.estadisticas.misses += 1;
            self.manejar_miss(indice, tag)
        };

        let linea = &mut self.cache[indice].vias[via_idx];
        linea.datos[offset] = dato;
        linea.dirty_bit = true;
        linea.ultimo_acceso = self.contador_ciclos;
    }

    /// Sincroniza todas las lineas dirty hacia la RAM sin invalidarlas.
    ///
    /// Despues de flush():
    /// - Cada linea que tenia dirty_bit == true tiene sus datos volcados a la RAM.
    /// - La linea sigue valida en cache
    /// - dirty_bit se pone en false (la cache y la RAM estan sincronizadas)
    ///
    /// Equivalente a un fsync de un sistema de archivos: garantiza durabilidad
    /// sin tirar los datos cacheados
    pub fn flush(&mut self) {
        for conjunto_idx in 0..CANTIDAD_CONJUNTOS {
            for via_idx in 0..2 {
                let linea = &self.cache[conjunto_idx].vias[via_idx];
                if linea.valido && linea.dirty_bit {
                    let dir_base =
                        self.reconstruir_direccion_base(linea.tag, conjunto_idx) as usize;
                    let datos = linea.datos; // copiar para evitar conflicto de borrow
                    self.ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                    // Limpiar el dirty_bit: cache y RAM quedan sincronizadas
                    self.cache[conjunto_idx].vias[via_idx].dirty_bit = false;
                }
            }
        }
    }
}
