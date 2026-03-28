#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

pub struct Cartridge {
    pub prg_rom: Vec<u8>,  // código — mapeado em 0x8000..=0xFFFF
    pub chr_rom: Vec<u8>,  // gráficos — usado pela PPU
    pub mapper: u8,
    pub mirroring: Mirroring,
}

impl Cartridge {
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        // Header iNES tem 16 bytes mínimos
        if data.len() < 16 {
            return Err("ROM inválida: menor que 16 bytes".to_string());
        }

        // Magic number: "NES\x1A"
        if &data[0..4] != b"NES\x1a" {
            return Err("Magic number inválido — não é uma ROM iNES válida".to_string());
        }

        let prg_size = data[4] as usize * 16384; // byte 4 * 16 KB
        let chr_size = data[5] as usize * 8192;  // byte 5 * 8 KB
        let flags6 = data[6];
        let flags7 = data[7];

        // Mapper = nibble alto do byte 7 | nibble alto do byte 6
        let mapper = (flags7 & 0xF0) | (flags6 >> 4);
        if mapper != 0 {
            return Err(format!(
                "Mapper {} não suportado — apenas Mapper 0 (NROM) está implementado",
                mapper
            ));
        }

        // Mirroring: bit 3 de flags6 = four-screen; bit 0 = vertical; senão horizontal
        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        // Trainer opcional: bit 2 de flags6, ocupa 512 bytes após o header
        let trainer_offset = if flags6 & 0x04 != 0 { 512 } else { 0 };
        let prg_start = 16 + trainer_offset;
        let chr_start = prg_start + prg_size;

        if data.len() < chr_start + chr_size {
            return Err(format!(
                "ROM truncada: esperava {} bytes, encontrou {}",
                chr_start + chr_size,
                data.len()
            ));
        }

        let prg_rom = data[prg_start..prg_start + prg_size].to_vec();
        let chr_rom = data[chr_start..chr_start + chr_size].to_vec();

        Ok(Cartridge { prg_rom, chr_rom, mapper, mirroring })
    }

    /// Lê da PRG-ROM. `addr` já vem relativo a 0x8000 (i.e. 0x0000..=0x7FFF).
    /// NROM-128 (16 KB): espelha — addr & 0x3FFF
    /// NROM-256 (32 KB): addr & 0x7FFF
    pub fn read_prg(&self, addr: u16) -> u8 {
        if self.prg_rom.is_empty() {
            return 0;
        }
        // Usa módulo para lidar corretamente com 16 KB e 32 KB
        self.prg_rom[addr as usize % self.prg_rom.len()]
    }
}
