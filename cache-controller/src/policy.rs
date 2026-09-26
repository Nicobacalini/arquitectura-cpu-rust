use crate::storage::{BLOQUE_BYTES, ControladorMemoria};

impl ControladorMemoria {
    /// Elige la via victima dentro del conjunto dado para realizar un reemplazo.
    /// La politica es LRU: si alguna via esta libre se selecciona directamente
    /// con prioridad para la via cero, y cuando ambas son validas se desaloja
    /// la de acceso mas antiguo desempatando por la via uno.
    pub fn elegir_via_victima(&self, indice_conjunto: usize) -> usize {
        let via0 = &self.cache[indice_conjunto].vias[0];
        let via1 = &self.cache[indice_conjunto].vias[1];

        if !via0.valido {
            return 0;
        }
        if !via1.valido {
            return 1;
        }

        if via0.ultimo_acceso < via1.ultimo_acceso {
            return 0;
        }
        1
    }

    /// Maneja un fallo de cache seleccionando una victima mediante LRU, volcando sus
    /// datos a RAM si estaba modificada y cargando el bloque nuevo en la linea.
    pub fn manejar_miss(&mut self, indice_conjunto: usize, tag: u16) -> usize {
        let via_victima = self.elegir_via_victima(indice_conjunto);

        // Volcado a RAM si la linea victima es valida y contiene datos modificados
        {
            let linea = &self.cache[indice_conjunto].vias[via_victima];
            if linea.valido && linea.dirty_bit {
                let dir_base = self.reconstruir_direccion_base(linea.tag, indice_conjunto) as usize;
                let datos = linea.datos;
                self.ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                self.estadisticas.desalojos_dirty += 1;
            }
        }

        let dir_base = self.reconstruir_direccion_base(tag, indice_conjunto) as usize;
        let mut nuevos_datos = [0u8; BLOQUE_BYTES];
        nuevos_datos.copy_from_slice(&self.ram[dir_base..dir_base + BLOQUE_BYTES]);

        let linea = &mut self.cache[indice_conjunto].vias[via_victima];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = false;
        linea.datos = nuevos_datos;
        linea.ultimo_acceso = self.contador_ciclos;

        via_victima
    }
}
