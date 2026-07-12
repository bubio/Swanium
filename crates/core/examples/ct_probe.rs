use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use swanium_core::keypad::KeyState;
use swanium_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
use swanium_core::system::System;

const ROM: &str = "/Volumes/CrucialX6/roms/WonderSwan/wonderswan-japan/Clock Tower (J) [M][!].ws";

fn main() {
    let rom = std::fs::read(ROM).expect("read ROM");
    let mut system = System::from_rom(rom);
    let mut args = std::env::args().skip(1);
    if let Some(state_path) = args.next() {
        let press_a = args.any(|arg| arg.eq_ignore_ascii_case("a"));
        let state = std::fs::read(&state_path).expect("read state");
        system.load_state_bytes(&state).expect("load state");
        dump_oam(&mut system, 0, "initial");
        write_ppm(&system, 0);
        let max_frames = std::env::var("CT_PROBE_FRAMES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(600);
        for frame in 1usize..=max_frames {
            let keys = if press_a && frame <= 8 {
                KeyState::A
            } else {
                KeyState::NONE
            };
            if frame.is_multiple_of(10) || (98..=106).contains(&frame) {
                dump_oam(&mut system, frame, "pre");
                if (98..=106).contains(&frame) {
                    write_sprite_debug(&system, frame, "pre");
                }
            }
            system.run_frame(keys);
            if frame.is_multiple_of(10) || (98..=106).contains(&frame) {
                dump_oam(&mut system, frame, "post");
                if (98..=106).contains(&frame) {
                    write_sprite_debug(&system, frame, "post");
                }
            }
            if (104..=106).contains(&frame) {
                write_sprite_tile_sheet(&system, frame + 1);
            }
            write_ppm(&system, frame);
        }
        return;
    }
    if let Some(save_path) = default_save_path() {
        if let Ok(save) = std::fs::read(save_path) {
            system.load_save_data(&save);
        }
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
                dump_oam(&mut system, frame, "post");
                write_ppm(&system, frame);
            }
        }
    }
}

fn default_save_path() -> Option<PathBuf> {
    let mut path = PathBuf::from(std::env::var_os("HOME")?);
    path.push("Library/Application Support/swanium/saves/Clock_Tower__J___M____.ws.sav");
    Some(path)
}

fn dump_oam(system: &mut System, frame: usize, phase: &str) {
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
    let mut near_player = Vec::new();
    let mut full_tail = Vec::new();
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
        let overlaps_player_area = (70u8..=140).any(|line| axis_delta(line, y) < 8)
            && (80u8..=155).any(|screen_x| axis_delta(screen_x, x) < 8);
        if overlaps_player_area {
            near_player.push(format!(
                "#{idx:03} t{:03} pal{} p{} w{} xy({x},{y})",
                word & 0x01ff,
                (word >> 9) & 7,
                (word >> 13) & 1,
                (word >> 12) & 1
            ));
        }
        if (100..=106).contains(&frame) && (100..=127).contains(&idx) {
            full_tail.push(format!(
                "#{idx:03} t{:03} pal{} p{} w{} hv{}{} xy({x},{y})",
                word & 0x01ff,
                (word >> 9) & 7,
                (word >> 13) & 1,
                (word >> 12) & 1,
                if word & (1 << 14) != 0 { "h" } else { "-" },
                if word & (1 << 15) != 0 { "v" } else { "-" },
            ));
        }
    }
    eprintln!(
        "frame={frame} phase={phase} disp={disp:02x} spr_base={spr_base:02x} first={spr_first} count={spr_count} win={win:?} enabled={enabled} window={window} priority={priority} near_player={}",
        near_player.join(" | ")
    );
    if !full_tail.is_empty() {
        eprintln!(
            "frame={frame} phase={phase} tail_oam={}",
            full_tail.join(" | ")
        );
    }
}

fn axis_delta(screen: u8, origin: u8) -> usize {
    screen.wrapping_sub(origin) as usize
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

fn write_sprite_tile_sheet(system: &System, frame: usize) {
    let scale = 8usize;
    let cols = 10usize;
    let rows = 2usize;
    let width = cols * 8 * scale;
    let height = rows * 8 * scale;
    let mut out = BufWriter::new(
        File::create(format!("/tmp/ct_tiles_for_frame_{frame:04}.ppm")).expect("create tile ppm"),
    );
    writeln!(out, "P6\n{width} {height}\n255").expect("ppm header");
    let mut pixels = vec![0u8; width * height * 3];
    for tile in 342usize..=361 {
        let idx = tile - 342;
        let ox = (idx % cols) * 8 * scale;
        let oy = (idx / cols) * 8 * scale;
        for ty in 0..8 {
            let addr = 0x2000 + tile * 16 + ty * 2;
            let plane0 = system.read_memory_at(addr as u32);
            let plane1 = system.read_memory_at((addr + 1) as u32);
            for tx in 0..8 {
                let bit = 7 - tx;
                let raw = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                let shade = match raw {
                    0 => 220,
                    1 => 150,
                    2 => 80,
                    _ => 20,
                };
                for sy in 0..scale {
                    for sx in 0..scale {
                        let x = ox + tx * scale + sx;
                        let y = oy + ty * scale + sy;
                        let p = (y * width + x) * 3;
                        pixels[p] = shade;
                        pixels[p + 1] = shade;
                        pixels[p + 2] = shade;
                    }
                }
            }
        }
    }
    out.write_all(&pixels).expect("write tile ppm");
}

fn write_sprite_debug(system: &System, frame: usize, phase: &str) {
    let scale = 4usize;
    let crop_x = 70usize;
    let crop_y = 55usize;
    let crop_w = 100usize;
    let crop_h = 100usize;
    let width = crop_w * scale;
    let height = crop_h * scale;
    let mut pixels = vec![0u8; width * height * 3];
    for px in pixels.chunks_exact_mut(3) {
        px.copy_from_slice(&[24, 24, 24]);
    }

    let spr_base = system.bus().peek_io(0x04);
    let spr_first = system.bus().peek_io(0x05);
    let spr_count = system.bus().peek_io(0x06);
    let oam = ((spr_base as u32) & 0x3F) << 9;
    for i in 0..(spr_count as usize).min(128) {
        let idx = (spr_first as usize + i) & 127;
        let addr = oam + idx as u32 * 4;
        let lo = system.read_memory_at(addr) as u16;
        let hi = system.read_memory_at(addr + 1) as u16;
        let word = lo | (hi << 8);
        let tile = word & 0x01ff;
        let palette = ((word >> 9) & 7) as u8;
        let hflip = word & (1 << 14) != 0;
        let vflip = word & (1 << 15) != 0;
        let y = system.read_memory_at(addr + 2);
        let x = system.read_memory_at(addr + 3);
        let color = sprite_debug_color(idx);
        for sy in 0..8usize {
            let screen_y = y.wrapping_add(sy as u8) as usize;
            if !(crop_y..crop_y + crop_h).contains(&screen_y) {
                continue;
            }
            let ty = if vflip { 7 - sy } else { sy };
            let row = 0x2000 + tile as u32 * 16 + ty as u32 * 2;
            let plane0 = system.read_memory_at(row);
            let plane1 = system.read_memory_at(row + 1);
            for sx in 0..8usize {
                let screen_x = x.wrapping_add(sx as u8) as usize;
                if !(crop_x..crop_x + crop_w).contains(&screen_x) {
                    continue;
                }
                let tx = if hflip { 7 - sx } else { sx };
                let bit = 7 - tx;
                let raw = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                if raw == 0 && palette >= 4 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let ox = (screen_x - crop_x) * scale + dx;
                        let oy = (screen_y - crop_y) * scale + dy;
                        let p = (oy * width + ox) * 3;
                        pixels[p..p + 3].copy_from_slice(&color);
                    }
                }
            }
        }
    }

    let mut out = BufWriter::new(
        File::create(format!("/tmp/ct_sprite_debug_{frame:04}_{phase}.ppm"))
            .expect("create sprite debug ppm"),
    );
    writeln!(out, "P6\n{width} {height}\n255").expect("ppm header");
    out.write_all(&pixels).expect("write sprite debug ppm");
}

fn sprite_debug_color(idx: usize) -> [u8; 3] {
    const COLORS: [[u8; 3]; 16] = [
        [230, 64, 64],
        [64, 180, 255],
        [255, 200, 64],
        [96, 220, 96],
        [220, 96, 255],
        [255, 128, 64],
        [64, 240, 220],
        [240, 240, 240],
        [160, 64, 64],
        [64, 120, 180],
        [180, 140, 64],
        [64, 160, 64],
        [160, 64, 180],
        [180, 96, 64],
        [64, 180, 160],
        [180, 180, 180],
    ];
    COLORS[idx & 15]
}
