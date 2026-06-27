use super::{
    tile_pixel, DisplayControl, MonoPaletteResolver, PaletteResolver, Ppu, SpriteEntry,
    TileMapEntry, FRAMEBUFFER_LEN, SCREEN_HEIGHT, SCREEN_WIDTH,
};

/// Configure palette 0 and the shade pool as an identity map, so a tile pixel
/// `i` resolves to shade `i`. Lets background-rendering tests assert raw pixel
/// values through the resolver.
fn set_identity_palette(ports: &mut [u8]) {
    ports[0x20] = 0x10; // palette 0: color0=0, color1=1
    ports[0x21] = 0x32; //            color2=2, color3=3
    ports[0x1C] = 0x10; // shade pool: pool0=0, pool1=1
    ports[0x1D] = 0x32; //             pool2=2, pool3=3
}

// ── Framebuffer ──────────────────────────────────────────────────────────────

#[test]
fn new_ppu_framebuffer_has_screen_pixel_count() {
    let ppu = Ppu::new();
    assert_eq!(ppu.framebuffer().len(), SCREEN_WIDTH * SCREEN_HEIGHT);
}

#[test]
fn new_ppu_framebuffer_length_matches_constant() {
    let ppu = Ppu::new();
    assert_eq!(ppu.framebuffer().len(), FRAMEBUFFER_LEN);
}

#[test]
fn new_ppu_framebuffer_is_cleared() {
    let ppu = Ppu::new();
    assert!(ppu.framebuffer().iter().all(|&p| p == 0));
}

#[test]
fn new_ppu_starts_at_line_zero() {
    let ppu = Ppu::new();
    assert_eq!(ppu.current_line(), 0);
}

#[test]
fn reset_clears_framebuffer() {
    let mut ppu = Ppu::new();
    ppu.framebuffer[0] = 0x0F;
    ppu.reset();
    assert!(ppu.framebuffer().iter().all(|&p| p == 0));
}

#[test]
fn reset_returns_current_line_to_zero() {
    let mut ppu = Ppu::new();
    ppu.current_line = 100;
    ppu.reset();
    assert_eq!(ppu.current_line(), 0);
}

#[test]
fn default_ppu_matches_new() {
    assert_eq!(
        Ppu::default().framebuffer().len(),
        Ppu::new().framebuffer().len()
    );
}

// ── DisplayControl ───────────────────────────────────────────────────────────

fn ports_with_disp_ctrl(value: u8) -> [u8; 0x100] {
    let mut ports = [0u8; 0x100];
    ports[0x00] = value;
    ports
}

#[test]
fn display_control_decodes_scr1_enable_bit() {
    let ports = ports_with_disp_ctrl(0b0000_0001);
    assert!(DisplayControl::from_ports(&ports).scr1_enabled);
}

#[test]
fn display_control_decodes_scr2_enable_bit() {
    let ports = ports_with_disp_ctrl(0b0000_0010);
    assert!(DisplayControl::from_ports(&ports).scr2_enabled);
}

#[test]
fn display_control_decodes_sprite_enable_bit() {
    let ports = ports_with_disp_ctrl(0b0000_0100);
    assert!(DisplayControl::from_ports(&ports).sprites_enabled);
}

#[test]
fn display_control_scr1_disabled_when_bit_clear() {
    let ports = ports_with_disp_ctrl(0b0000_0110);
    assert!(!DisplayControl::from_ports(&ports).scr1_enabled);
}

#[test]
fn display_control_all_layers_disabled_at_zero() {
    let ports = ports_with_disp_ctrl(0x00);
    let dc = DisplayControl::from_ports(&ports);
    assert_eq!(
        dc,
        DisplayControl {
            scr1_enabled: false,
            scr2_enabled: false,
            sprites_enabled: false,
            sprite_window_enabled: false,
            scr2_window_enabled: false,
            scr2_window_outside: false,
        }
    );
}

// ── Tile-map entry decode ────────────────────────────────────────────────────

#[test]
fn tilemap_entry_decodes_tile_index() {
    assert_eq!(TileMapEntry::decode(0x0105).tile_idx, 0x0105);
}

#[test]
fn tilemap_entry_masks_tile_index_to_9_bits() {
    // bits above 8 belong to palette/flip fields, not the tile index
    assert_eq!(TileMapEntry::decode(0xFFFF).tile_idx, 0x01FF);
}

#[test]
fn tilemap_entry_decodes_palette() {
    assert_eq!(TileMapEntry::decode(0x0F << 9).palette, 0x0F);
}

#[test]
fn tilemap_entry_decodes_horizontal_flip() {
    assert!(TileMapEntry::decode(1 << 14).hflip);
}

#[test]
fn tilemap_entry_decodes_vertical_flip() {
    assert!(TileMapEntry::decode(1 << 15).vflip);
}

#[test]
fn tilemap_entry_no_flips_when_clear() {
    let e = TileMapEntry::decode(0x0000);
    assert!(!e.hflip && !e.vflip);
}

// ── Tile pixel decode ────────────────────────────────────────────────────────

fn write_tile_row(wram: &mut [u8], tile_idx: usize, row: usize, plane0: u8, plane1: u8) {
    let addr = 0x2000 + tile_idx * 16 + row * 2;
    wram[addr] = plane0;
    wram[addr + 1] = plane1;
}

#[test]
fn tile_pixel_reads_low_plane_bit() {
    let mut wram = vec![0u8; 0x10000];
    // leftmost pixel (x=0) set in plane 0 only → value 1
    write_tile_row(&mut wram, 0, 0, 0b1000_0000, 0b0000_0000);
    assert_eq!(tile_pixel(&wram, 0, 0, 0), 1);
}

#[test]
fn tile_pixel_reads_high_plane_bit() {
    let mut wram = vec![0u8; 0x10000];
    // leftmost pixel set in plane 1 only → value 2
    write_tile_row(&mut wram, 0, 0, 0b0000_0000, 0b1000_0000);
    assert_eq!(tile_pixel(&wram, 0, 0, 0), 2);
}

#[test]
fn tile_pixel_combines_both_planes() {
    let mut wram = vec![0u8; 0x10000];
    write_tile_row(&mut wram, 0, 0, 0b1000_0000, 0b1000_0000);
    assert_eq!(tile_pixel(&wram, 0, 0, 0), 3);
}

#[test]
fn tile_pixel_msb_is_leftmost() {
    let mut wram = vec![0u8; 0x10000];
    // only x=7 (LSB) set
    write_tile_row(&mut wram, 0, 0, 0b0000_0001, 0b0000_0000);
    assert_eq!(tile_pixel(&wram, 0, 7, 0), 1);
}

#[test]
fn tile_pixel_zero_when_planes_clear() {
    let wram = vec![0u8; 0x10000];
    assert_eq!(tile_pixel(&wram, 0, 3, 5), 0);
}

#[test]
fn tile_pixel_respects_row_offset() {
    let mut wram = vec![0u8; 0x10000];
    write_tile_row(&mut wram, 0, 3, 0b1000_0000, 0b1000_0000);
    assert_eq!(tile_pixel(&wram, 0, 0, 3), 3);
}

// ── Background layer rendering ────────────────────────────────────────────────

fn write_map_entry(wram: &mut [u8], base: usize, col: usize, row: usize, word: u16) {
    let addr = base + (row * 32 + col) * 2;
    let [lo, hi] = word.to_le_bytes();
    wram[addr] = lo;
    wram[addr + 1] = hi;
}

/// Set up a single SCR1 tile at map (0,0) using tile index `tile_idx`, with
/// SCR1 enabled and map base 0. Returns (wram, ports).
fn setup_scr1_single_tile(tile_idx: u16, row0: (u8, u8)) -> (Vec<u8>, [u8; 0x100]) {
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x01; // SCR1 enable
    ports[0x07] = 0x00; // SCR1 map base nibble = 0 → base 0
    set_identity_palette(&mut ports);
    write_map_entry(&mut wram, 0, 0, 0, tile_idx);
    write_tile_row(&mut wram, tile_idx as usize, 0, row0.0, row0.1);
    (wram, ports)
}

#[test]
fn scr1_renders_tile_pixel_at_origin() {
    let (wram, ports) = setup_scr1_single_tile(0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn scr1_renders_second_pixel_of_tile() {
    let (wram, ports) = setup_scr1_single_tile(0, (0b0100_0000, 0b0100_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[1], 3);
}

#[test]
fn scr1_disabled_leaves_framebuffer_zero() {
    let (wram, mut ports) = setup_scr1_single_tile(0, (0b1000_0000, 0b0000_0000));
    ports[0x00] = 0x00; // all layers disabled
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 0);
}

#[test]
fn scr1_scroll_x_shifts_sampled_column() {
    // Place a distinct tile at map column 1; scroll right by 8 so screen x=0
    // samples column 1.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x01;
    ports[0x07] = 0x00;
    ports[0x10] = 8; // SCR1 scroll X = 8 (one tile)
    set_identity_palette(&mut ports);
    write_map_entry(&mut wram, 0, 1, 0, 5); // base 0, col 1 → tile 5
    write_tile_row(&mut wram, 5, 0, 0b1000_0000, 0b1000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 3);
}

#[test]
fn scr1_horizontal_flip_mirrors_tile() {
    // Tile has pixel only at x=0; with hflip it should appear at x=7.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x01;
    ports[0x07] = 0x00;
    set_identity_palette(&mut ports);
    write_map_entry(&mut wram, 0, 0, 0, 1 << 14); // hflip, tile 0
    write_tile_row(&mut wram, 0, 0, 0b1000_0000, 0b0000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[7], 1);
}

#[test]
fn scr2_transparent_pixel_lets_scr1_show_through() {
    // SCR1 and SCR2 use separate map bases (nibbles 0 and 1). SCR1 draws a
    // pixel of value 1; SCR2's pixel is 0 (transparent), so SCR1 shows through.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x03; // SCR1 + SCR2 enabled
    ports[0x07] = 0x01 << 4; // SCR1 base nibble 0 → 0x000; SCR2 nibble 1 → 0x800
    set_identity_palette(&mut ports);
    write_map_entry(&mut wram, 0, 0, 0, 1); // SCR1 (base 0) → tile 1
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000); // tile1 x0 = 1
    write_map_entry(&mut wram, 0x800, 0, 0, 0); // SCR2 (base 0x800) → tile 0 (zero)
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn scr2_opaque_pixel_overrides_scr1() {
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x03; // SCR1 + SCR2
    ports[0x07] = 0x01 << 4; // SCR2 base nibble 1 → 0x800; SCR1 nibble 0
    set_identity_palette(&mut ports);
    write_map_entry(&mut wram, 0, 0, 0, 1); // SCR1 → tile 1
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000); // SCR1 x0 = 1
    write_map_entry(&mut wram, 0x800, 0, 0, 2); // SCR2 → tile 2
    write_tile_row(&mut wram, 2, 0, 0b1000_0000, 0b1000_0000); // SCR2 x0 = 3
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 3);
}

#[test]
fn render_scanline_ignores_out_of_range_line() {
    let (wram, ports) = setup_scr1_single_tile(0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(SCREEN_HEIGHT as u8, &wram, &ports, &MonoPaletteResolver);
    // Nothing drawn; framebuffer stays cleared.
    assert!(ppu.framebuffer().iter().all(|&p| p == 0));
}

#[test]
fn render_scanline_updates_current_line() {
    let (wram, ports) = setup_scr1_single_tile(0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(5, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.current_line(), 5);
}

// ── Monochrome palette resolution ─────────────────────────────────────────────

#[test]
fn mono_resolver_maps_pixel_through_palette_and_pool() {
    // palette 0, color for pixel 1 = pool index 5; pool index 5 = shade 0x0A.
    let mut ports = [0u8; 0x100];
    ports[0x20] = 0x50; // pixel0 → pool0, pixel1 → pool5 (high nibble of low byte)
    ports[0x1E] = 0xA0; // pool4 = 0, pool5 = 0x0A (high nibble of byte 0x1E)
    assert_eq!(MonoPaletteResolver.resolve(&ports, 0, 1), 0x0A);
}

#[test]
fn mono_resolver_uses_high_byte_for_pixel_2_and_3() {
    // pixel 2 reads the low nibble of the palette's high byte (port 0x21).
    let mut ports = [0u8; 0x100];
    ports[0x21] = 0x03; // pixel2 → pool3
    ports[0x1D] = 0xC0; // pool2 = 0, pool3 = 0x0C
    assert_eq!(MonoPaletteResolver.resolve(&ports, 0, 2), 0x0C);
}

#[test]
fn mono_resolver_selects_palette_by_index() {
    // palette 1 lives at port 0x22; pixel 0 → pool index 7 → shade 0x0F.
    let mut ports = [0u8; 0x100];
    ports[0x22] = 0x07; // palette 1, pixel0 → pool7
    ports[0x1F] = 0xF0; // pool6 = 0, pool7 = 0x0F
    assert_eq!(MonoPaletteResolver.resolve(&ports, 1, 0), 0x0F);
}

#[test]
fn mono_resolver_returns_zero_shade_for_zeroed_registers() {
    let ports = [0u8; 0x100];
    assert_eq!(MonoPaletteResolver.resolve(&ports, 3, 2), 0);
}

#[test]
fn scr1_pixel_resolves_through_nonidentity_palette() {
    // A tile pixel of 1 under a palette mapping pixel1 → pool2 → shade 0x07.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x01; // SCR1 enable
    ports[0x07] = 0x00;
    ports[0x20] = 0x20; // pixel0 → pool0, pixel1 → pool2
    ports[0x1D] = 0x07; // pool2 = 0x07
    write_map_entry(&mut wram, 0, 0, 0, 0); // tile 0, palette 0
    write_tile_row(&mut wram, 0, 0, 0b1000_0000, 0b0000_0000); // x0 pixel = 1
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 0x07);
}

// ── Sprite attribute-entry decode ─────────────────────────────────────────────

#[test]
fn sprite_entry_decodes_tile_index() {
    let mut wram = vec![0u8; 0x10000];
    wram[0] = 0x05;
    wram[1] = 0x01; // word = 0x0105
    assert_eq!(SpriteEntry::decode(&wram, 0).tile_idx, 0x0105);
}

#[test]
fn sprite_entry_decodes_palette() {
    let mut wram = vec![0u8; 0x10000];
    let word: u16 = 0x07 << 9;
    wram[0] = word as u8;
    wram[1] = (word >> 8) as u8;
    assert_eq!(SpriteEntry::decode(&wram, 0).palette, 0x07);
}

#[test]
fn sprite_entry_decodes_priority_bit() {
    let mut wram = vec![0u8; 0x10000];
    let word: u16 = 1 << 13;
    wram[0] = word as u8;
    wram[1] = (word >> 8) as u8;
    assert!(SpriteEntry::decode(&wram, 0).priority);
}

#[test]
fn sprite_entry_decodes_coordinates() {
    let mut wram = vec![0u8; 0x10000];
    wram[2] = 30; // Y
    wram[3] = 50; // X
    let s = SpriteEntry::decode(&wram, 0);
    assert_eq!((s.x, s.y), (50, 30));
}

// ── Sprite rendering ──────────────────────────────────────────────────────────

fn write_sprite(wram: &mut [u8], oam_base: usize, idx: usize, word: u16, y: u8, x: u8) {
    let addr = oam_base + idx * 4;
    let [lo, hi] = word.to_le_bytes();
    wram[addr] = lo;
    wram[addr + 1] = hi;
    wram[addr + 2] = y;
    wram[addr + 3] = x;
}

/// Identity palette for sprite palette 0 (palette index 8 → port 0x30).
fn set_identity_sprite_palette(ports: &mut [u8]) {
    ports[0x30] = 0x10; // color0=0, color1=1
    ports[0x31] = 0x32; // color2=2, color3=3
    ports[0x1C] = 0x10; // pool0=0, pool1=1
    ports[0x1D] = 0x32; // pool2=2, pool3=3
}

/// One sprite (index 0) at OAM base 0x200, SPR enabled, identity palette.
fn setup_single_sprite(word: u16, y: u8, x: u8, row0: (u8, u8)) -> (Vec<u8>, [u8; 0x100]) {
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x04; // SPR enable only
    ports[0x04] = 0x01; // OAM base = 1 << 9 = 0x200
    ports[0x05] = 0; // first sprite 0
    ports[0x06] = 1; // process 1 sprite
    set_identity_sprite_palette(&mut ports);
    let tile_idx = (word & 0x1FF) as usize;
    write_sprite(&mut wram, 0x200, 0, word, y, x);
    write_tile_row(&mut wram, tile_idx, 0, row0.0, row0.1);
    (wram, ports)
}

#[test]
fn sprite_renders_pixel_at_its_position() {
    // sprite tile 1, x=0 y=0, pixel at tile x0 = 1
    let (wram, ports) = setup_single_sprite(1, 0, 0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn sprite_offset_by_x_position() {
    let (wram, ports) = setup_single_sprite(1, 0, 10, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[10], 1);
}

#[test]
fn sprite_transparent_pixel_not_drawn() {
    // tile row all zero → transparent everywhere
    let (wram, ports) = setup_single_sprite(1, 0, 0, (0b0000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 0);
}

#[test]
fn sprite_not_drawn_on_scanline_above_it() {
    let (wram, ports) = setup_single_sprite(1, 8, 0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver); // line 0, sprite at y=8
    assert_eq!(ppu.framebuffer()[0], 0);
}

#[test]
fn sprite_horizontal_flip_mirrors_within_cell() {
    // pixel only at tile x=0; with hflip it lands at screen x=7
    let (wram, ports) = setup_single_sprite(1 | (1 << 14), 0, 0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[7], 1);
}

#[test]
fn sprite_vertical_flip_mirrors_rows() {
    // pixel only on tile row 0; with vflip, line 7 of the sprite shows it.
    let (wram, ports) = setup_single_sprite(1 | (1 << 15), 0, 0, (0b1000_0000, 0b0000_0000));
    let mut ppu = Ppu::new();
    ppu.render_scanline(7, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[7 * SCREEN_WIDTH], 1);
}

#[test]
fn sprite_priority_1_draws_over_scr2() {
    // SCR2 opaque pixel 3; sprite priority 1 (front) pixel 1 → sprite wins.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x06; // SCR2 + SPR
    ports[0x07] = 0x00; // SCR2 base nibble (high) = 0 → 0x000
    ports[0x04] = 0x01; // OAM base 0x200
    ports[0x05] = 0;
    ports[0x06] = 1;
    set_identity_palette(&mut ports);
    set_identity_sprite_palette(&mut ports);
    // SCR2 tile at (0,0) → tile 5, pixel 3
    write_map_entry(&mut wram, 0, 0, 0, 5);
    write_tile_row(&mut wram, 5, 0, 0b1000_0000, 0b1000_0000);
    // sprite priority 1, tile 1, pixel 1
    write_sprite(&mut wram, 0x200, 0, 1 | (1 << 13), 0, 0);
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn sprite_priority_0_drawn_behind_scr2() {
    // SCR2 opaque pixel 3; sprite priority 0 (behind) → SCR2 wins.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x06; // SCR2 + SPR
    ports[0x07] = 0x00;
    ports[0x04] = 0x01;
    ports[0x05] = 0;
    ports[0x06] = 1;
    set_identity_palette(&mut ports);
    set_identity_sprite_palette(&mut ports);
    write_map_entry(&mut wram, 0, 0, 0, 5);
    write_tile_row(&mut wram, 5, 0, 0b1000_0000, 0b1000_0000); // SCR2 pixel 3
    write_sprite(&mut wram, 0x200, 0, 1, 0, 0); // priority 0
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000); // sprite pixel 1
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 3);
}

// ── Window masking (4e) ───────────────────────────────────────────────────────

#[test]
fn display_control_decodes_sprite_window_enable() {
    let ports = ports_with_disp_ctrl(1 << 3);
    assert!(DisplayControl::from_ports(&ports).sprite_window_enabled);
}

#[test]
fn display_control_decodes_scr2_window_outside() {
    let ports = ports_with_disp_ctrl(1 << 4);
    assert!(DisplayControl::from_ports(&ports).scr2_window_outside);
}

#[test]
fn display_control_decodes_scr2_window_enable() {
    let ports = ports_with_disp_ctrl(1 << 5);
    assert!(DisplayControl::from_ports(&ports).scr2_window_enabled);
}

/// SCR2 covering the whole screen with an inside-window restricted to x∈[4,7].
fn setup_scr2_windowed(outside: bool) -> (Vec<u8>, [u8; 0x100]) {
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    let mut ctrl = 0x02 | (1 << 5); // SCR2 enable + SCR2 window enable
    if outside {
        ctrl |= 1 << 4;
    }
    ports[0x00] = ctrl;
    ports[0x07] = 0x00; // SCR2 base nibble (high) = 0 → 0x000
    set_identity_palette(&mut ports);
    // window x∈[4,7], y∈[0,143]
    ports[0x08] = 4; // X1
    ports[0x09] = 0; // Y1
    ports[0x0A] = 7; // X2
    ports[0x0B] = 143; // Y2
                       // SCR2 tile 5: full row of pixel 3
    write_map_entry(&mut wram, 0, 0, 0, 5);
    write_tile_row(&mut wram, 5, 0, 0b1111_1111, 0b1111_1111);
    (wram, ports)
}

#[test]
fn scr2_inside_window_shows_pixel_within_bounds() {
    let (wram, ports) = setup_scr2_windowed(false);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[5], 3); // x=5 is inside [4,7]
}

#[test]
fn scr2_inside_window_hides_pixel_outside_bounds() {
    let (wram, ports) = setup_scr2_windowed(false);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 0); // x=0 is outside [4,7]
}

#[test]
fn scr2_outside_window_shows_pixel_beyond_bounds() {
    let (wram, ports) = setup_scr2_windowed(true);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 3); // x=0 is outside → shown in outside mode
}

#[test]
fn scr2_outside_window_hides_pixel_within_bounds() {
    let (wram, ports) = setup_scr2_windowed(true);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[5], 0); // x=5 is inside → hidden in outside mode
}

#[test]
fn windowed_sprite_hidden_outside_sprite_window() {
    // Sprite with window attribute (bit12); sprite window enabled, window at
    // x∈[100,107]. The sprite at x=0 should be hidden.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x04 | (1 << 3); // SPR enable + sprite window enable
    ports[0x04] = 0x01; // OAM base 0x200
    ports[0x05] = 0;
    ports[0x06] = 1;
    set_identity_sprite_palette(&mut ports);
    ports[0x0C] = 100; // window X1
    ports[0x0D] = 0; // Y1
    ports[0x0E] = 107; // X2
    ports[0x0F] = 143; // Y2
    write_sprite(&mut wram, 0x200, 0, 1 | (1 << 12), 0, 0); // window attr, x=0
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 0);
}

#[test]
fn windowed_sprite_shown_inside_sprite_window() {
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x04 | (1 << 3); // SPR enable + sprite window enable
    ports[0x04] = 0x01;
    ports[0x05] = 0;
    ports[0x06] = 1;
    set_identity_sprite_palette(&mut ports);
    ports[0x0C] = 0; // window X1
    ports[0x0D] = 0;
    ports[0x0E] = 7; // X2
    ports[0x0F] = 143;
    write_sprite(&mut wram, 0x200, 0, 1 | (1 << 12), 0, 0); // window attr, x=0 inside
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn non_windowed_sprite_unaffected_by_sprite_window() {
    // Sprite WITHOUT window attribute should ignore the sprite window.
    let mut wram = vec![0u8; 0x10000];
    let mut ports = [0u8; 0x100];
    ports[0x00] = 0x04 | (1 << 3); // SPR enable + sprite window enable
    ports[0x04] = 0x01;
    ports[0x05] = 0;
    ports[0x06] = 1;
    set_identity_sprite_palette(&mut ports);
    ports[0x0C] = 100; // window far from sprite
    ports[0x0E] = 107;
    ports[0x0F] = 143;
    write_sprite(&mut wram, 0x200, 0, 1, 0, 0); // no window attr, x=0
    write_tile_row(&mut wram, 1, 0, 0b1000_0000, 0b0000_0000);
    let mut ppu = Ppu::new();
    ppu.render_scanline(0, &wram, &ports, &MonoPaletteResolver);
    assert_eq!(ppu.framebuffer()[0], 1);
}

#[test]
fn sprite_entry_decodes_window_attribute() {
    let mut wram = vec![0u8; 0x10000];
    let word: u16 = 1 << 12;
    wram[0] = word as u8;
    wram[1] = (word >> 8) as u8;
    assert!(SpriteEntry::decode(&wram, 0).window);
}
