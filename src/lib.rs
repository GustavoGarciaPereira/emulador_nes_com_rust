mod apu;
mod bus;
mod cartridge;
mod cpu;
mod ppu;

use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;
use pyo3::prelude::*;

#[pyclass]
pub struct Nes {
    cpu: Cpu,
    bus: Bus,
}

#[pymethods]
impl Nes {
    #[new]
    fn new() -> Self {
        Nes {
            cpu: Cpu::new(),
            bus: Bus::new(),
        }
    }

    /// Lê o reset vector em 0xFFFC/0xFFFD e inicializa os registradores.
    fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
    }

    /// Executa uma instrução, roda 3 ciclos de PPU por ciclo de CPU e verifica NMI.
    fn step(&mut self) -> u8 {
        let cpu_cycles = self.cpu.step(&mut self.bus);
        for _ in 0..cpu_cycles {
            self.bus.apu.tick();
        }
        for _ in 0..(cpu_cycles as u32 * 3) {
            self.bus.ppu.tick();
            if self.bus.ppu.nmi_triggered {
                self.bus.ppu.nmi_triggered = false;
                self.cpu.nmi(&mut self.bus);
            }
        }
        cpu_cycles
    }

    /// Executa instruções até completar um frame inteiro (PPU frame counter muda).
    fn step_frame(&mut self) {
        let frame = self.bus.ppu.frame;
        while self.bus.ppu.frame == frame {
            let cpu_cycles = self.cpu.step(&mut self.bus);
            for _ in 0..cpu_cycles {
                self.bus.apu.tick();
            }
            for _ in 0..(cpu_cycles as u32 * 3) {
                self.bus.ppu.tick();
                if self.bus.ppu.nmi_triggered {
                    self.bus.ppu.nmi_triggered = false;
                    self.cpu.nmi(&mut self.bus);
                }
            }
        }
    }

    /// Retorna o framebuffer RGB atual (256 * 240 * 3 bytes).
    fn get_framebuffer(&self) -> Vec<u8> {
        self.bus.ppu.framebuffer.clone()
    }

    fn get_pc(&self) -> u16 {
        self.cpu.pc
    }

    fn get_a(&self) -> u8 {
        self.cpu.a
    }

    fn get_x(&self) -> u8 {
        self.cpu.x
    }

    fn get_y(&self) -> u8 {
        self.cpu.y
    }

    fn get_sp(&self) -> u8 {
        self.cpu.sp
    }

    fn get_status(&self) -> u8 {
        self.cpu.status
    }

    fn get_cycles(&self) -> u64 {
        self.cpu.cycles
    }

    fn mem_write(&mut self, addr: u16, value: u8) {
        self.bus.write(addr, value);
    }

    fn mem_read(&mut self, addr: u16) -> u8 {
        self.bus.read(addr)
    }

    pub fn set_pc(&mut self, addr: u16) {
        self.cpu.pc = addr;
    }

    fn set_input(&mut self, buttons: u8) {
        self.bus.controller1 = buttons;
    }

    /// Retorna e esvazia o buffer de amostras de áudio (f32, ~735 por frame a 44100 Hz).
    fn get_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu.take_samples()
    }

    /// Carrega uma ROM iNES, inicializa PPU com chr_rom/mirroring e faz reset da CPU.
    fn load_rom(&mut self, path: &str) -> PyResult<()> {
        let data = std::fs::read(path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        let cart = Cartridge::from_bytes(&data)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))?;

        // Copia CHR-ROM e mirroring para a PPU (acesso direto sem conflito de borrow)
        self.bus.ppu.chr_rom = cart.chr_rom.clone();
        self.bus.ppu.mirroring = cart.mirroring;

        self.bus.cartridge = Some(cart);
        self.cpu.reset(&mut self.bus);
        Ok(())
    }
}

#[pymodule]
fn nes_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Nes>()?;
    Ok(())
}
