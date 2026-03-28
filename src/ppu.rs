use crate::cartridge::Mirroring;

pub const NES_PALETTE: [(u8, u8, u8); 64] = [
    (84,84,84),(0,30,116),(8,16,144),(48,0,136),(68,0,100),(92,0,48),(84,4,0),(60,24,0),
    (32,42,0),(8,58,0),(0,64,0),(0,60,0),(0,50,60),(0,0,0),(0,0,0),(0,0,0),
    (152,150,152),(8,76,196),(48,50,236),(92,30,228),(136,20,176),(160,20,100),(152,34,32),(120,60,0),
    (84,90,0),(40,114,0),(8,124,0),(0,118,40),(0,102,120),(0,0,0),(0,0,0),(0,0,0),
    (236,238,236),(76,154,236),(120,124,236),(176,98,236),(228,84,236),(236,88,180),(236,106,100),(212,136,32),
    (160,170,0),(116,196,0),(76,208,32),(56,204,108),(56,180,204),(60,60,60),(0,0,0),(0,0,0),
    (236,238,236),(168,204,236),(188,188,236),(212,178,236),(236,174,236),(236,174,212),(236,180,176),(228,196,144),
    (204,210,120),(180,222,120),(168,226,144),(152,226,180),(160,214,228),(160,162,160),(0,0,0),(0,0,0),
];

pub struct Ppu {
    // Memória interna
    pub vram: [u8; 2048],       // nametables
    pub palette: [u8; 32],      // paleta
    pub oam: [u8; 256],         // sprites

    // CHR-ROM, CHR-RAM e mirroring (copiados do cartucho no load_rom)
    pub chr_rom: Vec<u8>,
    pub chr_ram: Vec<u8>,       // 8 KB — usado quando chr_rom está vazio (CHR-RAM)
    pub mirroring: Mirroring,

    // Estado MMC1 para bank switching de CHR (sincronizado do Cartridge pelo Bus)
    pub mapper: u8,
    pub mmc1_chr0: u8,
    pub mmc1_chr1: u8,
    pub mmc1_control: u8,

    // Registradores
    pub ctrl: u8,               // 0x2000
    pub mask: u8,               // 0x2001
    pub status: u8,             // 0x2002
    pub oam_addr: u8,           // 0x2003
    pub scroll_x: u8,
    pub scroll_y: u8,
    pub vram_addr: u16,         // endereço atual (PPUADDR)
    pub vram_addr_latch: bool,  // toggle high/low byte
    pub scroll_latch: bool,
    pub vram_read_buf: u8,      // buffer de leitura do PPUDATA

    // Estado interno
    pub scanline: i16,          // linha atual (0-261, onde 261 é pre-render)
    pub cycle: u16,             // ciclo dentro da scanline (0-340)
    pub frame: u64,             // contador de frames
    pub nmi_triggered: bool,    // sinaliza NMI para a CPU

    // Framebuffer RGB: 256 * 240 * 3 bytes
    pub framebuffer: Vec<u8>,
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            vram: [0u8; 2048],
            palette: [0u8; 32],
            oam: [0u8; 256],
            chr_rom: Vec::new(),
            chr_ram: Vec::new(),
            mirroring: Mirroring::Horizontal,
            mapper: 0,
            mmc1_chr0: 0,
            mmc1_chr1: 0,
            mmc1_control: 0x0C,
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            scroll_x: 0,
            scroll_y: 0,
            vram_addr: 0,
            vram_addr_latch: false,
            scroll_latch: false,
            vram_read_buf: 0,
            scanline: 0,
            cycle: 0,
            frame: 0,
            nmi_triggered: false,
            framebuffer: vec![0u8; 256 * 240 * 3],
        }
    }

    /// Roda 1 ciclo de PPU. Retorna true quando um frame completo foi gerado.
    pub fn tick(&mut self) -> bool {
        let mut frame_complete = false;

        // Início do VBlank: scanline 241, ciclo 1
        if self.scanline == 241 && self.cycle == 1 {
            self.status |= 0x80; // set VBlank
            if self.ctrl & 0x80 != 0 {
                self.nmi_triggered = true;
            }
        }

        // Linha de pré-render: limpa flags
        if self.scanline == 261 && self.cycle == 1 {
            self.status &= !0x80; // clear VBlank
            self.status &= !0x40; // clear Sprite 0 Hit
            self.status &= !0x20; // clear Sprite Overflow
        }

        // Renderiza scanline visível ao ciclo 257 (após todos os pixels)
        if self.scanline < 240 && self.cycle == 257 {
            if self.mask & 0x08 != 0 {
                self.render_background();
            }
            if self.mask & 0x10 != 0 {
                self.render_sprites();
            }
        }

        // Avança ciclo/scanline
        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame = self.frame.wrapping_add(1);
                frame_complete = true;
            }
        }

        frame_complete
    }

    /// Lê registrador PPU (addr já mapeado para 0x2000..=0x2007).
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr & 0x0007 {
            0x0002 => {
                // PPUSTATUS: retorna bits 7-5, limpa VBlank e os latches
                let val = (self.status & 0xE0) | (self.vram_read_buf & 0x1F);
                self.status &= !0x80;
                self.vram_addr_latch = false;
                self.scroll_latch = false;
                val
            }
            0x0004 => self.oam[self.oam_addr as usize], // OAMDATA
            0x0007 => {
                // PPUDATA: leitura com buffer de 1 ciclo
                let addr = self.vram_addr & 0x3FFF;
                let val = if addr >= 0x3F00 {
                    // Paleta não é bufferizada
                    let idx = self.palette_index(addr);
                    self.palette[idx]
                } else {
                    let ret = self.vram_read_buf;
                    if (0x2000..=0x3EFF).contains(&addr) {
                        let mirrored = mirror_vram_addr(addr, self.mirroring) as usize;
                        self.vram_read_buf = self.vram[mirrored];
                    }
                    ret
                };
                let inc: u16 = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc);
                val
            }
            _ => 0,
        }
    }

    /// Escreve em registrador PPU (addr já mapeado para 0x2000..=0x2007).
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x0007 {
            0x0000 => self.ctrl = value,  // PPUCTRL
            0x0001 => self.mask = value,  // PPUMASK
            0x0003 => self.oam_addr = value, // OAMADDR
            0x0004 => {
                // OAMDATA
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x0005 => {
                // PPUSCROLL
                if !self.scroll_latch {
                    self.scroll_x = value;
                } else {
                    self.scroll_y = value;
                }
                self.scroll_latch = !self.scroll_latch;
            }
            0x0006 => {
                // PPUADDR: primeiro byte = high, segundo = low
                if !self.vram_addr_latch {
                    self.vram_addr = (self.vram_addr & 0x00FF) | ((value as u16) << 8);
                } else {
                    self.vram_addr = (self.vram_addr & 0xFF00) | value as u16;
                }
                self.vram_addr_latch = !self.vram_addr_latch;
            }
            0x0007 => {
                // PPUDATA
                let addr = self.vram_addr & 0x3FFF;
                if addr >= 0x3F00 {
                    let idx = self.palette_index(addr);
                    self.palette[idx] = value;
                } else if (0x2000..=0x3EFF).contains(&addr) {
                    let mirrored = mirror_vram_addr(addr, self.mirroring) as usize;
                    self.vram[mirrored] = value;
                } else if addr < 0x2000 && !self.chr_ram.is_empty() {
                    // Escrita em CHR-RAM
                    self.chr_ram[addr as usize] = value;
                }
                let inc: u16 = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc);
            }
            _ => {}
        }
    }

    /// Renderiza uma scanline completa de background no framebuffer (pixel-a-pixel com scroll).
    fn render_background(&mut self) {
        let scanline = self.scanline as u16;
        // Pattern table do background: bit 4 de PPUCTRL
        let pt_base: u16 = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };

        for pixel_x in 0u16..256 {
            let x = pixel_x + self.scroll_x as u16;
            let y = scanline + self.scroll_y as u16;

            let coarse_x = x / 8;
            let fine_x   = x % 8;
            let coarse_y = y / 8;
            let fine_y   = y % 8;

            // Seleciona nametable com wrap, levando em conta PPUCTRL bits 0-1
            let nt_x = ((coarse_x / 32) ^ (self.ctrl as u16 & 0x01)) & 1;
            let nt_y = ((coarse_y / 30) ^ ((self.ctrl as u16 >> 1) & 0x01)) & 1;
            let base_nt: u16 = match (nt_x, nt_y) {
                (0, 0) => 0x2000,
                (1, 0) => 0x2400,
                (0, 1) => 0x2800,
                _      => 0x2C00,
            };

            let nt_addr   = base_nt + (coarse_y % 30) * 32 + (coarse_x % 32);
            let tile_idx  = self.ppu_read(nt_addr) as u16;

            // Atributo de paleta
            let attr_x    = (coarse_x % 32) / 4;
            let attr_y    = (coarse_y % 30) / 4;
            let attr_addr = base_nt + 0x03C0 + attr_y * 8 + attr_x;
            let attr_byte = self.ppu_read(attr_addr);
            let shift     = (((coarse_y % 4) / 2) * 2 + ((coarse_x % 4) / 2)) * 2;
            let palette_idx = (attr_byte >> shift) & 0x03;

            // Dados do padrão (plano 0 e 1)
            let pattern_lo = self.ppu_read(pt_base + tile_idx * 16 + fine_y);
            let pattern_hi = self.ppu_read(pt_base + tile_idx * 16 + fine_y + 8);

            let col_bit  = 7 - fine_x;
            let lo       = (pattern_lo >> col_bit) & 1;
            let hi       = (pattern_hi >> col_bit) & 1;
            let color_idx = (hi << 1) | lo;

            let palette_addr: u16 = if color_idx == 0 {
                0x3F00
            } else {
                0x3F00 + palette_idx as u16 * 4 + color_idx as u16
            };

            let nes_color = (self.ppu_read(palette_addr) & 0x3F) as usize;
            let (r, g, b) = NES_PALETTE[nes_color];
            let offset = (self.scanline as usize * 256 + pixel_x as usize) * 3;
            self.framebuffer[offset]     = r;
            self.framebuffer[offset + 1] = g;
            self.framebuffer[offset + 2] = b;
        }
    }

    /// Renderiza sprites da scanline atual sobre o framebuffer.
    fn render_sprites(&mut self) {
        let scanline = self.scanline;

        // Iteração reversa: sprite de menor índice fica por cima (prioridade correta)
        for i in (0..64usize).rev() {
            let base      = i * 4;
            let sprite_y  = self.oam[base] as i16 + 1;
            let tile_idx  = self.oam[base + 1] as u16;
            let attrs     = self.oam[base + 2];
            let sprite_x  = self.oam[base + 3] as i16;

            // Sprite fora dessa scanline
            if scanline < sprite_y || scanline >= sprite_y + 8 {
                continue;
            }

            let flip_v      = attrs & 0x80 != 0;
            let flip_h      = attrs & 0x40 != 0;
            let palette_idx = (attrs & 0x03) as u16;
            let behind_bg   = attrs & 0x20 != 0;

            let mut row = (scanline - sprite_y) as u16;
            if flip_v { row = 7 - row; }

            // Pattern table de sprite: bit 3 de PPUCTRL
            let table: u16 = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
            let tile_addr  = table + tile_idx * 16 + row;

            let lo = self.ppu_read(tile_addr);
            let hi = self.ppu_read(tile_addr + 8);

            for col in 0..8i16 {
                let bit = if flip_h { col } else { 7 - col } as u16;
                let lo_bit  = (lo >> bit) & 1;
                let hi_bit  = (hi >> bit) & 1;
                let color_idx = (hi_bit << 1) | lo_bit;

                if color_idx == 0 { continue; } // pixel transparente

                let px = sprite_x + col;
                if px < 0 || px >= 256 { continue; }

                let fb_offset = (scanline as usize * 256 + px as usize) * 3;

                // Sprite 0 Hit: sprite 0 com pixel opaco sobre background opaco
                if i == 0 {
                    self.status |= 0x40;
                }

                if !behind_bg {
                    let palette_addr = 0x3F10 + palette_idx * 4 + color_idx as u16;
                    let color = self.ppu_read(palette_addr) & 0x3F;
                    let (r, g, b) = NES_PALETTE[color as usize];
                    self.framebuffer[fb_offset]     = r;
                    self.framebuffer[fb_offset + 1] = g;
                    self.framebuffer[fb_offset + 2] = b;
                }
            }
        }
    }

    /// Lê do espaço de endereçamento da PPU (CHR-ROM/RAM, VRAM, paleta).
    fn ppu_read(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_rom.is_empty() {
                    // CHR-RAM (Mapper 0 sem CHR-ROM ou Mapper 1 com CHR-RAM)
                    self.chr_ram.get(addr as usize).copied().unwrap_or(0)
                } else if self.mapper == 1 {
                    // MMC1: bank switching de CHR
                    let chr_mode = (self.mmc1_control >> 4) & 1;
                    if chr_mode == 0 {
                        // 8KB mode
                        let bank = (self.mmc1_chr0 & 0xFE) as usize;
                        let offset = bank * 0x2000 + addr as usize;
                        self.chr_rom.get(offset % self.chr_rom.len()).copied().unwrap_or(0)
                    } else {
                        // 4KB mode
                        if addr < 0x1000 {
                            let offset = self.mmc1_chr0 as usize * 0x1000 + addr as usize;
                            self.chr_rom.get(offset % self.chr_rom.len()).copied().unwrap_or(0)
                        } else {
                            let offset = self.mmc1_chr1 as usize * 0x1000 + (addr - 0x1000) as usize;
                            self.chr_rom.get(offset % self.chr_rom.len()).copied().unwrap_or(0)
                        }
                    }
                } else {
                    self.chr_rom[addr as usize % self.chr_rom.len()]
                }
            }
            0x2000..=0x3EFF => {
                let mirrored = mirror_vram_addr(addr, self.mirroring) as usize;
                self.vram[mirrored]
            }
            0x3F00..=0x3FFF => {
                let idx = self.palette_index(addr);
                self.palette[idx]
            }
            _ => 0,
        }
    }

    /// Converte endereço de paleta (0x3F00..=0x3FFF) para índice em self.palette[0..32].
    fn palette_index(&self, addr: u16) -> usize {
        let idx = (addr - 0x3F00) as usize % 32;
        // Espelhos de transparência: 0x10/0x14/0x18/0x1C → 0x00/0x04/0x08/0x0C
        match idx {
            0x10 | 0x14 | 0x18 | 0x1C => idx - 0x10,
            _ => idx,
        }
    }
}

/// Converte endereço do espaço de nametable (0x2000..=0x3EFF) para índice em vram[0..2048].
fn mirror_vram_addr(addr: u16, mirroring: Mirroring) -> u16 {
    let addr = (addr & 0x0FFF) as usize; // relativo a 0x2000
    let table = addr / 0x0400;           // qual nametable (0-3)
    let offset = addr % 0x0400;          // offset dentro dela

    let index = match (mirroring, table) {
        (Mirroring::Vertical, 0) => 0,
        (Mirroring::Vertical, 1) => 1,
        (Mirroring::Vertical, 2) => 0,
        (Mirroring::Vertical, 3) => 1,
        (Mirroring::Horizontal, 0) => 0,
        (Mirroring::Horizontal, 1) => 0,
        (Mirroring::Horizontal, 2) => 1,
        (Mirroring::Horizontal, 3) => 1,
        (Mirroring::SingleScreenLow,  _) => 0,
        (Mirroring::SingleScreenHigh, _) => 1,
        _ => 0, // FourScreen e fallback: usa nametable 0
    };

    (index * 0x0400 + offset) as u16
}
