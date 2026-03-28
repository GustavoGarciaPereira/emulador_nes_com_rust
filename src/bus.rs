use crate::cartridge::Cartridge;
use crate::ppu::Ppu;

pub struct Bus {
    pub ram: [u8; 2048],
    pub cartridge: Option<Cartridge>,
    pub ppu: Ppu,
    pub controller1: u8,        // estado atual dos botões (bitmask)
    pub controller1_shift: u8,  // registrador de shift para leitura serial
    pub controller_strobe: bool,
}

impl Bus {
    pub fn new() -> Self {
        Bus {
            ram: [0u8; 2048],
            cartridge: None,
            ppu: Ppu::new(),
            controller1: 0,
            controller1_shift: 0,
            controller_strobe: false,
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // RAM interna 2 KB + espelhos até 0x1FFF
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            // PPU registers (espelhados a cada 8 bytes)
            0x2000..=0x3FFF => self.ppu.read_register(addr & 0x2007),
            // Controlador 1
            0x4016 => {
                let bit = (self.controller1_shift & 0x80) >> 7;
                if !self.controller_strobe {
                    self.controller1_shift <<= 1;
                }
                bit | 0x40 // bits 1-7 retornam 0x40 no hardware real
            }
            // Controlador 2 — não implementado
            0x4017 => 0x40,
            // APU / IO — stub
            0x4000..=0x401F => 0,
            // Expansão / SRAM — stub
            0x4020..=0x7FFF => 0,
            // PRG-ROM do cartucho
            0x8000..=0xFFFF => {
                if let Some(cart) = &self.cartridge {
                    cart.read_prg(addr - 0x8000)
                } else {
                    0
                }
            }
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr & 0x2007, value),
            0x4014 => {
                // OAM DMA: copia 256 bytes de page*256 para OAM
                // Lê direto da fonte adequada para evitar self.read() recursivo
                let page = (value as u16) << 8;
                for i in 0..256u16 {
                    let src = page + i;
                    let byte = match src {
                        0x0000..=0x1FFF => self.ram[(src & 0x07FF) as usize],
                        _ => 0,
                    };
                    self.ppu.oam[i as usize] = byte;
                }
                // Nota: OAM DMA consome 513-514 ciclos de CPU — timing ignorado por ora
            }
            0x4016 => {
                self.controller_strobe = value & 1 == 1;
                if self.controller_strobe {
                    self.controller1_shift = self.controller1; // trava estado atual
                }
            }
            0x4000..=0x401F => {} // APU/IO — stub
            0x4020..=0xFFFF => {} // Escrita em ROM ignorada no Mapper 0
        }
    }
}
