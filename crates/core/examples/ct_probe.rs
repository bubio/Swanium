use std::fs::File;
use std::io::{BufWriter, Write};

use swanium_core::keypad::KeyState;
use swanium_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use swanium_core::system::System;

const ROM: &str = "/Volumes/CrucialX6/roms/WonderSwan/wonderswan-japan/Clock Tower (J) [M][!].ws";
const SAVE: &str =
    "/Users/seiji/Library/Application Support/swanium/saves/Clock_Tower__J___M____.ws.sav";

fn main() {
    let rom = std::fs::read(ROM).expect("read ROM");
    let mut system = System::from_rom(rom);
    if let Some(state_path) = std::env::args().nth(1) {
        let state = std::fs::read(&state_path).expect("read state");
        system.load_state_bytes(&state).expect("load state");
        dump_oam(&mut system, 0);
        write_ppm(&system, 0);
        for frame in 1usize..=180 {
            system.run_frame(KeyState::NONE);
            if frame.is_multiple_of(10) {
                dump_oam(&mut system, frame);
                write_ppm(&system, frame);
            }
        }
        return;
    }
    if let Ok(save) = std::fs::read(SAVE) {
        system.load_save_data(&save);
    }

    let mut frame = 0usize;
    let script = [
        // Logo/title need more time in headless direct boot than the earlier
        // quick probe allowed.
        (220, KeyState::NONE),
        (8, KeyState::START),
        (60, KeyState::NONE),
        (8, KeyState::X3),
        (20, KeyState::NONE),
        (8, KeyState::A),
        (240, KeyState::NONE),
        (20, KeyState::X2),
        (8, KeyState::A),
        (420, KeyState::NONE),
    ];

    for (frames, keys) in script {
        for _ in 0..frames {
            system.run_frame(keys);
            frame += 1;
            if frame.is_multiple_of(30) {
                dump_oam(&mut system, frame);
                write_ppm(&system, frame);
            }
        }
    }
}

fn dump_oam(system: &mut System, frame: usize) {
    let bus = system.bus_mut();
    let disp = bus.peek_io(0x00);
    let spr_base = bus.peek_io(0x04);
    let spr_first = bus.peek_io(0x05);
    let spr_count = bus.peek_io(0x06);
    let win = [
        bus.peek_io(0x0C),
        bus.peek_io(0x0D),
        bus.peek_io(0x0E),
        bus.peek_io(0x0F),
    ];
    let oam = ((spr_base as u32) & 0x3F) << 9;
    let mut enabled = 0usize;
    let mut window = 0usize;
    let mut priority = 0usize;
    let mut near_right = Vec::new();
    for i in 0..(spr_count as usize).min(128) {
        let idx = (spr_first as usize + i) & 127;
        let addr = oam + idx as u32 * 4;
        let lo = system.read_memory_at(addr) as u16;
        let hi = system.read_memory_at(addr + 1) as u16;
        let word = lo | (hi << 8);
        let y = system.read_memory_at(addr + 2);
        let x = system.read_memory_at(addr + 3);
        if word & 0x01ff != 0 || x != 0 || y != 0 {
            enabled += 1;
        }
        if word & (1 << 12) != 0 {
            window += 1;
        }
        if word & (1 << 13) != 0 {
            priority += 1;
        }
        if (140..=190).contains(&x) && (60..=140).contains(&y) {
            near_right.push(format!(
                "#{idx:03} t{:03} pal{} p{} w{} xy({x},{y})",
                word & 0x01ff,
                (word >> 9) & 7,
                (word >> 13) & 1,
                (word >> 12) & 1
            ));
        }
    }
    eprintln!(
        "frame={frame} disp={disp:02x} spr_base={spr_base:02x} first={spr_first} count={spr_count} win={win:?} enabled={enabled} window={window} priority={priority} near={}",
        near_right.join(" | ")
    );
}

fn write_ppm(system: &System, frame: usize) {
    let path = format!("/tmp/ct_probe_{frame:04}.ppm");
    let mut out = BufWriter::new(File::create(path).expect("create ppm"));
    writeln!(out, "P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255").expect("ppm header");
    for &rgb in system.framebuffer() {
        let r = ((rgb >> 8) & 0x0f) as u8 * 17;
        let g = ((rgb >> 4) & 0x0f) as u8 * 17;
        let b = (rgb & 0x0f) as u8 * 17;
        out.write_all(&[r, g, b]).expect("ppm pixel");
    }
}
