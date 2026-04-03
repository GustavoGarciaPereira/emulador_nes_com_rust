#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLow,
    SingleScreenHigh,
}

pub struct Cartridge {
    pub prg_rom: Vec<u8>,         // código — mapeado em 0x8000..=0xFFFF
    pub chr_rom: Vec<u8>,         // gráficos — usado pela PPU
    pub chr_ram: Vec<u8>,         // 8KB — usado quando chr_rom está vazio
    pub mapper: u8,
    pub mirroring: Mirroring,
    // MMC1 state
    pub mmc1_shift: u8,           // shift register (5 bits)
    pub mmc1_shift_count: u8,     // quantos bits já foram escritos (0-4)
    pub mmc1_control: u8,         // registrador de controle
    pub mmc1_chr0: u8,            // CHR bank 0
    pub mmc1_chr1: u8,            // CHR bank 1
    pub mmc1_prg: u8,             // PRG bank selecionado
    // UxROM (Mapper 2) state
    pub uxrom_prg_bank: u8,       // banco selecionável (0x8000–0xBFFF)
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
        if mapper != 0 && mapper != 1 && mapper != 2 {
            return Err(format!(
                "Mapper {} não suportado — apenas Mapper 0 (NROM), Mapper 1 (MMC1) e Mapper 2 (UxROM) estão implementados",
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
        // Aloca CHR-RAM de 8 KB quando não há CHR-ROM (e.g. jogos com CHR-RAM no Mapper 1)
        let chr_ram = if chr_size == 0 { vec![0u8; 8192] } else { Vec::new() };

        Ok(Cartridge {
            prg_rom,
            chr_rom,
            chr_ram,
            mapper,
            mirroring,
            mmc1_shift: 0,
            mmc1_shift_count: 0,
            mmc1_control: 0x0C, // PRG mode 3 por padrão (lo selecionado, hi fixo no último banco)
            mmc1_chr0: 0,
            mmc1_chr1: 0,
            mmc1_prg: 0,
            uxrom_prg_bank: 0,
        })
    }

    /// Lê da PRG-ROM. `addr` já vem relativo a 0x8000 (i.e. 0x0000..=0x7FFF).
    pub fn read_prg(&self, addr: u16) -> u8 {
        if self.prg_rom.is_empty() {
            return 0;
        }
        match self.mapper {
            0 => {
                // NROM: módulo para lidar corretamente com 16 KB (espelhado) e 32 KB
                self.prg_rom[addr as usize % self.prg_rom.len()]
            }
            1 => {
                let prg_banks = self.prg_rom.len() / 0x4000;
                let mode = (self.mmc1_control >> 2) & 0x03;
                let (bank_lo, bank_hi) = match mode {
                    0 | 1 => {
                        // 32KB mode — ignora bit 0
                        let bank = (self.mmc1_prg & 0xFE) as usize;
                        (bank, bank + 1)
                    }
                    2 => (0, self.mmc1_prg as usize),              // lo fixo em 0, hi selecionado
                    3 => (self.mmc1_prg as usize, prg_banks - 1),  // lo selecionado, hi fixo no último
                    _ => (0, prg_banks - 1),
                };
                if addr < 0x4000 {
                    let offset = (bank_lo % prg_banks) * 0x4000 + addr as usize;
                    self.prg_rom[offset]
                } else {
                    let offset = (bank_hi % prg_banks) * 0x4000 + (addr - 0x4000) as usize;
                    self.prg_rom[offset]
                }
            }
            2 => {
                // UxROM: 0x8000–0xBFFF = banco selecionável; 0xC000–0xFFFF = último banco (fixo)
                let prg_banks = self.prg_rom.len() / 0x4000;
                if addr < 0x4000 {
                    let bank = self.uxrom_prg_bank as usize % prg_banks;
                    self.prg_rom[bank * 0x4000 + addr as usize]
                } else {
                    let last = prg_banks - 1;
                    self.prg_rom[last * 0x4000 + (addr - 0x4000) as usize]
                }
            }
            _ => 0,
        }
    }

    /// Processa uma escrita em 0x8000–0xFFFF (mapper register).
    pub fn write_prg(&mut self, addr: u16, value: u8) {
        // UxROM: qualquer escrita em 0x8000–0xFFFF seleciona o banco PRG-LO
        if self.mapper == 2 {
            self.uxrom_prg_bank = value;
            return;
        }

        // MMC1 shift register
        if value & 0x80 != 0 {
            // Reset do shift register
            self.mmc1_shift = 0;
            self.mmc1_shift_count = 0;
            self.mmc1_control |= 0x0C; // força PRG mode 3
            return;
        }

        self.mmc1_shift |= (value & 1) << self.mmc1_shift_count;
        self.mmc1_shift_count += 1;

        if self.mmc1_shift_count == 5 {
            let data = self.mmc1_shift;
            self.mmc1_shift = 0;
            self.mmc1_shift_count = 0;

            match addr {
                0x8000..=0x9FFF => {
                    self.mmc1_control = data;
                    self.mirroring = match data & 0x03 {
                        0 => Mirroring::SingleScreenLow,
                        1 => Mirroring::SingleScreenHigh,
                        2 => Mirroring::Vertical,
                        3 => Mirroring::Horizontal,
                        _ => Mirroring::Horizontal,
                    };
                }
                0xA000..=0xBFFF => self.mmc1_chr0 = data,
                0xC000..=0xDFFF => self.mmc1_chr1 = data,
                0xE000..=0xFFFF => self.mmc1_prg = data & 0x0F,
                _ => {}
            }
        }
    }

    /// Lê do espaço CHR (0x0000..=0x1FFF) com suporte a bank switching MMC1.
    pub fn read_chr(&self, addr: u16) -> u8 {
        match self.mapper {
            0 | 2 => {
                // Mapper 0 (NROM) e Mapper 2 (UxROM): CHR fixo em 8 KB ou CHR-RAM
                if self.chr_rom.is_empty() {
                    self.chr_ram.get(addr as usize & 0x1FFF).copied().unwrap_or(0)
                } else {
                    self.chr_rom[addr as usize & 0x1FFF]
                }
            }
            1 => {
                if self.chr_rom.is_empty() {
                    return self.chr_ram.get(addr as usize & 0x1FFF).copied().unwrap_or(0);
                }
                let chr_mode = (self.mmc1_control >> 4) & 1;
                if chr_mode == 0 {
                    // 8KB mode
                    let bank = (self.mmc1_chr0 & 0xFE) as usize;
                    let offset = bank * 0x2000 + addr as usize;
                    self.chr_rom[offset % self.chr_rom.len()]
                } else {
                    // 4KB mode
                    if addr < 0x1000 {
                        let offset = self.mmc1_chr0 as usize * 0x1000 + addr as usize;
                        self.chr_rom[offset % self.chr_rom.len()]
                    } else {
                        let offset = self.mmc1_chr1 as usize * 0x1000 + (addr - 0x1000) as usize;
                        self.chr_rom[offset % self.chr_rom.len()]
                    }
                }
            }
            _ => 0,
        }
    }

    /// Escreve no CHR-RAM (somente quando chr_rom está vazio).
    pub fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_rom.is_empty() {
            let idx = addr as usize & 0x1FFF;
            if idx < self.chr_ram.len() {
                self.chr_ram[idx] = value;
            }
        }
    }
}
