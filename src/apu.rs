// APU — MOS 6502 Audio Processing Unit
// Canais: Pulse 1, Pulse 2, Triangle, Noise
// Geração de áudio: ~44100 Hz, mono, f32

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

const NOISE_PERIODS: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

// Padrões de duty cycle: 12.5%, 25%, 50%, 75%
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

// Sequência fixa de 32 passos do canal Triangle
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

// ─────────────────────────── Canal Pulse ────────────────────────────

struct Pulse {
    enabled: bool,
    duty: u8,
    length_halt: bool,   // também é envelope_loop
    constant_volume: bool,
    volume: u8,          // period do envelope ou volume constante
    env_start: bool,
    env_vol: u8,         // valor atual do envelope (0-15)
    env_div: u8,         // divisor do envelope
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_reload: bool,
    sweep_div: u8,
    timer: u16,
    timer_period: u16,
    seq_step: u8,
    length: u8,
    is_pulse1: bool,     // diferença no negate: pulse1 subtrai 1 extra
}

impl Pulse {
    fn new(is_pulse1: bool) -> Self {
        Pulse {
            enabled: false,
            duty: 0,
            length_halt: false,
            constant_volume: false,
            volume: 0,
            env_start: false,
            env_vol: 0,
            env_div: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_reload: false,
            sweep_div: 0,
            timer: 0,
            timer_period: 0,
            seq_step: 0,
            length: 0,
            is_pulse1,
        }
    }

    // $4000 / $4004
    fn write0(&mut self, v: u8) {
        self.duty = (v >> 6) & 3;
        self.length_halt = v & 0x20 != 0;
        self.constant_volume = v & 0x10 != 0;
        self.volume = v & 0x0F;
    }

    // $4001 / $4005 — sweep
    fn write1(&mut self, v: u8) {
        self.sweep_enabled = v & 0x80 != 0;
        self.sweep_period = (v >> 4) & 7;
        self.sweep_negate = v & 0x08 != 0;
        self.sweep_shift = v & 7;
        self.sweep_reload = true;
    }

    // $4002 / $4006 — timer low
    fn write2(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0xFF00) | v as u16;
    }

    // $4003 / $4007 — timer high + length load
    fn write3(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | ((v as u16 & 7) << 8);
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.seq_step = 0;
        self.env_start = true;
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.seq_step = (self.seq_step.wrapping_add(1)) & 7;
        } else {
            self.timer -= 1;
        }
    }

    fn clock_envelope(&mut self) {
        if self.env_start {
            self.env_start = false;
            self.env_vol = 15;
            self.env_div = self.volume;
        } else if self.env_div == 0 {
            self.env_div = self.volume;
            if self.env_vol > 0 {
                self.env_vol -= 1;
            } else if self.length_halt {
                self.env_vol = 15;
            }
        } else {
            self.env_div -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length > 0 {
            self.length -= 1;
        }
    }

    fn clock_sweep(&mut self) {
        if self.sweep_reload {
            self.sweep_div = self.sweep_period;
            self.sweep_reload = false;
            return;
        }
        if self.sweep_div > 0 {
            self.sweep_div -= 1;
            return;
        }
        self.sweep_div = self.sweep_period;
        if self.sweep_enabled && self.sweep_shift > 0 && !self.sweep_muted() {
            let delta = self.timer_period >> self.sweep_shift;
            if self.sweep_negate {
                if self.is_pulse1 {
                    self.timer_period = self.timer_period.wrapping_sub(delta).wrapping_sub(1);
                } else {
                    self.timer_period = self.timer_period.wrapping_sub(delta);
                }
            } else {
                self.timer_period = self.timer_period.wrapping_add(delta);
            }
        }
    }

    // Sweep silencia o canal se o período é muito pequeno ou vai overflow
    fn sweep_muted(&self) -> bool {
        if self.timer_period < 8 {
            return true;
        }
        if !self.sweep_negate {
            let target = self.timer_period.wrapping_add(self.timer_period >> self.sweep_shift);
            if target > 0x7FF {
                return true;
            }
        }
        false
    }

    fn output(&self) -> f32 {
        if !self.enabled || self.length == 0 || self.sweep_muted() {
            return 0.0;
        }
        if DUTY_TABLE[self.duty as usize][self.seq_step as usize] == 0 {
            return 0.0;
        }
        if self.constant_volume {
            self.volume as f32
        } else {
            self.env_vol as f32
        }
    }
}

// ─────────────────────────── Canal Triangle ─────────────────────────

struct Triangle {
    enabled: bool,
    control: bool,          // também é length_halt
    linear_reload_val: u8,
    linear_counter: u8,
    linear_reload_flag: bool,
    timer: u16,
    timer_period: u16,
    step: u8,
    length: u8,
}

impl Triangle {
    fn new() -> Self {
        Triangle {
            enabled: false,
            control: false,
            linear_reload_val: 0,
            linear_counter: 0,
            linear_reload_flag: false,
            timer: 0,
            timer_period: 0,
            step: 0,
            length: 0,
        }
    }

    // $4008
    fn write0(&mut self, v: u8) {
        self.control = v & 0x80 != 0;
        self.linear_reload_val = v & 0x7F;
    }

    // $400A — timer low
    fn write2(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0xFF00) | v as u16;
    }

    // $400B — timer high + length load
    fn write3(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | ((v as u16 & 7) << 8);
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.linear_reload_flag = true;
    }

    // Triangle timer cloca a cada ciclo de CPU (não APU)
    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            if self.length > 0 && self.linear_counter > 0 {
                self.step = (self.step.wrapping_add(1)) & 31;
            }
        } else {
            self.timer -= 1;
        }
    }

    fn clock_linear(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_val;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.control {
            self.linear_reload_flag = false;
        }
    }

    fn clock_length(&mut self) {
        if !self.control && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> f32 {
        // timer_period < 2 gera frequências audíveis demais — silenciar
        if !self.enabled || self.length == 0 || self.linear_counter == 0 || self.timer_period < 2 {
            return 0.0;
        }
        TRIANGLE_SEQUENCE[self.step as usize] as f32
    }
}

// ─────────────────────────── Canal Noise ────────────────────────────

struct Noise {
    enabled: bool,
    length_halt: bool,
    constant_volume: bool,
    volume: u8,
    env_start: bool,
    env_vol: u8,
    env_div: u8,
    mode: bool,       // false = normal (bit 1), true = short (bit 6)
    timer: u16,
    timer_period: u16,
    shift: u16,       // LFSR 15-bit, inicializado em 1
    length: u8,
}

impl Noise {
    fn new() -> Self {
        Noise {
            enabled: false,
            length_halt: false,
            constant_volume: false,
            volume: 0,
            env_start: false,
            env_vol: 0,
            env_div: 0,
            mode: false,
            timer: 0,
            timer_period: 0,
            shift: 1,
            length: 0,
        }
    }

    // $400C
    fn write0(&mut self, v: u8) {
        self.length_halt = v & 0x20 != 0;
        self.constant_volume = v & 0x10 != 0;
        self.volume = v & 0x0F;
    }

    // $400E — mode + period
    fn write2(&mut self, v: u8) {
        self.mode = v & 0x80 != 0;
        self.timer_period = NOISE_PERIODS[(v & 0x0F) as usize];
    }

    // $400F — length load
    fn write3(&mut self, v: u8) {
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.env_start = true;
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            let other_bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift ^ (self.shift >> other_bit)) & 1;
            self.shift = (self.shift >> 1) | (feedback << 14);
        } else {
            self.timer -= 1;
        }
    }

    fn clock_envelope(&mut self) {
        if self.env_start {
            self.env_start = false;
            self.env_vol = 15;
            self.env_div = self.volume;
        } else if self.env_div == 0 {
            self.env_div = self.volume;
            if self.env_vol > 0 {
                self.env_vol -= 1;
            } else if self.length_halt {
                self.env_vol = 15;
            }
        } else {
            self.env_div -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> f32 {
        // Bit 0 do LFSR = 1 → silêncio
        if !self.enabled || self.length == 0 || self.shift & 1 != 0 {
            return 0.0;
        }
        if self.constant_volume {
            self.volume as f32
        } else {
            self.env_vol as f32
        }
    }
}

// ─────────────────────────── APU principal ──────────────────────────

pub struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,

    // Frame counter
    frame_counter: u32,
    frame_mode: bool,       // false = 4-step, true = 5-step
    frame_irq_inhibit: bool,
    apu_divider: bool,      // inverte a cada ciclo de CPU → pulso a cada 2 ciclos

    // Geração de amostras
    sample_rate: f32,       // 44100.0 Hz
    cpu_clock: f32,         // 1_789_773.0 Hz
    sample_accum: f32,
    pub sample_buffer: Vec<f32>,
}

impl Apu {
    pub fn new() -> Self {
        Apu {
            pulse1: Pulse::new(true),
            pulse2: Pulse::new(false),
            triangle: Triangle::new(),
            noise: Noise::new(),
            frame_counter: 0,
            frame_mode: false,
            frame_irq_inhibit: false,
            apu_divider: false,
            sample_rate: 44100.0,
            cpu_clock: 1_789_773.0,
            sample_accum: 0.0,
            sample_buffer: Vec::with_capacity(1024),
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000 => self.pulse1.write0(value),
            0x4001 => self.pulse1.write1(value),
            0x4002 => self.pulse1.write2(value),
            0x4003 => self.pulse1.write3(value),
            0x4004 => self.pulse2.write0(value),
            0x4005 => self.pulse2.write1(value),
            0x4006 => self.pulse2.write2(value),
            0x4007 => self.pulse2.write3(value),
            0x4008 => self.triangle.write0(value),
            0x400A => self.triangle.write2(value),
            0x400B => self.triangle.write3(value),
            0x400C => self.noise.write0(value),
            0x400E => self.noise.write2(value),
            0x400F => self.noise.write3(value),
            0x4015 => {
                self.pulse1.enabled   = value & 0x01 != 0;
                self.pulse2.enabled   = value & 0x02 != 0;
                self.triangle.enabled = value & 0x04 != 0;
                self.noise.enabled    = value & 0x08 != 0;
                if !self.pulse1.enabled   { self.pulse1.length   = 0; }
                if !self.pulse2.enabled   { self.pulse2.length   = 0; }
                if !self.triangle.enabled { self.triangle.length = 0; }
                if !self.noise.enabled    { self.noise.length    = 0; }
            }
            0x4017 => {
                self.frame_mode = value & 0x80 != 0;
                self.frame_irq_inhibit = value & 0x40 != 0;
                self.frame_counter = 0;
                // 5-step: dispara quarter + half imediatamente ao resetar
                if self.frame_mode {
                    self.clock_quarter();
                    self.clock_half();
                }
            }
            _ => {}
        }
    }

    /// Cloca o APU por 1 ciclo de CPU. Chamado pela CPU após cada instrução.
    pub fn tick(&mut self) {
        // Triangle cloca seu timer a cada ciclo de CPU
        self.triangle.clock_timer();

        // Pulse e Noise clocam a cada 2 ciclos de CPU (divisor APU)
        self.apu_divider = !self.apu_divider;
        if !self.apu_divider {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
        }

        // Frame counter (em ciclos de CPU)
        self.frame_counter = self.frame_counter.wrapping_add(1);
        let fc = self.frame_counter;
        if !self.frame_mode {
            // 4-step mode
            match fc {
                3729  => self.clock_quarter(),
                7457  => { self.clock_quarter(); self.clock_half(); }
                11186 => self.clock_quarter(),
                14915 => {
                    self.clock_quarter();
                    self.clock_half();
                    self.frame_counter = 0;
                }
                _ => {}
            }
        } else {
            // 5-step mode
            match fc {
                3729  => self.clock_quarter(),
                7457  => { self.clock_quarter(); self.clock_half(); }
                11186 => self.clock_quarter(),
                // step 4 sem clock (14915 é idle)
                18641 => {
                    self.clock_quarter();
                    self.clock_half();
                    self.frame_counter = 0;
                }
                _ => {}
            }
        }

        // Gerar amostra de áudio na taxa correta
        self.sample_accum += self.sample_rate / self.cpu_clock;
        if self.sample_accum >= 1.0 {
            self.sample_accum -= 1.0;
            self.sample_buffer.push(self.mix());
        }
    }

    fn clock_quarter(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear();
        self.noise.clock_envelope();
    }

    fn clock_half(&mut self) {
        self.pulse1.clock_length();
        self.pulse1.clock_sweep();
        self.pulse2.clock_length();
        self.pulse2.clock_sweep();
        self.triangle.clock_length();
        self.noise.clock_length();
    }

    fn mix(&self) -> f32 {
        let p1  = self.pulse1.output();
        let p2  = self.pulse2.output();
        let tri = self.triangle.output();
        let nz  = self.noise.output();
        // Fórmula de mixing do NES (aproximação linear da tabela de lookup)
        let pulse_out = 0.00752 * (p1 + p2);
        let tnd_out   = 0.00851 * tri + 0.00494 * nz;
        pulse_out + tnd_out
    }

    /// Retorna e esvazia o buffer de amostras acumuladas desde a última chamada.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.sample_buffer)
    }
}
