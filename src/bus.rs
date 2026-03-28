use crate::cartridge::Cartridge;
use crate::ppu::Ppu;

pub struct Bus {
    pub ram: [u8; 2048],
    pub cartridge: Option<Cartridge>,
    pub ppu: Ppu,
}

impl Bus {
    pub fn new() -> Self {
        Bus {
            ram: [0u8; 2048],
            cartridge: None,
            ppu: Ppu::new(),
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // RAM interna 2 KB + espelhos até 0x1FFF
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            // PPU registers (espelhados a cada 8 bytes)
            0x2000..=0x3FFF => self.ppu.read_register(addr & 0x2007),
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
                // OAM DMA — stub por enquanto (copiaria 256 bytes de RAM para OAM)
            }
            0x4000..=0x401F => {} // APU/IO — stub
            0x4020..=0xFFFF => {} // Escrita em ROM ignorada no Mapper 0
        }
    }
}
