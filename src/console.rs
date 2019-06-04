use crate::apu::APU;
use crate::cart::CartReadingError;
use crate::controller::ButtonState;
use crate::cpu::CPU;
use crate::memory::MemoryBus;
use crate::minifb::Window;
use crate::ppu::PPU;

use std::sync::mpsc::Sender;

/// Used to act as an owner of everything needed to run a game
/// Is also responsible for holding ram,
/// as well as communication between processors.
pub struct Console {
    apu: APU,
    cpu: CPU,
    ppu: PPU,
}

impl Console {
    pub fn new(
        rom_buffer: &[u8],
        tx: Sender<f32>,
        sample_rate: u32,
    ) -> Result<Self, CartReadingError> {
        // Todo, use an actual sample rate
        // Will fail if the cart couldn't be read
        let mem_res = MemoryBus::with_rom(rom_buffer);
        mem_res.map(|mut memory| {
            let ppu = PPU::new(&mut memory);
            let cpu = CPU::new(memory);
            Console {
                apu: APU::new(tx, sample_rate),
                cpu,
                ppu,
            }
        })
    }

    pub fn step(&mut self) -> i32 {
        let cpucycles = self.cpu.step();
        let m = &mut self.cpu.mem;
        for _ in 0..cpucycles * 3 {
            self.ppu.step(m);
        }
        for _ in 0..cpucycles {
            self.apu.step(m);
        }
        cpucycles
    }

    pub fn step_micros(&mut self, micros: u32) {
        // This emulates 1.79 cpu cycles per microsecond
        let mut cpu_cycles = ((micros * 179) / 100) as i32;
        while cpu_cycles > 0 {
            cpu_cycles = cpu_cycles - self.step();
        }
    }

    pub fn step_frame(&mut self) {
        self.step_micros(1_000_000 / 60);
    }

    pub fn update_window(&self, window: &mut Window) {
        self.ppu.update_window(window);
    }

    pub fn update_controller(&mut self, buttons: ButtonState) {
        self.cpu.set_buttons(buttons);
    }

    /// Resets everything to it's initial state
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.cpu.mem.reset();
        self.ppu.reset(&mut self.cpu.mem);
        self.ppu.clear_vbuffers();
    }

    pub fn print_cpu(&self) {
        self.cpu.print_state();
    }

    pub fn print_ram(&mut self, address: u16) {
        let read = self.cpu.read(address);
        println!("${:X} = {:X}", address, read)
    }
}
