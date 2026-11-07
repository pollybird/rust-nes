//! Simplified NES APU (2A03 core): two pulse channels, triangle, noise, and the
//! frame sequencer. DMC is stubbed (no samples, status bits follow writes).
//!
//! Timing model: pulse/noise timers run at half the CPU clock (APU cycles),
//! the triangle timer at the full CPU clock. The frame counter clocks
//! envelopes + triangle linear counter every quarter frame (~240Hz in 4-step
//! mode) and length counters + sweeps every half frame (~120Hz).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const CPU_CYCLES_PER_SEC: f64 = 1_789_773.0; // NTSC

pub struct Ring {
    pub buf: VecDeque<f32>,
    pub capacity: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Ring { buf: VecDeque::with_capacity(cap), capacity: cap }
    }
    #[inline]
    pub fn push(&mut self, s: f32) {
        if self.buf.len() >= self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(s);
    }
    #[inline]
    pub fn drain_into(&mut self, out: &mut [f32], channels: usize) {
        let mut i = 0;
        while i < out.len() {
            let s = self.buf.pop_front().unwrap_or(0.0);
            out[i] = s;
            for c in 1..channels {
                if i + c < out.len() {
                    out[i + c] = s;
                }
            }
            i += channels;
        }
    }
}

pub type SharedRing = Arc<Mutex<Ring>>;

pub const PULSE_DUTY: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96,
    22, 192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Clone)]
struct Pulse {
    enabled: bool, // $4015 channel-enable bit
    is_p1: bool,
    duty: u8,
    period: u16, // 11-bit timer period
    timer: u16,
    seq: u8,
    length: u8,
    // envelope
    env_start: bool,
    env_div: u8,
    env_counter: u8,
    volume: u8,
    const_vol: bool,
    env_loop: bool, // doubles as length-counter halt
    // sweep
    swp_enabled: bool,
    swp_period: u8,
    swp_negate: bool,
    swp_shift: u8,
    swp_timer: u8,
    swp_reload: bool,
    output: f32,
}

impl Pulse {
    fn new(is_p1: bool) -> Self {
        Pulse {
            enabled: false,
            is_p1,
            duty: 0,
            period: 0,
            timer: 0,
            seq: 0,
            length: 0,
            env_start: false,
            env_div: 0,
            env_counter: 0,
            volume: 0,
            const_vol: false,
            env_loop: false,
            swp_enabled: false,
            swp_period: 0,
            swp_negate: false,
            swp_shift: 0,
            swp_timer: 0,
            swp_reload: false,
            output: 0.0,
        }
    }

    #[inline]
    fn envelope(&self) -> u8 {
        if self.const_vol {
            self.volume
        } else {
            self.env_counter
        }
    }

    /// Sweep target period, continuously evaluated (drives muting too).
    fn sweep_target(&self) -> u16 {
        let delta = self.period >> self.swp_shift;
        if self.swp_negate {
            // pulse1 negates with ones' complement (-delta-1), pulse2 with twos'
            let extra = if self.is_p1 { 1 } else { 0 };
            self.period.wrapping_sub(delta + extra)
        } else {
            self.period + delta
        }
    }

    #[inline]
    fn sweep_muted(&self) -> bool {
        self.period < 8 || self.sweep_target() > 0x7FF
    }

    /// APU-cycle timer clock (call every second CPU cycle).
    fn tick_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.period;
            self.seq = (self.seq + 1) & 7;
        }
        if self.length > 0 && !self.sweep_muted() {
            self.output = PULSE_DUTY[self.duty as usize & 3][self.seq as usize] as f32
                * self.envelope() as f32;
        } else {
            self.output = 0.0;
        }
    }

    /// Quarter frame.
    fn env_tick(&mut self) {
        if self.env_start {
            self.env_start = false;
            self.env_counter = 15;
            self.env_div = self.volume;
        } else if self.env_div > 0 {
            self.env_div -= 1;
        } else {
            self.env_div = self.volume;
            if self.env_counter > 0 {
                self.env_counter -= 1;
            } else if self.env_loop {
                self.env_counter = 15;
            }
        }
    }

    /// Half frame.
    fn sweep_tick(&mut self) {
        if self.swp_timer == 0 && self.swp_enabled && self.swp_shift > 0 && !self.sweep_muted()
        {
            self.period = self.sweep_target();
        }
        if self.swp_timer == 0 || self.swp_reload {
            self.swp_timer = self.swp_period;
            self.swp_reload = false;
        } else {
            self.swp_timer -= 1;
        }
    }

    /// Half frame.
    fn length_tick(&mut self) {
        if !self.env_loop && self.length > 0 {
            self.length -= 1;
        }
    }

    fn write_reg(&mut self, off: u8, val: u8) {
        match off {
            0 => {
                self.duty = val >> 6;
                self.env_loop = val & 0x20 != 0;
                self.const_vol = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            1 => {
                self.swp_enabled = val & 0x80 != 0;
                self.swp_period = (val >> 4) & 7;
                self.swp_negate = val & 0x08 != 0;
                self.swp_shift = val & 0x07;
                self.swp_reload = true;
            }
            2 => {
                self.period = (self.period & 0x700) | val as u16;
            }
            3 => {
                self.period = (self.period & 0xFF) | (((val & 7) as u16) << 8);
                if self.enabled {
                    self.length = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.seq = 0;
                self.env_start = true;
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct Triangle {
    enabled: bool,
    control: bool, // $4008 bit7: length halt / linear hold
    linear_reload: bool,
    linear_period: u8,
    linear_counter: u8,
    length: u8,
    period: u16,
    timer: u16,
    step: u8,
    output: f32,
}

const TRI_SERIES: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15,
];

impl Triangle {
    fn new() -> Self {
        Triangle {
            enabled: false,
            control: false,
            linear_reload: false,
            linear_period: 0,
            linear_counter: 0,
            length: 0,
            period: 0,
            timer: 0,
            step: 0,
            output: 0.0,
        }
    }
    /// CPU-cycle timer clock. When halted (length/linear zero) the sequence
    /// stops advancing and the output holds its current level.
    fn tick_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            return;
        }
        self.timer = self.period;
        if self.length > 0 && self.linear_counter > 0 {
            self.step = (self.step + 1) & 31;
            self.output = TRI_SERIES[self.step as usize] as f32;
        }
    }
    /// Quarter frame.
    fn clock_linear(&mut self) {
        if self.linear_reload {
            self.linear_counter = self.linear_period;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.control {
            self.linear_reload = false;
        }
    }
    /// Half frame.
    fn length_tick(&mut self) {
        if !self.control && self.length > 0 {
            self.length -= 1;
        }
    }
    fn write_reg(&mut self, off: u8, val: u8) {
        match off {
            0 => {
                self.control = val & 0x80 != 0;
                self.linear_period = val & 0x7F;
            }
            2 => {
                self.period = (self.period & 0x700) | val as u16;
            }
            3 => {
                self.period = (self.period & 0xFF) | (((val as u16) & 7) << 8);
                if self.enabled {
                    self.length = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.linear_reload = true;
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct Noise {
    enabled: bool,
    length: u8,
    env_start: bool,
    env_div: u8,
    env_counter: u8,
    volume: u8,
    const_vol: bool,
    env_loop: bool,
    short_mode: bool,
    period: u16, // in APU cycles
    timer: u16,
    shift_reg: u16,
    output: f32,
}

/// Noise timer periods in APU cycles (NTSC).
const NOISE_PERIODS: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

impl Noise {
    fn new() -> Self {
        Noise {
            enabled: false,
            length: 0,
            env_start: false,
            env_div: 0,
            env_counter: 0,
            volume: 0,
            const_vol: false,
            env_loop: false,
            short_mode: false,
            period: 4,
            timer: 0,
            shift_reg: 1,
            output: 0.0,
        }
    }
    #[inline]
    fn envelope(&self) -> u8 {
        if self.const_vol {
            self.volume
        } else {
            self.env_counter
        }
    }
    /// APU-cycle timer clock (every second CPU cycle).
    fn tick_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
        } else {
            self.timer = self.period;
            let tap2 = if self.short_mode { 6 } else { 1 };
            let fb = ((self.shift_reg ^ (self.shift_reg >> tap2)) & 1) as u16;
            self.shift_reg = (self.shift_reg >> 1) | (fb << 14);
        }
        if self.length > 0 && self.shift_reg & 1 == 0 {
            self.output = self.envelope() as f32;
        } else {
            self.output = 0.0;
        }
    }
    /// Quarter frame.
    fn env_tick(&mut self) {
        if self.env_start {
            self.env_start = false;
            self.env_counter = 15;
            self.env_div = self.volume;
        } else if self.env_div > 0 {
            self.env_div -= 1;
        } else {
            self.env_div = self.volume;
            if self.env_counter > 0 {
                self.env_counter -= 1;
            } else if self.env_loop {
                self.env_counter = 15;
            }
        }
    }
    /// Half frame.
    fn length_tick(&mut self) {
        if !self.env_loop && self.length > 0 {
            self.length -= 1;
        }
    }
    fn write_reg(&mut self, off: u8, val: u8) {
        match off {
            0 => {
                self.env_loop = val & 0x20 != 0;
                self.const_vol = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            2 => {
                self.period = NOISE_PERIODS[(val & 0x0F) as usize];
                self.short_mode = val & 0x80 != 0;
            }
            3 => {
                if self.enabled {
                    self.length = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.env_start = true;
            }
            _ => {}
        }
    }
}

pub struct Apu {
    p1: Pulse,
    p2: Pulse,
    tri: Triangle,
    noise: Noise,
    half: bool, // APU/CPU cycle parity
    sequencer: u8,
    sequencer_counter: u16,
    mode_irq_inhibit: bool, // $4017 bit6
    five_step: bool,        // $4017 bit7
    irq_occurred: bool,
    cycle: u64,
    next_sample_cycle: u64,
    cycles_per_sample: f64,
    sample_rate: f64,
    ring: Option<SharedRing>,
    muted: bool,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

/// Quarter-frame timing in CPU cycles (relative step deltas).
const SEQ_PERIOD_4: [u16; 4] = [7457, 7457, 7457, 7459];
const SEQ_PERIOD_5: [u16; 5] = [7457, 7457, 7457, 7458, 7454];

impl Apu {
    pub fn new() -> Self {
        Apu {
            p1: Pulse::new(true),
            p2: Pulse::new(false),
            tri: Triangle::new(),
            noise: Noise::new(),
            half: false,
            sequencer: 0,
            sequencer_counter: SEQ_PERIOD_4[0],
            mode_irq_inhibit: false,
            five_step: false,
            irq_occurred: false,
            cycle: 0,
            next_sample_cycle: 0,
            cycles_per_sample: CPU_CYCLES_PER_SEC / 44100.0,
            sample_rate: 44100.0,
            ring: None,
            muted: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Apu::new();
    }

    pub fn set_ring(&mut self, ring: Option<SharedRing>, rate: f64) {
        self.sample_rate = rate;
        self.cycles_per_sample = CPU_CYCLES_PER_SEC / rate;
        self.ring = ring;
    }

    pub fn set_muted(&mut self, m: bool) {
        self.muted = m;
    }

    pub fn irq_line(&self) -> bool {
        self.irq_occurred && !self.mode_irq_inhibit
    }

    /// Advance APU by n CPU cycles and push any crossed audio samples to the ring.
    pub fn advance(&mut self, n: u32) {
        for _ in 0..n {
            // triangle runs at CPU rate; pulse/noise at half (APU) rate
            self.tri.tick_timer();
            self.half = !self.half;
            if self.half {
                self.p1.tick_timer();
                self.p2.tick_timer();
                self.noise.tick_timer();
            }

            if self.sequencer_counter > 0 {
                self.sequencer_counter -= 1;
            } else {
                self.sequencer_step();
            }

            self.cycle += 1;
            if self.ring.is_some() && self.cycle >= self.next_sample_cycle {
                let s = if self.muted { 0.0 } else { self.sample() };
                if let Some(r) = &self.ring {
                    if let Ok(mut g) = r.lock() {
                        g.push(s);
                    }
                }
                self.next_sample_cycle =
                    (self.cycle as f64 + self.cycles_per_sample).floor() as u64;
            }
        }
    }

    fn sequencer_step(&mut self) {
        let step = self.sequencer as usize;
        let five = self.five_step;
        // quarter-frame units: envelopes + triangle linear counter
        let quarter = !(five && step == 4);
        // half-frame units: length counters + sweep
        let half = step == 1 || step == 3;
        if quarter {
            self.p1.env_tick();
            self.p2.env_tick();
            self.noise.env_tick();
            self.tri.clock_linear();
        }
        if half {
            self.p1.length_tick();
            self.p2.length_tick();
            self.tri.length_tick();
            self.noise.length_tick();
            self.p1.sweep_tick();
            self.p2.sweep_tick();
        }
        if !five && step == 3 {
            self.irq_occurred = true;
        }
        let steps = if five { 5 } else { 4 };
        self.sequencer = (self.sequencer + 1) % steps;
        self.sequencer_counter = if five {
            SEQ_PERIOD_5[self.sequencer as usize]
        } else {
            SEQ_PERIOD_4[self.sequencer as usize]
        };
    }

    // ---------------- register file ----------------

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000 => self.p1.write_reg(0, val),
            0x4001 => self.p1.write_reg(1, val),
            0x4002 => self.p1.write_reg(2, val),
            0x4003 => self.p1.write_reg(3, val),
            0x4004 => self.p2.write_reg(0, val),
            0x4005 => self.p2.write_reg(1, val),
            0x4006 => self.p2.write_reg(2, val),
            0x4007 => self.p2.write_reg(3, val),
            0x4008 => self.tri.write_reg(0, val),
            0x400A => self.tri.write_reg(2, val),
            0x400B => self.tri.write_reg(3, val),
            0x400C => self.noise.write_reg(0, val),
            0x400E => self.noise.write_reg(2, val),
            0x400F => self.noise.write_reg(3, val),
            0x4010..=0x4013 => {
                // DMC control registers accepted but not synthesized
            }
            0x4015 => {
                self.p1.enabled = val & 1 != 0;
                self.p2.enabled = val & 2 != 0;
                self.tri.enabled = val & 4 != 0;
                self.noise.enabled = val & 8 != 0;
                if !self.p1.enabled {
                    self.p1.length = 0;
                }
                if !self.p2.enabled {
                    self.p2.length = 0;
                }
                if !self.tri.enabled {
                    self.tri.length = 0;
                }
                if !self.noise.enabled {
                    self.noise.length = 0;
                }
            }
            0x4017 => {
                self.five_step = val & 0x80 != 0;
                self.mode_irq_inhibit = val & 0x40 != 0;
                if self.mode_irq_inhibit {
                    self.irq_occurred = false;
                }
                self.sequencer = 0;
                self.sequencer_counter = SEQ_PERIOD_4[0];
                // 5-step write clocks quarter+half-frame units immediately
                if self.five_step {
                    self.p1.env_tick();
                    self.p2.env_tick();
                    self.noise.env_tick();
                    self.tri.clock_linear();
                    self.p1.length_tick();
                    self.p2.length_tick();
                    self.tri.length_tick();
                    self.noise.length_tick();
                    self.p1.sweep_tick();
                    self.p2.sweep_tick();
                }
            }
            _ => {}
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let mut r = 0u8;
                if self.p1.length > 0 {
                    r |= 1;
                }
                if self.p2.length > 0 {
                    r |= 2;
                }
                if self.tri.length > 0 {
                    r |= 4;
                }
                if self.noise.length > 0 {
                    r |= 8;
                }
                if self.irq_occurred {
                    r |= 0x40;
                }
                self.irq_occurred = false;
                r
            }
            _ => 0,
        }
    }

    // ---------------- mixing ----------------

    /// NES non-linear DAC mix.
    pub fn sample(&self) -> f32 {
        let p = self.p1.output + self.p2.output;
        let pulse = if p > 0.0 {
            95.88 / (8128.0 / p + 100.0)
        } else {
            0.0
        };
        let t = self.tri.output / 8227.0 + self.noise.output / 12241.0;
        let tnd = if t > 0.0 {
            159.79 / (1.0 / t + 100.0)
        } else {
            0.0
        };
        ((pulse + tnd) * 1.5).clamp(0.0, 1.0)
    }
}
