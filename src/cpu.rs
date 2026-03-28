use crate::bus::Bus;

// Flags do registrador de status (bit position)
const FLAG_C: u8 = 1 << 0; // Carry
const FLAG_Z: u8 = 1 << 1; // Zero
const FLAG_I: u8 = 1 << 2; // Interrupt Disable
const FLAG_D: u8 = 1 << 3; // Decimal (ignorado no NES)
const FLAG_B: u8 = 1 << 4; // Break
const FLAG_U: u8 = 1 << 5; // Unused (sempre 1)
const FLAG_V: u8 = 1 << 6; // Overflow
const FLAG_N: u8 = 1 << 7; // Negative

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum AddrMode {
    Imp, // Implied
    Acc, // Accumulator
    Imm, // Immediate
    Zp,  // Zero Page
    Zpx, // Zero Page, X
    Zpy, // Zero Page, Y
    Abs, // Absolute
    Abx, // Absolute, X
    Aby, // Absolute, Y
    Ind, // Indirect
    Izx, // (Indirect, X)
    Izy, // (Indirect), Y
    Rel, // Relative
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub cycles: u64,
}

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            status: 0x24, // FLAG_U e FLAG_I setados por padrão
            cycles: 0,
        }
    }

    pub fn reset(&mut self, bus: &mut Bus) {
        let lo = bus.read(0xFFFC) as u16;
        let hi = bus.read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;
        self.sp = 0xFD;
        self.status = 0x24;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.cycles += 8;
    }

    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        let opcode = self.fetch(bus);
        let elapsed = self.execute(opcode, bus);
        self.cycles += elapsed as u64;
        elapsed
    }

    /// Dispara NMI: salva PC e status na pilha, pula para o vetor 0xFFFA/0xFFFB.
    pub fn nmi(&mut self, bus: &mut Bus) {
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, self.pc as u8);
        // Salva status com B limpo (NMI não seta B)
        let status = self.status & !0x10;
        self.push(bus, status);
        self.status |= FLAG_I;
        let lo = bus.read(0xFFFA) as u16;
        let hi = bus.read(0xFFFB) as u16;
        self.pc = (hi << 8) | lo;
        self.cycles += 7;
    }

    // -------------------------------------------------------------------------
    // Helpers de flags
    // -------------------------------------------------------------------------

    fn set_flag(&mut self, flag: u8, condition: bool) {
        if condition {
            self.status |= flag;
        } else {
            self.status &= !flag;
        }
    }

    fn get_flag(&self, flag: u8) -> bool {
        self.status & flag != 0
    }

    // Atualiza Negative e Zero com base em value
    fn update_nz(&mut self, value: u8) {
        self.set_flag(FLAG_Z, value == 0);
        self.set_flag(FLAG_N, value & 0x80 != 0);
    }

    // -------------------------------------------------------------------------
    // Helpers de fetch
    // -------------------------------------------------------------------------

    fn fetch(&mut self, bus: &mut Bus) -> u8 {
        let val = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        val
    }

    fn fetch_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch(bus) as u16;
        let hi = self.fetch(bus) as u16;
        (hi << 8) | lo
    }

    // -------------------------------------------------------------------------
    // Helpers de pilha (stack em 0x0100–0x01FF)
    // -------------------------------------------------------------------------

    fn push(&mut self, bus: &mut Bus, value: u8) {
        bus.write(0x0100 | self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pop(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | self.sp as u16)
    }

    fn push_word(&mut self, bus: &mut Bus, value: u16) {
        self.push(bus, (value >> 8) as u8);
        self.push(bus, (value & 0xFF) as u8);
    }

    fn pop_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.pop(bus) as u16;
        let hi = self.pop(bus) as u16;
        (hi << 8) | lo
    }

    // -------------------------------------------------------------------------
    // Resolução de endereço por modo de endereçamento
    // Retorna (endereço efetivo, page_crossed)
    // -------------------------------------------------------------------------

    fn get_address(&mut self, mode: AddrMode, bus: &mut Bus) -> (u16, bool) {
        match mode {
            AddrMode::Imm => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (addr, false)
            }
            AddrMode::Zp => {
                let addr = self.fetch(bus) as u16;
                (addr, false)
            }
            AddrMode::Zpx => {
                let base = self.fetch(bus);
                (base.wrapping_add(self.x) as u16, false)
            }
            AddrMode::Zpy => {
                let base = self.fetch(bus);
                (base.wrapping_add(self.y) as u16, false)
            }
            AddrMode::Abs => {
                let addr = self.fetch_word(bus);
                (addr, false)
            }
            AddrMode::Abx => {
                let base = self.fetch_word(bus);
                let addr = base.wrapping_add(self.x as u16);
                let crossed = (base & 0xFF00) != (addr & 0xFF00);
                (addr, crossed)
            }
            AddrMode::Aby => {
                let base = self.fetch_word(bus);
                let addr = base.wrapping_add(self.y as u16);
                let crossed = (base & 0xFF00) != (addr & 0xFF00);
                (addr, crossed)
            }
            AddrMode::Ind => {
                let ptr = self.fetch_word(bus);
                // Bug do 6502: se ptr termina em 0xFF, o hi byte não cruza de página
                let lo = bus.read(ptr) as u16;
                let hi = if ptr & 0x00FF == 0x00FF {
                    bus.read(ptr & 0xFF00) as u16
                } else {
                    bus.read(ptr + 1) as u16
                };
                ((hi << 8) | lo, false)
            }
            AddrMode::Izx => {
                let base = self.fetch(bus);
                let ptr = base.wrapping_add(self.x) as u16;
                let lo = bus.read(ptr & 0x00FF) as u16;
                let hi = bus.read((ptr + 1) & 0x00FF) as u16;
                ((hi << 8) | lo, false)
            }
            AddrMode::Izy => {
                let ptr = self.fetch(bus) as u16;
                let lo = bus.read(ptr & 0x00FF) as u16;
                let hi = bus.read((ptr + 1) & 0x00FF) as u16;
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let crossed = (base & 0xFF00) != (addr & 0xFF00);
                (addr, crossed)
            }
            AddrMode::Rel => {
                let offset = self.fetch(bus) as i8;
                let addr = self.pc.wrapping_add(offset as u16);
                (addr, false)
            }
            AddrMode::Imp | AddrMode::Acc => (0, false),
        }
    }

    // -------------------------------------------------------------------------
    // Operações aritméticas / lógicas
    // -------------------------------------------------------------------------

    fn adc(&mut self, value: u8) {
        let a = self.a as u16;
        let v = value as u16;
        let c = self.get_flag(FLAG_C) as u16;
        let result = a + v + c;
        self.set_flag(FLAG_C, result > 0xFF);
        // Overflow: sinal de A e M iguais, mas resultado tem sinal diferente
        self.set_flag(FLAG_V, (!(a ^ v) & (a ^ result)) & 0x80 != 0);
        self.a = result as u8;
        self.update_nz(self.a);
    }

    fn sbc(&mut self, value: u8) {
        // SBC é ADC com o operando invertido (complemento de 1)
        self.adc(value ^ 0xFF);
    }

    fn compare(&mut self, reg: u8, value: u8) {
        let result = reg.wrapping_sub(value);
        self.set_flag(FLAG_C, reg >= value);
        self.update_nz(result);
    }

    // -------------------------------------------------------------------------
    // Shifts e rotações em memória
    // -------------------------------------------------------------------------

    fn asl_mem(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        let v = bus.read(addr);
        self.set_flag(FLAG_C, v & 0x80 != 0);
        let result = v << 1;
        bus.write(addr, result);
        result
    }

    fn lsr_mem(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        let v = bus.read(addr);
        self.set_flag(FLAG_C, v & 0x01 != 0);
        let result = v >> 1;
        bus.write(addr, result);
        result
    }

    fn rol_mem(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        let v = bus.read(addr);
        let new_carry = v & 0x80 != 0;
        let result = (v << 1) | (self.get_flag(FLAG_C) as u8);
        self.set_flag(FLAG_C, new_carry);
        bus.write(addr, result);
        result
    }

    fn ror_mem(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        let v = bus.read(addr);
        let new_carry = v & 0x01 != 0;
        let result = (v >> 1) | ((self.get_flag(FLAG_C) as u8) << 7);
        self.set_flag(FLAG_C, new_carry);
        bus.write(addr, result);
        result
    }

    // -------------------------------------------------------------------------
    // Branches
    // -------------------------------------------------------------------------

    fn branch(&mut self, bus: &mut Bus, condition: bool) -> u8 {
        let (target, _) = self.get_address(AddrMode::Rel, bus);
        if condition {
            let page_crossed = (self.pc & 0xFF00) != (target & 0xFF00);
            self.pc = target;
            if page_crossed { 4 } else { 3 }
        } else {
            2
        }
    }

    // -------------------------------------------------------------------------
    // Decode + execute
    // -------------------------------------------------------------------------

    fn execute(&mut self, opcode: u8, bus: &mut Bus) -> u8 {
        match opcode {
            // -----------------------------------------------------------------
            // LDA
            // -----------------------------------------------------------------
            0xA9 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.a = bus.read(a); self.update_nz(self.a); 2 }
            0xA5 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.a = bus.read(a); self.update_nz(self.a); 3 }
            0xB5 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); self.a = bus.read(a); self.update_nz(self.a); 4 }
            0xAD => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.a = bus.read(a); self.update_nz(self.a); 4 }
            0xBD => { let (a, c) = self.get_address(AddrMode::Abx, bus); self.a = bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0xB9 => { let (a, c) = self.get_address(AddrMode::Aby, bus); self.a = bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0xA1 => { let (a, _) = self.get_address(AddrMode::Izx, bus); self.a = bus.read(a); self.update_nz(self.a); 6 }
            0xB1 => { let (a, c) = self.get_address(AddrMode::Izy, bus); self.a = bus.read(a); self.update_nz(self.a); 5 + c as u8 }

            // -----------------------------------------------------------------
            // LDX
            // -----------------------------------------------------------------
            0xA2 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.x = bus.read(a); self.update_nz(self.x); 2 }
            0xA6 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.x = bus.read(a); self.update_nz(self.x); 3 }
            0xB6 => { let (a, _) = self.get_address(AddrMode::Zpy, bus); self.x = bus.read(a); self.update_nz(self.x); 4 }
            0xAE => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.x = bus.read(a); self.update_nz(self.x); 4 }
            0xBE => { let (a, c) = self.get_address(AddrMode::Aby, bus); self.x = bus.read(a); self.update_nz(self.x); 4 + c as u8 }

            // -----------------------------------------------------------------
            // LDY
            // -----------------------------------------------------------------
            0xA0 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.y = bus.read(a); self.update_nz(self.y); 2 }
            0xA4 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.y = bus.read(a); self.update_nz(self.y); 3 }
            0xB4 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); self.y = bus.read(a); self.update_nz(self.y); 4 }
            0xAC => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.y = bus.read(a); self.update_nz(self.y); 4 }
            0xBC => { let (a, c) = self.get_address(AddrMode::Abx, bus); self.y = bus.read(a); self.update_nz(self.y); 4 + c as u8 }

            // -----------------------------------------------------------------
            // STA
            // -----------------------------------------------------------------
            0x85 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); bus.write(a, self.a); 3 }
            0x95 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); bus.write(a, self.a); 4 }
            0x8D => { let (a, _) = self.get_address(AddrMode::Abs, bus); bus.write(a, self.a); 4 }
            0x9D => { let (a, _) = self.get_address(AddrMode::Abx, bus); bus.write(a, self.a); 5 }
            0x99 => { let (a, _) = self.get_address(AddrMode::Aby, bus); bus.write(a, self.a); 5 }
            0x81 => { let (a, _) = self.get_address(AddrMode::Izx, bus); bus.write(a, self.a); 6 }
            0x91 => { let (a, _) = self.get_address(AddrMode::Izy, bus); bus.write(a, self.a); 6 }

            // -----------------------------------------------------------------
            // STX
            // -----------------------------------------------------------------
            0x86 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); bus.write(a, self.x); 3 }
            0x96 => { let (a, _) = self.get_address(AddrMode::Zpy, bus); bus.write(a, self.x); 4 }
            0x8E => { let (a, _) = self.get_address(AddrMode::Abs, bus); bus.write(a, self.x); 4 }

            // -----------------------------------------------------------------
            // STY
            // -----------------------------------------------------------------
            0x84 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); bus.write(a, self.y); 3 }
            0x94 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); bus.write(a, self.y); 4 }
            0x8C => { let (a, _) = self.get_address(AddrMode::Abs, bus); bus.write(a, self.y); 4 }

            // -----------------------------------------------------------------
            // Transferências entre registradores
            // -----------------------------------------------------------------
            0xAA => { self.x = self.a;  self.update_nz(self.x);  2 } // TAX
            0xA8 => { self.y = self.a;  self.update_nz(self.y);  2 } // TAY
            0x8A => { self.a = self.x;  self.update_nz(self.a);  2 } // TXA
            0x98 => { self.a = self.y;  self.update_nz(self.a);  2 } // TYA
            0xBA => { self.x = self.sp; self.update_nz(self.x);  2 } // TSX
            0x9A => { self.sp = self.x;                          2 } // TXS (sem flags)

            // -----------------------------------------------------------------
            // Pilha
            // -----------------------------------------------------------------
            0x48 => { let v = self.a; self.push(bus, v); 3 }                                         // PHA
            0x68 => { let v = self.pop(bus); self.a = v; self.update_nz(self.a); 4 }                 // PLA
            0x08 => { let s = self.status | FLAG_B | FLAG_U; self.push(bus, s); 3 }                  // PHP
            0x28 => { let s = self.pop(bus); self.status = (s | FLAG_U) & !FLAG_B; 4 }               // PLP

            // -----------------------------------------------------------------
            // AND
            // -----------------------------------------------------------------
            0x29 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.a &= bus.read(a); self.update_nz(self.a); 2 }
            0x25 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.a &= bus.read(a); self.update_nz(self.a); 3 }
            0x35 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); self.a &= bus.read(a); self.update_nz(self.a); 4 }
            0x2D => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.a &= bus.read(a); self.update_nz(self.a); 4 }
            0x3D => { let (a, c) = self.get_address(AddrMode::Abx, bus); self.a &= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x39 => { let (a, c) = self.get_address(AddrMode::Aby, bus); self.a &= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x21 => { let (a, _) = self.get_address(AddrMode::Izx, bus); self.a &= bus.read(a); self.update_nz(self.a); 6 }
            0x31 => { let (a, c) = self.get_address(AddrMode::Izy, bus); self.a &= bus.read(a); self.update_nz(self.a); 5 + c as u8 }

            // -----------------------------------------------------------------
            // ORA
            // -----------------------------------------------------------------
            0x09 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.a |= bus.read(a); self.update_nz(self.a); 2 }
            0x05 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.a |= bus.read(a); self.update_nz(self.a); 3 }
            0x15 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); self.a |= bus.read(a); self.update_nz(self.a); 4 }
            0x0D => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.a |= bus.read(a); self.update_nz(self.a); 4 }
            0x1D => { let (a, c) = self.get_address(AddrMode::Abx, bus); self.a |= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x19 => { let (a, c) = self.get_address(AddrMode::Aby, bus); self.a |= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x01 => { let (a, _) = self.get_address(AddrMode::Izx, bus); self.a |= bus.read(a); self.update_nz(self.a); 6 }
            0x11 => { let (a, c) = self.get_address(AddrMode::Izy, bus); self.a |= bus.read(a); self.update_nz(self.a); 5 + c as u8 }

            // -----------------------------------------------------------------
            // EOR
            // -----------------------------------------------------------------
            0x49 => { let (a, _) = self.get_address(AddrMode::Imm, bus); self.a ^= bus.read(a); self.update_nz(self.a); 2 }
            0x45 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); self.a ^= bus.read(a); self.update_nz(self.a); 3 }
            0x55 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); self.a ^= bus.read(a); self.update_nz(self.a); 4 }
            0x4D => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.a ^= bus.read(a); self.update_nz(self.a); 4 }
            0x5D => { let (a, c) = self.get_address(AddrMode::Abx, bus); self.a ^= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x59 => { let (a, c) = self.get_address(AddrMode::Aby, bus); self.a ^= bus.read(a); self.update_nz(self.a); 4 + c as u8 }
            0x41 => { let (a, _) = self.get_address(AddrMode::Izx, bus); self.a ^= bus.read(a); self.update_nz(self.a); 6 }
            0x51 => { let (a, c) = self.get_address(AddrMode::Izy, bus); self.a ^= bus.read(a); self.update_nz(self.a); 5 + c as u8 }

            // -----------------------------------------------------------------
            // ADC
            // -----------------------------------------------------------------
            0x69 => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.adc(v); 2 }
            0x65 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.adc(v); 3 }
            0x75 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a); self.adc(v); 4 }
            0x6D => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.adc(v); 4 }
            0x7D => { let (a, c) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a); self.adc(v); 4 + c as u8 }
            0x79 => { let (a, c) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a); self.adc(v); 4 + c as u8 }
            0x61 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a); self.adc(v); 6 }
            0x71 => { let (a, c) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a); self.adc(v); 5 + c as u8 }

            // -----------------------------------------------------------------
            // SBC
            // -----------------------------------------------------------------
            0xE9 => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.sbc(v); 2 }
            0xE5 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.sbc(v); 3 }
            0xF5 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a); self.sbc(v); 4 }
            0xED => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.sbc(v); 4 }
            0xFD => { let (a, c) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a); self.sbc(v); 4 + c as u8 }
            0xF9 => { let (a, c) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a); self.sbc(v); 4 + c as u8 }
            0xE1 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a); self.sbc(v); 6 }
            0xF1 => { let (a, c) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a); self.sbc(v); 5 + c as u8 }

            // -----------------------------------------------------------------
            // CMP
            // -----------------------------------------------------------------
            0xC9 => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.compare(self.a, v); 2 }
            0xC5 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.compare(self.a, v); 3 }
            0xD5 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a); self.compare(self.a, v); 4 }
            0xCD => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.compare(self.a, v); 4 }
            0xDD => { let (a, c) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a); self.compare(self.a, v); 4 + c as u8 }
            0xD9 => { let (a, c) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a); self.compare(self.a, v); 4 + c as u8 }
            0xC1 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a); self.compare(self.a, v); 6 }
            0xD1 => { let (a, c) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a); self.compare(self.a, v); 5 + c as u8 }

            // -----------------------------------------------------------------
            // CPX
            // -----------------------------------------------------------------
            0xE0 => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.compare(self.x, v); 2 }
            0xE4 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.compare(self.x, v); 3 }
            0xEC => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.compare(self.x, v); 4 }

            // -----------------------------------------------------------------
            // CPY
            // -----------------------------------------------------------------
            0xC0 => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.compare(self.y, v); 2 }
            0xC4 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.compare(self.y, v); 3 }
            0xCC => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.compare(self.y, v); 4 }

            // -----------------------------------------------------------------
            // INC
            // -----------------------------------------------------------------
            0xE6 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.update_nz(v); 5 }
            0xF6 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.update_nz(v); 6 }
            0xEE => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.update_nz(v); 6 }
            0xFE => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // DEC
            // -----------------------------------------------------------------
            0xC6 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.update_nz(v); 5 }
            0xD6 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.update_nz(v); 6 }
            0xCE => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.update_nz(v); 6 }
            0xDE => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // INX / INY / DEX / DEY
            // -----------------------------------------------------------------
            0xE8 => { self.x = self.x.wrapping_add(1); self.update_nz(self.x); 2 } // INX
            0xC8 => { self.y = self.y.wrapping_add(1); self.update_nz(self.y); 2 } // INY
            0xCA => { self.x = self.x.wrapping_sub(1); self.update_nz(self.x); 2 } // DEX
            0x88 => { self.y = self.y.wrapping_sub(1); self.update_nz(self.y); 2 } // DEY

            // -----------------------------------------------------------------
            // ASL
            // -----------------------------------------------------------------
            0x0A => {
                let c = self.a & 0x80 != 0;
                self.a <<= 1;
                self.set_flag(FLAG_C, c);
                self.update_nz(self.a);
                2
            }
            0x06 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.asl_mem(bus, a); self.update_nz(v); 5 }
            0x16 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.asl_mem(bus, a); self.update_nz(v); 6 }
            0x0E => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.asl_mem(bus, a); self.update_nz(v); 6 }
            0x1E => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.asl_mem(bus, a); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // LSR
            // -----------------------------------------------------------------
            0x4A => {
                let c = self.a & 0x01 != 0;
                self.a >>= 1;
                self.set_flag(FLAG_C, c);
                self.update_nz(self.a);
                2
            }
            0x46 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.lsr_mem(bus, a); self.update_nz(v); 5 }
            0x56 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.lsr_mem(bus, a); self.update_nz(v); 6 }
            0x4E => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.lsr_mem(bus, a); self.update_nz(v); 6 }
            0x5E => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.lsr_mem(bus, a); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // ROL
            // -----------------------------------------------------------------
            0x2A => {
                let new_c = self.a & 0x80 != 0;
                self.a = (self.a << 1) | (self.get_flag(FLAG_C) as u8);
                self.set_flag(FLAG_C, new_c);
                self.update_nz(self.a);
                2
            }
            0x26 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.rol_mem(bus, a); self.update_nz(v); 5 }
            0x36 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.rol_mem(bus, a); self.update_nz(v); 6 }
            0x2E => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.rol_mem(bus, a); self.update_nz(v); 6 }
            0x3E => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.rol_mem(bus, a); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // ROR
            // -----------------------------------------------------------------
            0x6A => {
                let new_c = self.a & 0x01 != 0;
                self.a = (self.a >> 1) | ((self.get_flag(FLAG_C) as u8) << 7);
                self.set_flag(FLAG_C, new_c);
                self.update_nz(self.a);
                2
            }
            0x66 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.ror_mem(bus, a); self.update_nz(v); 5 }
            0x76 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.ror_mem(bus, a); self.update_nz(v); 6 }
            0x6E => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.ror_mem(bus, a); self.update_nz(v); 6 }
            0x7E => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.ror_mem(bus, a); self.update_nz(v); 7 }

            // -----------------------------------------------------------------
            // JMP
            // -----------------------------------------------------------------
            0x4C => { let (a, _) = self.get_address(AddrMode::Abs, bus); self.pc = a; 3 }
            0x6C => { let (a, _) = self.get_address(AddrMode::Ind, bus); self.pc = a; 5 }

            // -----------------------------------------------------------------
            // JSR / RTS
            // -----------------------------------------------------------------
            0x20 => {
                let (target, _) = self.get_address(AddrMode::Abs, bus);
                let ret = self.pc.wrapping_sub(1); // aponta pro último byte da instrução JSR
                self.push_word(bus, ret);
                self.pc = target;
                6
            }
            0x60 => {
                let addr = self.pop_word(bus);
                self.pc = addr.wrapping_add(1);
                6
            }

            // -----------------------------------------------------------------
            // Branches
            // -----------------------------------------------------------------
            0xF0 => { let cond = self.get_flag(FLAG_Z);  self.branch(bus, cond)  } // BEQ
            0xD0 => { let cond = !self.get_flag(FLAG_Z); self.branch(bus, cond)  } // BNE
            0xB0 => { let cond = self.get_flag(FLAG_C);  self.branch(bus, cond)  } // BCS
            0x90 => { let cond = !self.get_flag(FLAG_C); self.branch(bus, cond)  } // BCC
            0x30 => { let cond = self.get_flag(FLAG_N);  self.branch(bus, cond)  } // BMI
            0x10 => { let cond = !self.get_flag(FLAG_N); self.branch(bus, cond)  } // BPL
            0x70 => { let cond = self.get_flag(FLAG_V);  self.branch(bus, cond)  } // BVS
            0x50 => { let cond = !self.get_flag(FLAG_V); self.branch(bus, cond)  } // BVC

            // -----------------------------------------------------------------
            // Flags
            // -----------------------------------------------------------------
            0x38 => { self.set_flag(FLAG_C, true);  2 } // SEC
            0x18 => { self.set_flag(FLAG_C, false); 2 } // CLC
            0x78 => { self.set_flag(FLAG_I, true);  2 } // SEI
            0x58 => { self.set_flag(FLAG_I, false); 2 } // CLI
            0xF8 => { self.set_flag(FLAG_D, true);  2 } // SED
            0xD8 => { self.set_flag(FLAG_D, false); 2 } // CLD
            0xB8 => { self.set_flag(FLAG_V, false); 2 } // CLV

            // -----------------------------------------------------------------
            // NOP
            // -----------------------------------------------------------------
            0xEA => 2,

            // -----------------------------------------------------------------
            // BIT
            // -----------------------------------------------------------------
            0x24 => {
                let (a, _) = self.get_address(AddrMode::Zp, bus);
                let v = bus.read(a);
                self.set_flag(FLAG_Z, self.a & v == 0);
                self.set_flag(FLAG_N, v & 0x80 != 0);
                self.set_flag(FLAG_V, v & 0x40 != 0);
                3
            }
            0x2C => {
                let (a, _) = self.get_address(AddrMode::Abs, bus);
                let v = bus.read(a);
                self.set_flag(FLAG_Z, self.a & v == 0);
                self.set_flag(FLAG_N, v & 0x80 != 0);
                self.set_flag(FLAG_V, v & 0x40 != 0);
                4
            }

            // -----------------------------------------------------------------
            // BRK
            // -----------------------------------------------------------------
            0x00 => {
                self.pc = self.pc.wrapping_add(1); // byte de padding após o opcode
                let pc = self.pc;
                let status = self.status | FLAG_B | FLAG_U;
                self.push_word(bus, pc);
                self.push(bus, status);
                self.set_flag(FLAG_I, true);
                let lo = bus.read(0xFFFE) as u16;
                let hi = bus.read(0xFFFF) as u16;
                self.pc = (hi << 8) | lo;
                7
            }

            // -----------------------------------------------------------------
            // RTI
            // -----------------------------------------------------------------
            0x40 => {
                let s = self.pop(bus);
                self.status = (s | FLAG_U) & !FLAG_B;
                let pc = self.pop_word(bus);
                self.pc = pc;
                6
            }

            // =================================================================
            // OPCODES ILEGAIS / NÃO-OFICIAIS
            // =================================================================

            // -----------------------------------------------------------------
            // *NOP extras — apenas consomem bytes do PC sem efeito
            // -----------------------------------------------------------------
            // Implied (1 byte, 2 ciclos)
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => 2,

            // Immediate (2 bytes, 2 ciclos)
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                let _ = self.get_address(AddrMode::Imm, bus);
                2
            }
            // Zero Page (2 bytes, 3 ciclos)
            0x04 | 0x44 | 0x64 => {
                let _ = self.get_address(AddrMode::Zp, bus);
                3
            }
            // Zero Page,X (2 bytes, 4 ciclos)
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                let _ = self.get_address(AddrMode::Zpx, bus);
                4
            }
            // Absolute (3 bytes, 4 ciclos)
            0x0C => {
                let _ = self.get_address(AddrMode::Abs, bus);
                4
            }
            // Absolute,X (3 bytes, 4+1 ciclos)
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let (_, c) = self.get_address(AddrMode::Abx, bus);
                4 + c as u8
            }

            // -----------------------------------------------------------------
            // *LAX — lê memória, armazena em A e X, atualiza NZ
            // -----------------------------------------------------------------
            0xA3 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 6 }
            0xA7 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 3 }
            0xAF => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 4 }
            0xB3 => { let (a, c) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 5 + c as u8 }
            0xB7 => { let (a, _) = self.get_address(AddrMode::Zpy, bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 4 }
            0xBF => { let (a, c) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a); self.a = v; self.x = v; self.update_nz(v); 4 + c as u8 }

            // -----------------------------------------------------------------
            // *SAX — escreve (A & X) na memória, sem afetar flags
            // -----------------------------------------------------------------
            0x83 => { let (a, _) = self.get_address(AddrMode::Izx, bus); bus.write(a, self.a & self.x); 6 }
            0x87 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); bus.write(a, self.a & self.x); 3 }
            0x8F => { let (a, _) = self.get_address(AddrMode::Abs, bus); bus.write(a, self.a & self.x); 4 }
            0x97 => { let (a, _) = self.get_address(AddrMode::Zpy, bus); bus.write(a, self.a & self.x); 4 }

            // -----------------------------------------------------------------
            // *SBC ilegal — idêntico ao oficial
            // -----------------------------------------------------------------
            0xEB => { let (a, _) = self.get_address(AddrMode::Imm, bus); let v = bus.read(a); self.sbc(v); 2 }

            // -----------------------------------------------------------------
            // *DCP — DEC memória, depois CMP com A
            // -----------------------------------------------------------------
            0xC3 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 8 }
            0xC7 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 5 }
            0xCF => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 6 }
            0xD3 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 8 }
            0xD7 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 6 }
            0xDB => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 7 }
            0xDF => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a).wrapping_sub(1); bus.write(a, v); self.compare(self.a, v); 7 }

            // -----------------------------------------------------------------
            // *ISC — INC memória, depois SBC com A
            // -----------------------------------------------------------------
            0xE3 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 8 }
            0xE7 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 5 }
            0xEF => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 6 }
            0xF3 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 8 }
            0xF7 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 6 }
            0xFB => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 7 }
            0xFF => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = bus.read(a).wrapping_add(1); bus.write(a, v); self.sbc(v); 7 }

            // -----------------------------------------------------------------
            // *RLA — ROL memória, depois AND com A
            // -----------------------------------------------------------------
            0x23 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 8 }
            0x27 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 5 }
            0x2F => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 6 }
            0x33 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 8 }
            0x37 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 6 }
            0x3B => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 7 }
            0x3F => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.rol_mem(bus, a); self.a &= v; self.update_nz(self.a); 7 }

            // -----------------------------------------------------------------
            // *SLO — ASL memória, depois ORA com A
            // -----------------------------------------------------------------
            0x03 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 8 }
            0x07 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 5 }
            0x0F => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 6 }
            0x13 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 8 }
            0x17 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 6 }
            0x1B => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 7 }
            0x1F => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.asl_mem(bus, a); self.a |= v; self.update_nz(self.a); 7 }

            // -----------------------------------------------------------------
            // *SRE — LSR memória, depois EOR com A
            // -----------------------------------------------------------------
            0x43 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 8 }
            0x47 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 5 }
            0x4F => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 6 }
            0x53 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 8 }
            0x57 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 6 }
            0x5B => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 7 }
            0x5F => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.lsr_mem(bus, a); self.a ^= v; self.update_nz(self.a); 7 }

            // -----------------------------------------------------------------
            // *RRA — ROR memória, depois ADC com A
            // (o carry do ROR é imediatamente consumido pelo ADC)
            // -----------------------------------------------------------------
            0x63 => { let (a, _) = self.get_address(AddrMode::Izx, bus); let v = self.ror_mem(bus, a); self.adc(v); 8 }
            0x67 => { let (a, _) = self.get_address(AddrMode::Zp,  bus); let v = self.ror_mem(bus, a); self.adc(v); 5 }
            0x6F => { let (a, _) = self.get_address(AddrMode::Abs, bus); let v = self.ror_mem(bus, a); self.adc(v); 6 }
            0x73 => { let (a, _) = self.get_address(AddrMode::Izy, bus); let v = self.ror_mem(bus, a); self.adc(v); 8 }
            0x77 => { let (a, _) = self.get_address(AddrMode::Zpx, bus); let v = self.ror_mem(bus, a); self.adc(v); 6 }
            0x7B => { let (a, _) = self.get_address(AddrMode::Aby, bus); let v = self.ror_mem(bus, a); self.adc(v); 7 }
            0x7F => { let (a, _) = self.get_address(AddrMode::Abx, bus); let v = self.ror_mem(bus, a); self.adc(v); 7 }

            // Qualquer outro opcode não mapeado — NOP de 2 ciclos
            _ => 2,
        }
    }
}
