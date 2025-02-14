use alloc::boxed::Box;

use super::memory::{Mapper, MemoryBus};

use crate::ports::{PixelBuffer, VideoDevice};

const PALETTE: [u32; 64] = [
    0xFF75_7575,
    0xFF27_1B8F,
    0xFF00_00AB,
    0xFF47_009F,
    0xFF8F_0077,
    0xFFAB_0013,
    0xFFA7_0000,
    0xFF7F_0B00,
    0xFF43_2F00,
    0xFF00_4700,
    0xFF00_5100,
    0xFF00_3F17,
    0xFF1B_3F5F,
    0xFF00_0000,
    0xFF00_0000,
    0xFF00_0000,
    0xFFBC_BCBC,
    0xFF00_73EF,
    0xFF23_3BEF,
    0xFF83_00F3,
    0xFFBF_00BF,
    0xFFE7_005B,
    0xFFDB_2B00,
    0xFFCB_4F0F,
    0xFF8B_7300,
    0xFF00_9700,
    0xFF00_AB00,
    0xFF00_933B,
    0xFF00_838B,
    0xFF00_0000,
    0xFF00_0000,
    0xFF00_0000,
    0xFFFF_FFFF,
    0xFF3F_BFFF,
    0xFF5F_97FF,
    0xFFA7_8BFD,
    0xFFF7_7BFF,
    0xFFFF_77B7,
    0xFFFF_7763,
    0xFFFF_9B3B,
    0xFFF3_BF3F,
    0xFF83_D313,
    0xFF4F_DF4B,
    0xFF58_F898,
    0xFF00_EBDB,
    0xFF00_0000,
    0xFF00_0000,
    0xFF00_0000,
    0xFFFF_FFFF,
    0xFFAB_E7FF,
    0xFFC7_D7FF,
    0xFFD7_CBFF,
    0xFFFF_C7FF,
    0xFFFF_C7DB,
    0xFFFF_BFB3,
    0xFFFF_DBAB,
    0xFFFF_E7A3,
    0xFFE3_FFA3,
    0xFFAB_F3BF,
    0xFFB3_FFCF,
    0xFF9F_FFF3,
    0xFF00_0000,
    0xFF00_0000,
    0xFF00_0000,
];

struct NameTables([u8; 2048]);

impl Default for NameTables {
    fn default() -> Self {
        NameTables([0; 2048])
    }
}

impl NameTables {
    fn read(&self, addr: u16) -> u8 {
        self.0[(addr % 2048) as usize]
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.0[(addr % 2048) as usize] = val;
    }
}

pub struct OAM(pub [u8; 256]);

impl Default for OAM {
    fn default() -> Self {
        OAM([0; 256])
    }
}

/// Represents openly modifiable PPU state
#[derive(Default)]
pub struct PPUState {
    // Memory
    palettes: [u8; 32],
    nametables: NameTables,
    pub oam: OAM, // public to allow cpu DMA transfer
    /// Current vram address (15 bit)
    pub v: u16, // Public for access during CPU IO reading
    /// Temporary vram address (15 bit)
    t: u16,
    /// Write toggle (1 bit)
    w: u8,
    /// Fine x scroll (3 bit)
    x: u8,
    register: u8,
    // Nmi flags
    nmi_occurred: bool,
    nmi_output: bool,
    nmi_previous: bool,
    nmi_delay: u8,

    // $2000 PPUCTRL
    // 0: $2000, 1: $2400, 2: $2800, 3: $2C00
    flg_nametable: u8,
    // 0: add 1, 1: add 32
    pub flg_increment: u8, // Pub for access during Bus IO
    // 0: $0000, 1: $1000
    flg_spritetable: u8,
    // 0: $0000, 1: $1000
    flg_backgroundtable: u8,
    // 0: 8x8, 1: 8x16
    flg_spritesize: u8,
    // 0: read EXT, 1: write EXT
    flg_masterslave: u8,

    // $2001 PPUMASK
    // 0: color, 1: grayscale
    flg_grayscale: u8,
    // 0: hide, 1: show
    flg_showleftbg: u8,
    // 0: hide, 1: sho
    flg_showleftsprites: u8,
    // 0: hide, 1: show
    flg_showbg: u8,
    // 0: hide, 1: show
    flg_showsprites: u8,
    // 0: normal, 1: emphasized
    flg_redtint: u8,
    // 0: normal, 1: emphasized
    flg_greentint: u8,
    // 0: normal, 1: emphasized
    flg_bluetint: u8,

    // $2002 PPUSTATUS
    flg_sprite0hit: u8,
    flg_spriteoverflow: u8,

    // $2003 OAMADDR
    pub oam_address: u8, // Pub for DMA transfer

    // $2007 PPUDATA
    pub buffer_data: u8, // Pub for Bus access during CPU IO
}

impl PPUState {
    pub fn new() -> Self {
        PPUState::default()
    }

    fn nmi_change(&mut self) {
        let nmi = self.nmi_output && self.nmi_occurred;
        if nmi && !self.nmi_previous {
            self.nmi_delay = 15;
        }
        self.nmi_previous = nmi;
    }

    fn read(&self, mapper: &dyn Mapper, address: u16) -> u8 {
        let wrapped = address % 0x4000;
        match wrapped {
            a if a < 0x2000 => mapper.read(a),
            a if a < 0x3F00 => {
                let mode = mapper.mirroring_mode();
                let mirrored = mode.mirror_address(a);
                self.nametables.read(mirrored)
            }
            a if a < 0x4000 => self.read_palette(a % 32),
            a => {
                panic!("Unhandled PPU memory read at {:X}", a);
            }
        }
    }

    fn write(&mut self, mapper: &mut dyn Mapper, address: u16, value: u8) {
        let wrapped = address % 0x4000;
        match wrapped {
            a if a < 0x2000 => mapper.write(a, value),
            a if a < 0x3F00 => {
                let mode = mapper.mirroring_mode();
                let mirrored = mode.mirror_address(a);
                self.nametables.write(mirrored, value);
            }
            a if a < 0x4000 => {
                self.write_palette(a % 32, value);
            }
            a => {
                panic!("Unhandled PPU memory write at {:X}", a);
            }
        }
    }

    fn read_palette(&self, address: u16) -> u8 {
        let wrapped = if address >= 16 && address % 4 == 0 {
            address - 16
        } else {
            address
        };
        self.palettes[wrapped as usize]
    }

    fn write_palette(&mut self, address: u16, value: u8) {
        let wrapped = if address >= 16 && address % 4 == 0 {
            address - 16
        } else {
            address
        };
        self.palettes[wrapped as usize] = value;
    }

    /// Needs the wrapper because it might read from CHR data
    pub fn read_register(&mut self, m: &dyn Mapper, address: u16) -> u8 {
        match address {
            0x2002 => self.read_status(),
            0x2004 => self.read_oam_data(),
            0x2007 => self.read_data(m),
            _ => 0,
        }
    }

    fn read_status(&mut self) -> u8 {
        let mut res = self.register & 0x1F;
        res |= self.flg_spriteoverflow << 5;
        res |= self.flg_sprite0hit << 6;
        if self.nmi_occurred {
            res |= 1 << 7;
        }
        self.nmi_occurred = false;
        self.nmi_change();
        self.w = 0;
        res
    }

    fn read_oam_data(&self) -> u8 {
        self.oam.0[self.oam_address as usize]
    }

    fn read_data(&mut self, mapper: &dyn Mapper) -> u8 {
        let v = self.v;
        let mut value = self.read(mapper, v);
        if v % 0x4000 < 0x3F00 {
            core::mem::swap(&mut self.buffer_data, &mut value);
        } else {
            let read = self.read(mapper, v - 0x1000);
            self.buffer_data = read;
        }
        if self.flg_increment == 0 {
            self.v += 1;
        } else {
            self.v += 32;
        }
        value
    }

    pub fn write_register(&mut self, mapper: &mut dyn Mapper, address: u16, value: u8) {
        self.register = value;
        match address {
            0x2000 => self.write_control(value),
            0x2001 => self.write_mask(value),
            0x2003 => self.write_oam_address(value),
            0x2004 => self.write_oam_data(value),
            0x2005 => self.write_scroll(value),
            0x2006 => self.write_address(value),
            0x2007 => self.write_data(mapper, value),
            // This case can never be reached, since the address is % 8,
            _ => {}
        }
    }

    // See: https://wiki.nesdev.com/w/index.php/PPU_registers#PPUCTRL
    fn write_control(&mut self, value: u8) {
        self.flg_nametable = value & 2;
        self.flg_increment = (value >> 2) & 1;
        self.flg_spritetable = (value >> 3) & 1;
        self.flg_backgroundtable = (value >> 4) & 1;
        self.flg_spritesize = (value >> 5) & 1;
        self.flg_masterslave = (value >> 6) & 1;
        self.nmi_output = (value >> 7) & 1 == 1;
        self.nmi_change();
        self.t = (self.t & 0xF3FF) | ((u16::from(value) & 0x03) << 10);
    }

    fn write_mask(&mut self, value: u8) {
        self.flg_grayscale = value & 1;
        self.flg_showleftbg = (value >> 1) & 1;
        self.flg_showleftsprites = (value >> 2) & 1;
        self.flg_showbg = (value >> 3) & 1;
        self.flg_showsprites = (value >> 4) & 1;
        self.flg_redtint = (value >> 5) & 1;
        self.flg_greentint = (value >> 6) & 1;
        self.flg_bluetint = (value >> 7) & 1;
    }

    fn write_oam_address(&mut self, value: u8) {
        self.oam_address = value;
    }

    fn write_oam_data(&mut self, value: u8) {
        let a = self.oam_address as usize;
        self.oam.0[a] = value;
        self.oam_address = self.oam_address.wrapping_add(1);
    }

    fn write_scroll(&mut self, value: u8) {
        if self.w == 0 {
            self.t = (self.t & 0x7FE0) | (u16::from(value) >> 3);
            self.x = value & 0x7;
            self.w = 1;
        } else {
            let s1 = (u16::from(value) & 0x7) << 12;
            self.t = (self.t & 0xC1F) | ((u16::from(value) & 0xF8) << 2) | s1;
            self.w = 0;
        }
    }

    fn write_address(&mut self, value: u8) {
        if self.w == 0 {
            self.t = (self.t & 0x80FF) | ((u16::from(value) & 0x3F) << 8);
            self.w = 1;
        } else {
            self.t = (self.t & 0xFF00) | u16::from(value);
            self.v = self.t;
            self.w = 0;
        }
    }

    fn write_data(&mut self, mapper: &mut dyn Mapper, value: u8) {
        let v = self.v;
        self.write(mapper, v, value);
        if self.flg_increment == 0 {
            self.v += 1;
        } else {
            self.v += 32;
        }
    }

    fn copy_y(&mut self) {
        let mask = 0b0111_1011_1110_0000;
        self.v = (self.v & !mask) | (self.t & mask);
    }

    fn increment_x(&mut self) {
        if self.v & 0x001F == 31 {
            self.v &= 0xFFE0;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= 0x8FFF;
            let y = match (self.v & 0x3E0) >> 5 {
                29 => {
                    self.v ^= 0x800;
                    0
                }
                31 => 0,
                val => val + 1,
            };
            self.v = (self.v & 0xFC1F) | (y << 5);
        }
    }

    fn copy_x(&mut self) {
        let mask = 0b0000_0100_0001_1111;
        self.v = (self.v & !mask) | (self.t & mask);
    }
}

/// Represents the PPU
pub(crate) struct PPU {
    cycle: i32,
    scanline: i32,

    // This need to be boxed to avoid blowing up the stack
    v_buffer: Box<PixelBuffer>,

    // Background temporary variables
    nametable_byte: u8,
    attributetable_byte: u8,
    lowtile_byte: u8,
    hightile_byte: u8,
    tiledata: u64,

    /// Even / odd frame flag (1 bit)
    f: u8,
    // Sprite temp variables
    sprite_count: i32,
    sprite_patterns: [u32; 8],
    sprite_positions: [u8; 8],
    sprite_priorities: [u8; 8],
    sprite_indices: [u8; 8], //mem: Rc<RefCell<MemoryBus>>
}

impl PPU {
    /// Creates a new PPU
    pub fn new(m: &mut MemoryBus) -> Self {
        let mut ppu = PPU {
            cycle: 0,
            scanline: 0,
            v_buffer: Box::default(),
            nametable_byte: 0,
            attributetable_byte: 0,
            lowtile_byte: 0,
            hightile_byte: 0,
            tiledata: 0,
            f: 0,
            sprite_count: 0,
            sprite_patterns: [0; 8],
            sprite_positions: [0; 8],
            sprite_priorities: [0; 8],
            sprite_indices: [0; 8],
        };
        ppu.reset(m);
        ppu
    }

    /// Resets the PPU to its initial state
    pub fn reset(&mut self, m: &mut MemoryBus) {
        self.cycle = 340;
        self.scanline = 240;
        m.ppu.write_control(0);
        m.ppu.write_mask(0);
        m.ppu.write_oam_address(0);
    }

    /// Used to clear vbuffers to make image completely neutral
    /// This isn't called in the standard reset.
    pub fn clear_vbuffers(&mut self) {
        self.v_buffer = Box::default();
    }

    fn fetch_nametable_byte(&mut self, m: &mut MemoryBus) {
        let v = m.ppu.v;
        let address = 0x2000 | (v & 0x0FFF);
        self.nametable_byte = m.ppu.read(&*m.mapper, address);
    }

    fn fetch_attributetable_byte(&mut self, m: &mut MemoryBus) {
        let v = m.ppu.v;
        let address = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
        let shift = ((v >> 4) & 4) | (v & 2);
        let read = m.ppu.read(&*m.mapper, address);
        self.attributetable_byte = ((read >> shift) & 3) << 2;
    }

    fn fetch_lowtile_byte(&mut self, m: &mut MemoryBus) {
        let fine_y = (m.ppu.v >> 12) & 7;
        let table = m.ppu.flg_backgroundtable;
        let tile = u16::from(self.nametable_byte);
        let address = 0x1000 * u16::from(table) + tile * 16 + fine_y;
        self.lowtile_byte = m.ppu.read(&*m.mapper, address);
    }

    fn fetch_hightile_byte(&mut self, m: &mut MemoryBus) {
        let fine_y = (m.ppu.v >> 12) & 7;
        let table = m.ppu.flg_backgroundtable;
        let tile = u16::from(self.nametable_byte);
        let address = 0x1000 * u16::from(table) + tile * 16 + fine_y;
        self.hightile_byte = m.ppu.read(&*m.mapper, address + 8);
    }

    fn store_tiledata(&mut self) {
        let mut data: u32 = 0;
        for _ in 0..8 {
            let a = self.attributetable_byte;
            let p1 = (self.lowtile_byte & 0x80) >> 7;
            let p2 = (self.hightile_byte & 0x80) >> 6;
            self.lowtile_byte <<= 1;
            self.hightile_byte <<= 1;
            data <<= 4;
            data |= u32::from(a | p1 | p2);
        }
        self.tiledata |= u64::from(data);
    }

    fn fetch_sprite_pattern(&self, m: &mut MemoryBus, i: usize, mut row: i32) -> u32 {
        let mut tile = m.ppu.oam.0[i * 4 + 1];
        let attributes = m.ppu.oam.0[i * 4 + 2];
        let address = if m.ppu.flg_spritesize == 0 {
            if attributes & 0x80 == 0x80 {
                row = 7 - row;
            }
            let table = m.ppu.flg_spritetable;
            0x1000 * u16::from(table) + u16::from(tile) * 16 + (row as u16)
        } else {
            if attributes & 0x80 == 0x80 {
                row = 15 - row;
            }
            let table = tile & 1;
            tile &= 0xFE;
            if row > 7 {
                tile += 1;
                row -= 8;
            }
            0x1000 * u16::from(table) + u16::from(tile) * 16 + (row as u16)
        };
        let a = (attributes & 3) << 2;
        let mut lowtile_byte = m.ppu.read(&*m.mapper, address);
        let mut hightile_byte = m.ppu.read(&*m.mapper, address + 8);
        let mut data: u32 = 0;
        for _ in 0..8 {
            let (p1, p2) = if attributes & 0x40 == 0x40 {
                let p1 = lowtile_byte & 1;
                let p2 = (hightile_byte & 1) << 1;
                lowtile_byte >>= 1;
                hightile_byte >>= 1;
                (p1, p2)
            } else {
                let p1 = (lowtile_byte & 0x80) >> 7;
                let p2 = (hightile_byte & 0x80) >> 6;
                lowtile_byte <<= 1;
                hightile_byte <<= 1;
                (p1, p2)
            };
            data <<= 4;
            data |= u32::from(a | p1 | p2);
        }
        data
    }

    fn evaluate_sprites(&mut self, m: &mut MemoryBus) {
        let h: i32 = if m.ppu.flg_spritesize == 0 { 8 } else { 16 };
        let mut count = 0;
        for i in 0..64 {
            let y = m.ppu.oam.0[i * 4];
            let a_reg = m.ppu.oam.0[i * 4 + 2];
            let x = m.ppu.oam.0[i * 4 + 3];
            let row = self.scanline - i32::from(y);
            if row < 0 || row >= h {
                continue;
            }
            if count < 8 {
                let pattern = self.fetch_sprite_pattern(m, i, row);
                self.sprite_patterns[count] = pattern;
                self.sprite_positions[count] = x;
                self.sprite_priorities[count] = (a_reg >> 5) & 1;
                self.sprite_indices[count] = i as u8;
            }
            count += 1;
        }
        if count > 8 {
            count = 8;
            m.ppu.flg_spriteoverflow = 1;
        }
        self.sprite_count = count as i32;
    }

    fn set_vblank(&mut self, m: &mut MemoryBus, video: &mut impl VideoDevice) {
        video.blit_pixels(self.v_buffer.as_ref());
        m.ppu.nmi_occurred = true;
        m.ppu.nmi_change();
    }

    fn clear_vblank(&self, m: &mut MemoryBus) {
        m.ppu.nmi_occurred = false;
        m.ppu.nmi_change();
    }

    fn fetch_tiledata(&self) -> u32 {
        (self.tiledata >> 32) as u32
    }

    fn background_pixel(&mut self, m: &mut MemoryBus) -> u8 {
        if m.ppu.flg_showbg == 0 {
            0
        } else {
            let data = self.fetch_tiledata() >> ((7 - m.ppu.x) * 4);
            (data & 0x0F) as u8
        }
    }

    fn sprite_pixel(&mut self, m: &mut MemoryBus) -> (u8, u8) {
        if m.ppu.flg_showsprites == 0 {
            (0, 0)
        } else {
            for i in 0..self.sprite_count {
                let sp_off = i32::from(self.sprite_positions[i as usize]);
                let mut offset = (self.cycle - 1) - sp_off;
                if !(0..=7).contains(&offset) {
                    continue;
                }
                offset = 7 - offset;
                let shift = (offset * 4) as u8;
                let pattern = self.sprite_patterns[i as usize];
                let color = ((pattern >> shift) & 0x0F) as u8;
                if color % 4 == 0 {
                    continue;
                }
                return (i as u8, color);
            }
            (0, 0)
        }
    }

    fn render_pixel(&mut self, m: &mut MemoryBus) {
        let x = self.cycle - 1;
        let y = self.scanline;
        let mut background = self.background_pixel(m);
        let (i, mut sprite) = self.sprite_pixel(m);
        if x < 8 && m.ppu.flg_showleftbg == 0 {
            background = 0;
        }
        if x < 8 && m.ppu.flg_showleftsprites == 0 {
            sprite = 0;
        }
        let bg = background % 4 != 0;
        let sp = sprite % 4 != 0;
        let color = match (bg, sp) {
            (false, false) => 0,
            (false, true) => sprite | 0x10,
            (true, false) => background,
            (true, true) => {
                let ind = i as usize;
                if self.sprite_indices[ind] == 0 && x < 255 {
                    m.ppu.flg_sprite0hit = 1;
                }
                if self.sprite_priorities[ind] == 0 {
                    sprite | 0x10
                } else {
                    background
                }
            }
        };
        let mut color_index = m.ppu.read_palette(u16::from(color)) % 64;
        if m.ppu.flg_grayscale != 0 {
            color_index &= 0x30;
        }
        let argb = PALETTE[color_index as usize];
        self.v_buffer.write(x as usize, y as usize, argb);
    }

    /// Steps the ppu forward
    pub fn step(&mut self, m: &mut MemoryBus, video: &mut impl VideoDevice) -> bool {
        self.tick(m);
        let rendering = m.ppu.flg_showbg != 0 || m.ppu.flg_showsprites != 0;
        let preline = self.scanline == 261;
        let visibleline = self.scanline < 240;
        let renderline = preline || visibleline;
        let prefetch_cycle = self.cycle >= 321 && self.cycle <= 336;
        let visible_cycle = self.cycle >= 1 && self.cycle <= 256;
        let fetch_cycle = prefetch_cycle || visible_cycle;

        // Background logic
        if rendering {
            if visibleline && visible_cycle {
                self.render_pixel(m)
            }
            if renderline && fetch_cycle {
                self.tiledata <<= 4;
                match self.cycle % 8 {
                    1 => self.fetch_nametable_byte(m),
                    3 => self.fetch_attributetable_byte(m),
                    5 => self.fetch_lowtile_byte(m),
                    7 => self.fetch_hightile_byte(m),
                    0 => self.store_tiledata(),
                    _ => {}
                }
            }
            if preline && self.cycle >= 280 && self.cycle <= 304 {
                m.ppu.copy_y();
            }
            if renderline {
                if fetch_cycle && self.cycle % 8 == 0 {
                    m.ppu.increment_x();
                }
                if self.cycle == 256 {
                    m.ppu.increment_y();
                }
                if self.cycle == 257 {
                    m.ppu.copy_x();
                }
            }
        }

        // Sprite logic
        if rendering && self.cycle == 257 {
            if visibleline {
                self.evaluate_sprites(m);
            } else {
                self.sprite_count = 0;
            }
        }

        let mut frame_happened = false;
        // Vblank logic
        if self.scanline == 241 && self.cycle == 1 {
            self.set_vblank(m, video);
            frame_happened = true;
        }
        if preline && self.cycle == 1 {
            self.clear_vblank(m);
            m.ppu.flg_sprite0hit = 0;
            m.ppu.flg_spriteoverflow = 0;
        }
        frame_happened
    }

    fn tick(&mut self, m: &mut MemoryBus) {
        if m.ppu.nmi_delay > 0 {
            m.ppu.nmi_delay -= 1;
            let was_nmi = m.ppu.nmi_output && m.ppu.nmi_occurred;
            if m.ppu.nmi_delay == 0 && was_nmi {
                m.cpu.set_nmi();
            }
        }
        let show_something = m.ppu.flg_showbg != 0 || m.ppu.flg_showsprites != 0;
        let should_reset = self.f == 1 && self.scanline == 261 && self.cycle == 339;
        if show_something && should_reset {
            self.cycle = 0;
            self.scanline = 0;
            self.f ^= 1;
            return;
        }

        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.f ^= 1;
            }
        }
    }
}
