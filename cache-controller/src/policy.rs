use crate::storage::{BLOQUE_BYTES, ControladorMemoria};

impl ControladorMemoria {
    /// Elige la via victima dentro del conjunto dado para realizar un reemplazo
    ///
    /// La politica de reemplazo es LRU:
    /// 1. Si la via 0 esta invalida, se la elige directamente (espacio libre)
    /// 2. Si la via 1 esta invalida, se la elige directamente
    /// 3. Si ambas son validas, se desaloja la que tiene el `ultimo_acceso` mas antiguo (menor valor)
    ///    Desempate: si son iguales, gana la via 1 por convencion
    pub fn elegir_via_victima(&self, indice_conjunto: usize) -> usize {
        let via0 = &self.cache[indice_conjunto].vias[0];
        let via1 = &self.cache[indice_conjunto].vias[1];

        // Via 0 libre no hay dato valido, se usa sin desalojar nada
        if !via0.valido {
            return 0;
        }
        // Via 1 libre idem para la via 1
        if !via1.valido {
            return 1;
        }

        // Ambas vias son validas: se aplica LRU
        if via0.ultimo_acceso < via1.ultimo_acceso {
            return 0; // la via 0 fue usada hace mas tiempo
        }
        1
    }

    /// Maneja un cache miss: elige victima (LRU), la desaloja escribiendo a RAM
    /// si estaba dirty (Write-Back), trae el nuevo bloque desde RAM, y actualiza la linea
    /// Devuelve el indice de via donde quedo cargado el bloque nuevo
    pub fn manejar_miss(&mut self, indice_conjunto: usize, tag: u8) -> usize {
        // Elegir la via victima con LRU
        let via_victima = self.elegir_via_victima(indice_conjunto);

        // Write-back: si la via era valida y sucia, volcar sus datos a RAM antes de sobreescribirla
        // Se usa el TAG VIEJO de la via victima para reconstruir la direccion correcta
        {
            let linea = &self.cache[indice_conjunto].vias[via_victima];
            if linea.valido && linea.dirty_bit {
                let dir_base = self.reconstruir_direccion_base(linea.tag, indice_conjunto) as usize;
                let datos = linea.datos; // copiar para evitar conflicto de borrow
                self.ram[dir_base..dir_base + BLOQUE_BYTES].copy_from_slice(&datos);
                self.estadisticas.desalojos_dirty += 1;
            }
        }

        // Calcular la direccion base del bloque solicitado y traerlo desde RAM
        let dir_base = self.reconstruir_direccion_base(tag, indice_conjunto) as usize;
        let mut nuevos_datos = [0u8; BLOQUE_BYTES];
        nuevos_datos.copy_from_slice(&self.ram[dir_base..dir_base + BLOQUE_BYTES]);

        // Escribir la nueva linea en la via victima
        let linea = &mut self.cache[indice_conjunto].vias[via_victima];
        linea.tag = tag;
        linea.valido = true;
        linea.dirty_bit = false;
        linea.datos = nuevos_datos;
        linea.ultimo_acceso = self.contador_ciclos;

        via_victima
    }
}
