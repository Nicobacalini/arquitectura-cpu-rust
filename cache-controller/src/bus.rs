use crate::storage::{BLOQUE_BYTES, CANTIDAD_CONJUNTOS, ControladorMemoria};

impl ControladorMemoria {
    /// Lee un byte de la direccion dada. Ante un acierto actualiza el acceso y
    /// devuelve el dato, mientras que ante un fallo carga previamente el bloque.
    pub fn leer_byte(&mut self, direccion: u16) -> u8 {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        if let Some(via_idx) = self.buscar_via_hit(indice, tag) {
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.estadisticas.hits += 1;
            self.cache[indice].vias[via_idx].datos[offset]
        } else {
            self.estadisticas.misses += 1;
            let via_idx = self.manejar_miss(indice, tag);
            self.cache[indice].vias[via_idx].ultimo_acceso = self.contador_ciclos;
            self.cache[indice].vias[via_idx].datos[offset]
        }
    }

    /// Escribe un byte en la direccion dada aplicando Write-Back y Write-Allocate.
    /// No escribe directamente en RAM, ya que el volcado ocurre al desalojar o al llamar a flush.
    pub fn escribir_byte(&mut self, direccion: u16, dato: u8) {
        self.contador_ciclos += 1;
        let (tag, indice, offset) = self.decodificar_direccion(direccion);

        let via_idx = if let Some(via_idx) = self.buscar_via_hit(indice, tag) {
            self.estadisticas.hits += 1;
            via_idx
        } else {
            self.estadisticas.misses += 1;
            self.manejar_miss(indice, tag)
        };

        let linea = &mut self.cache[indice].vias[via_idx];
        linea.datos[offset] = dato;
        linea.dirty_bit = true;
        linea.ultimo_acceso = self.contador_ciclos;
    }

    /// Sincroniza todas las lineas dirty hacia la RAM sin invalidarlas.
    /// Garantiza persistencia y deja el dirty_bit en false manteniendo validas las lineas.
    pub fn flush(&mut self) {
        for conjunto_idx in 0..CANTIDAD_CONJUNTOS {
            for via_idx in 0..2 {
                let linea = &self.cache[conjunto_idx].vias[via_idx];
                if linea.valido && linea.dirty_bit {
                    let dir_base =
                        self.reconstruir_direccion_base(linea.tag, conjunto_idx) as usize;
                    let datos = linea.datos;
                    self.ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                    self.cache[conjunto_idx].vias[via_idx].dirty_bit = false;
                }
            }
        }
    }
}
