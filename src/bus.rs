use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::ppu::Ppu;

pub struct Bus {
    pub ram: [u8; 2048],
    pub cartridge: Option<Cartridge>,
    pub ppu: Ppu,
    pub apu: Apu,
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
            apu: Apu::new(),
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
            // APU status
            0x4015 => 0, // stub: retorna 0 (sem IRQ, sem DMC)
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
            // APU: Pulse 1, Pulse 2, Triangle, Noise, DMC, status, frame counter
            // (0x4014 = OAM DMA e 0x4016 = controller já tratados acima)
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write(addr, value),
            0x4018..=0x401F => {} // expansão — ignorado
            0x4020..=0x7FFF => {} // Expansão / SRAM — ignorado
            0x8000..=0xFFFF => {
                // Escreve no mapper (MMC1 shift register, etc.)
                // Lê o estado atualizado em variáveis locais para evitar conflito de borrow
                // entre self.cartridge e self.ppu
                let sync = if let Some(cart) = &mut self.cartridge {
                    cart.write_prg(addr, value);
                    Some((cart.mmc1_chr0, cart.mmc1_chr1, cart.mmc1_control, cart.mirroring))
                } else {
                    None
                };
                if let Some((chr0, chr1, control, mirroring)) = sync {
                    self.ppu.mmc1_chr0 = chr0;
                    self.ppu.mmc1_chr1 = chr1;
                    self.ppu.mmc1_control = control;
                    self.ppu.mirroring = mirroring;
                }
            }
        }
    }
}
