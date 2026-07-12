# Clock Tower Sprite Artifact Investigation

## Purpose

This document tracks the Clock Tower for WonderSwan rendering bug so the
investigation does not keep looping over the same guesses. Treat this as the
working notebook for observations, hypotheses, experiments, and results.

Status: fixed and confirmed by the user in the GUI gameplay path on
2026-07-12. Do not reopen this issue without a new reproduction after the
line-142 next-frame sprite capture change.

Confirmed fix:

- Use StoicGoose (MIT) as the current permissive reference for this issue.
- Keep sprite-vs-sprite priority as earlier OAM entries in front of later OAM
  entries.
- Fetch sprite tile pixels live from WRAM; do not latch sprite tile bytes with
  OAM.
- Capture the next frame's sprite attributes near the end of visible rendering
  (`line == VISIBLE_SCANLINES - 2`, i.e. line 142), then promote that capture
  at the frame boundary.
- Headless image review from the user save state did not show the left/right
  split artifact in frames 96..130 or 140..220 after the line-142 OAM capture
  change.
- The user confirmed in the frontend that this fixed the Clock Tower walking
  sprite artifact.

## User-Visible Symptom

- Target ROM:
  `/Volumes/CrucialX6/roms/WonderSwan/wonderswan-japan/Clock Tower (J) [M][!].ws`
- The title screen and menu render correctly.
- Reproduction from fresh start:
  1. At the title screen, press `START`.
  2. At the menu, press Down once to select `QUICK START`.
  3. Press `A`.
  4. In the game scene, move the cursor in the walking direction and press `A`
     once. The character starts walking continuously.
- Before the fix, during walking, a one-frame or short-lived sprite artifact
  appeared.
- The artifact looked like a part of the next walking animation frame was drawn
  at the wrong horizontal position, similar to an afterimage.
- The user pointed out that choosing `GAME START` or another item instead of
  `QUICK START` can start a demo, so menu selection must be exact.
- The original black-screen issue was no longer the main symptom by the time of
  this investigation. The fixed problem was the walking sprite artifact.

## User-Provided Evidence

- `TITLE.png`: `$HOME/Desktop/TITLE.png`
- `MENU.png`: `$HOME/Desktop/MENU.png`
- Still image of the artifact:
  `/Volumes/CrucialX6/Media/スクリーンショット 2026-07-12 1.25.24.png`
- Re-pasted still image:
  `/var/folders/7f/0150_05x30qgmvgbqbwh88f80000gn/T/codex-clipboard-3ef46f85-2811-4372-bf7e-629abf5d5ed2.png`
- Video:
  `$HOME/Desktop/ClockTower.mov`
- The user confirmed that the same ROM behaves normally in
  `nesdev-org/MesenCE`.
- MesenCE is GPLv3. Do not read or copy its implementation. It may be used only
  as an external behavior reference reported by the user.
- The user also identified `$HOME/dev/_Emu/Original/StoicGoose` as a
  normal-working MIT-licensed reference. This source is allowed for comparison.

## Save State / Probe Setup

- User-created save state:
  `$HOME/Library/Application Support/swanium/states/Clock_Tower__J___M____.ws.state`
- From that state, loading and pressing `A` (`X` key in the frontend) starts the
  walking behavior and reproduces the artifact.
- The probe tool must not be deleted until the user says the issue is fixed:
  `crates/core/examples/ct_probe.rs`
- Current probe behavior:
  - Loads the Clock Tower ROM.
  - Optionally loads a save state path passed as argv[1].
  - If an `a` argument is present, presses `A` for the first 8 frames.
  - `CT_PROBE_FRAMES` controls the number of frames dumped.
  - Dumps full-frame PPM files as `/tmp/ct_probe_####.ppm`.
  - Dumps OAM information periodically and for frames around 98..106.
  - Dumps a sprite tile sheet for selected frames as
    `/tmp/ct_tiles_for_frame_####.ppm`.

Example command:

```sh
CT_PROBE_FRAMES=130 cargo run -p swanium-core --example ct_probe -- "$HOME/Library/Application Support/swanium/states/Clock_Tower__J___M____.ws.state" a
```

## Important Process Notes

- Do not claim the issue is fixed based only on unit tests.
- Do not claim image confirmation unless continuous frames around the artifact
  have been inspected.
- The artifact is easy to miss because a single normal-looking frame can appear
  between bad frames.
- The user repeatedly said "not fixed" during earlier false leads; user visual
  confirmation is the final acceptance criterion for this class of bug.
- User confirmation was received after the line-142 next-frame sprite capture
  change.
- Keep investigation artifacts until the fix is committed and no longer needed:
  - `ct_probe.rs`
  - generated frame dumps in `/tmp` are disposable, but do not remove the tool.

## Observations From Generated Frames

### Headless Probe Frames

Using the save state and `ct_probe`, a sheet was generated from frames 96..130:

- `/tmp/ct_probe_after_fix_frames_096_130.png`
- Later, after an experimental VBlank timing change:
  `/tmp/ct_probe_vblank_start_frames_096_130.png`
- After changing sprite-vs-sprite priority so later OAM entries draw in front:
  `/tmp/ct_probe_after_oam_order_096_130.png`
  `/tmp/ct_probe_after_oam_order_140_220.png`
- After changing OAM capture to line 142:
  `/tmp/ct_probe_line142_096_130.png`
  `/tmp/ct_probe_line142_140_220.png`

Observed suspicious frames in the probe output:

- Around frame 103, a thin vertical part appears above/near the character.
- Around frame 122, a similar mixed-looking pose appears.
- After the experimental VBlank timing change, the suspicious frame shifted
  later, around 105/126, rather than disappearing.

This means the symptom is not merely a subjective misread of a normal walking
pose. The current emulator can produce visibly mixed sprite frames in the probe
path.

After the later-OAM-first sampling change:

- Frames 96..130 no longer show the displaced next-motion piece.
- Frames 140..220, including the regions corresponding to the user's video
  frames around 160 and 192, also appear coherent in the generated sheet.
- This is image-based confirmation in the headless probe, not final user
  acceptance.
- The user later reported that this was still not fixed. Treat this as a false
  lead unless new evidence specifically points back to sprite-vs-sprite order.

After the line-142 OAM capture change:

- Frames 96..130 and 140..220 were regenerated.
- No obvious displaced next-motion piece was visible in the generated sheets.
- The user confirmed that the frontend gameplay path is fixed.

### User Video Frames

`ClockTower.mov` was decoded with ffmpeg into:

- `/tmp/ct_video_frames/frame_####.png`

The video is approximately:

- Resolution: 740x554
- Duration: 8.961667 seconds
- Output frames: 538 at 60 fps extraction

A cropped sheet was generated:

- `/tmp/ct_video_char_150_240.png`

Observed in the video:

- Frame 160 shows a severe mixed/split character image.
- Frame 192 shows a lower-body fragment displaced from the main character.
- These are consistent with the user's report: part of the next motion frame is
  drawn at a wrong position for a short time.

## OAM / Sprite Observations

The character is composed of many 8x8 sprites near the tail of the sprite table.

Common observed sprite range:

- Character sprites often use entries around `#112..#127`.
- Character tiles observed around `342..361`.
- Palettes commonly show `pal4`, priority bit set, window bit clear.

Examples from OAM dumps:

- Earlier pose around frames 98..101:
  - `spr_base=07` or `0b`
  - `first=0`
  - `count=128`
  - Player sprites include pairs such as:
    - `#112 t361 xy(103,127)`
    - `#113 t360 xy(95,127)`
    - `#114 t355 xy(103,119)`
    - ...
    - `#127 t342 xy(111,119)`
- Later pose around frames 102..106:
  - The active OAM base can switch between `0x07` and `0x0b`.
  - Player sprite indices and tile numbers shift by one entry in some dumps.

Important caveat:

- Existing OAM dumps are frame-level or post-frame snapshots. They do not prove
  what the PPU actually captured at line 142. A future probe should dump the
  exact captured sprite table used for rendering the bad frame.

## Current Code Changes In Working Tree

As of this document, the working tree contains investigation/fix changes in:

- `crates/core/examples/ct_probe.rs`
- `crates/core/src/bus/mod.rs`
- `crates/core/src/ppu/mod.rs`
- `crates/core/src/ppu/tests.rs`

The PPU sprite tile-latch change was removed after comparison with StoicGoose.
The OAM-order change was also reverted; StoicGoose effectively keeps earlier
OAM entries in front. The confirmed fix is line-142 next-frame sprite attribute
capture, promoted at frame end.

Do not assume all current changes should be committed together.

## Experiments And Results

### 1. Sprite OAM Latching

Existing project behavior already latched sprite attributes for the frame.

Rationale:

- Games may rewrite OAM over several CPU instructions.
- Rendering directly from live OAM can mix old and new body parts.

Result:

- OAM latching alone was already present and did not prevent the Clock Tower
  artifact.

### 2. Sprite Tile Data Latching

Change attempted and later removed in `crates/core/src/ppu/mod.rs`:

- Added `latched_sprite_tiles`.
- At sprite latch time, copied each sprite tile's bytes from WRAM.
- Sprite rendering sampled from the latched tile bytes instead of live WRAM.
- Added serde skip/default so old save states still load.

Hypothesis:

- OAM was latched but sprite tile pixels were live, so an old sprite position
  could be combined with a newly uploaded animation tile.

Regression test added:

- `sprite_tile_write_during_frame_is_visible_next_frame`

Verification:

- `cargo test -p swanium-core` passed.
- `cargo clippy -p swanium-core --all-targets -- -D warnings` passed.
- `cargo fmt --all -- --check` passed.
- Old Clock Tower save state loaded after adding serde skip/default.

Result:

- This is a real consistency improvement, but it did not eliminate the visible
  Clock Tower artifact. The user still saw the bug, and later image review also
  showed bad frames.

Status:

- Removed after StoicGoose comparison. Do not reintroduce without a hardware
  test or a specific title that proves tile bytes must be latched.

### 3. VBlank IRQ Timing Shift

Change attempted in `crates/core/src/system.rs`:

- Moved `bus.on_vblank()` to the start of line `VISIBLE_SCANLINES` before
  running that line's CPU budget, instead of after the line's CPU budget.

Hypothesis:

- VBlank IRQ was effectively one scanline late, leaving too little VBlank time
  for Clock Tower's OAM buffer update before the next frame latch.

Result:

- The suspicious mixed frame moved in time but did not disappear.
- Example: instead of a bad frame near 103/122, suspicious frames appeared
  around 105/126 in the regenerated sheet.

Status:

- This is not a sufficient fix.
- It may still be a correct timing adjustment, but it must not be committed
  solely as the Clock Tower fix without broader timing validation.

### 4. Sprite-Vs-Sprite OAM Priority Order

Change attempted in `crates/core/src/ppu/mod.rs`:

- Keep scanline sprite collection in OAM table order.
- Apply the 32-sprites-per-line limit during collection.
- When sampling the pixel, iterate the collected sprites in reverse so later OAM
  entries draw in front of earlier entries.

Regression test updated:

- Temporarily changed to `later_sprite_wins_over_earlier_sprite_when_scr2_transparent`.
- Later reverted to
  `earlier_priority_0_sprite_wins_over_later_priority_1_when_scr2_transparent`.

Rationale:

- Clock Tower builds the walking character from overlapping 8x8 sprites near
  OAM entries `#112..#127`.
- The bad frame looked like the next walking pose's body part was not hidden by
  the intended foreground body sprite.
- The previous implementation sampled the first opaque sprite in table order,
  so earlier OAM entries were always in front.

Image verification:

- Ran `CT_PROBE_FRAMES=130 cargo run -p swanium-core --example ct_probe -- "$HOME/Library/Application Support/swanium/states/Clock_Tower__J___M____.ws.state" a`
- Generated `/tmp/ct_probe_after_oam_order_096_130.png`.
- Ran `CT_PROBE_FRAMES=240 ...` and generated
  `/tmp/ct_probe_after_oam_order_140_220.png`.

Result:

- The reviewed headless frame sheets looked coherent, but the user reported the
  frontend was still not fixed.
- StoicGoose comparison also argues against this change: it copies captured
  sprites in reverse and then draws forward, which makes earlier original OAM
  entries win.

Status:

- Reverted. Do not treat later-OAM-first as the leading fix.

### 5. StoicGoose MIT Reference Comparison

The user identified `$HOME/dev/_Emu/Original/StoicGoose` as a
normal-working MIT-licensed reference.

Relevant observations:

- `DisplayControllerCommon.Step` captures `spriteDataNextFrame` when
  `lineCurrent == VerticalDisp - 2` (`144 - 2 == 142`).
- It captures from current `sprBase`, `sprFirst`, and `sprCount`.
- At frame end, it copies `spriteDataNextFrame` into `spriteData` in reverse
  order.
- Sprite rendering then scans `spriteData` forward and writes pixels to the
  framebuffer without returning on first opaque sprite.
- Net effect for overlapping sprites is still earlier original OAM entries in
  front.
- Sprite tile pixels are read live through `DisplayUtilities.ReadPixel`; tile
  bytes are not latched with OAM.

Conclusion:

- The important difference is not sprite-vs-sprite order and not tile-byte
  latching.
- The important candidate difference is when the sprite table is evaluated for
  the next frame: line 142 rather than the next frame's line 0.

### 6. Line-142 Next-Frame Sprite Capture

Change attempted in `crates/core/src/ppu/mod.rs` and
`crates/core/src/system.rs`:

- Added a `next_sprites` buffer.
- Capture next-frame sprite attributes before rendering visible line 142.
- Promote the captured buffer at frame end.
- Keep an initial `latch_sprites_if_needed` path so the first rendered frame
  has valid sprites without overwriting later captures every frame.
- Removed sprite tile byte latching; sprite tile pixels are fetched live.
- Reverted sprite-vs-sprite priority to earlier OAM entries in front.

Regression test added:

- `promoted_next_frame_sprites_use_end_of_visible_frame_capture`

Image verification:

- Ran `CT_PROBE_FRAMES=240 cargo run -p swanium-core --example ct_probe -- "$HOME/Library/Application Support/swanium/states/Clock_Tower__J___M____.ws.state" a`
- Generated `/tmp/ct_probe_line142_096_130.png`.
- Generated `/tmp/ct_probe_line142_140_220.png`.

Result:

- No obvious split/afterimage was visible in those headless sheets.
- The user confirmed that the walking sprite artifact no longer appears in the
  frontend gameplay path.

Status:

- Confirmed fix. This is the change to preserve.

### 7. Priority Bit / Sprite Window Experiments

Earlier investigation included priority-bit and sprite-window interpretation
experiments.

Known facts:

- Reversing/inverting the sprite priority bit against SCR2 did not solve the
  Clock Tower artifact.
- The current priority condition was restored to the original behavior:
  `if !sprite.priority && scr2_opaque { continue; }`
- Sprite window interpretation has compatibility implications, e.g. Golden Axe
  uses bit 12 on character sprites with the sprite window parked off-screen.

Result:

- Priority-bit/window behavior is not the leading cause of this artifact.

Status:

- Do not keep re-trying priority-bit inversion unless a new observation directly
  implicates it.

### 8. Render-Before-CPU / Line Ordering Experiments

An earlier experiment changed per-line ordering so rendering happened before
CPU execution.

Result:

- It did not resolve the symptom and was reverted.

Status:

- Not a leading hypothesis.

### 9. REP / CPU Timing Experiments

Observation:

- The video artifact looks like an OAM buffer update in progress is being used
  for a frame.
- This strongly suggests CPU/frame timing may be late relative to VBlank.

Relevant code:

- `crates/core/src/cpu/mod.rs`
  - `exec_string_op`
  - REP string operations run the full repeat in one CPU step.
  - Current base costs:
    - `MOVS`: 5 cycles per element
    - `STOS`: 3 cycles per element
    - `LODS`: 3 cycles per element
    - `SCAS`: 4 cycles per element
    - `CMPS`: 6 cycles per element
- `crates/core/src/system.rs`
  - `run_cpu_cycles`
  - Long REP instructions can overshoot scanline budgets.
  - There is an `interrupt_return_override_ip` mechanism for long REP IRQ
    restart behavior.

Earlier quick changes to REP slicing / faster REP behavior were tried and then
reverted. They did not produce a confirmed fix.

Status:

- Still a strong area to investigate, but it needs targeted instrumentation:
  record when Clock Tower writes OAM/tile data/SPR_BASE relative to VBlank and
  frame latch, rather than guessing cycle constants.

## Historical Hypotheses

These hypotheses drove the investigation. After user confirmation, the
line-142 next-frame sprite capture is the accepted explanation/fix for this
specific Clock Tower artifact.

### Hypothesis A: The Frame Latch Captured A Partially Updated OAM Buffer

Why it fits:

- The artifact resembles a mixed OAM table: some body parts from one pose, some
  from another.
- Clock Tower appears to alternate `SPR_BASE` between two OAM buffers (`0x07`
  and `0x0b` observed).
- The bad frame can move when VBlank timing changes.

What to verify:

- Log every write to:
  - I/O `0x04` (`SPR_BASE`)
  - I/O `0x05` (`SPR_FIRST`)
  - I/O `0x06` (`SPR_COUNT`)
  - OAM memory regions selected by `SPR_BASE`
  - tile bytes for tiles `342..361`
- For each frame, record:
  - current scanline
  - CPU CS:IP
  - whether currently in visible area or VBlank
  - frame number
  - old/new value
- Dump the exact latched sprite table used by PPU for the bad frame.

Result:

- Bad frame's latched table contains a mixture of old and new positions/tile
  indices, or latches the buffer before Clock Tower finishes writing it.
- The line-142 capture change fixed the user-visible artifact, consistent with
  the emulator previously evaluating sprites at the wrong point relative to
  Clock Tower's OAM updates.

### Hypothesis B: Long REP Instructions Are Crossing Scanline/VBlank Boundaries
Too Coarsely

Why it fits:

- OAM or tile updates may be performed with REP string operations.
- The emulator executes a full REP as one instruction, then bills the full
  cycle cost afterward.
- If the REP writes many bytes across a frame boundary, the emulator may make
  all writes visible too early or too late relative to PPU latching.

What to verify:

- Trace long REP instructions during the bad frames:
  - opcode
  - count
  - source/destination
  - start/end cycle within frame
  - whether destination overlaps OAM or tile memory
- Check whether a REP that should span a frame boundary is being applied
  atomically before or after the PPU latch.

Expected if true:

- A large REP to OAM/tile memory overlaps the frame boundary or VBlank boundary,
  and the current atomic execution model makes the PPU observe an impossible
  intermediate/final state.

### Hypothesis C: Sprite Latch Timing Should Be VBlank-End, Not Frame-Start

Why it fits:

- Current driver latches sprites once before line 0 of `run_frame`.
- If the hardware uses OAM evaluation at a different time, the emulator may be
  one frame early/late.

What to verify:

- Hardware documentation and behavior tests for when WonderSwan sprite table
  changes become visible.
- Public test ROMs if available.
- A minimal ROM that changes OAM at specific VBlank/line times.

Expected if true:

- Changing latch point will consistently remove the artifact without moving it
  to another frame, and regression tests can express the correct visibility
  boundary.

### Hypothesis D: Sprite Tile Data Should Not Be Latched The Same Way As OAM

Why it fits:

- The tile-latch change was plausible but did not fix the issue.
- Real hardware may fetch sprite tile data per line/pixel while OAM is latched,
  or use a more specific pipeline.

What to verify:

- Build a minimal ROM that rewrites sprite tile bytes during visible rendering
  and compare to known hardware/reference behavior.
- Avoid GPL source. Use public docs, tests, or independent behavior captures.

Expected if true:

- The added tile latch may need to be removed or refined. It should not be
  assumed correct just because it is internally consistent.

## Instrumentation Needed Next

Add targeted debug output behind an environment variable so normal behavior and
tests are unaffected.

Suggested env var:

- `SWANIUM_CT_TRACE=1`

Trace records should include:

- Frame number
- Scanline number
- CPU CS:IP
- Event type:
  - `vblank_start`
  - `sprite_latch`
  - `io_write`
  - `wram_write_oam`
  - `wram_write_tile`
  - `rep_start`
  - `rep_end`
- Relevant values:
  - `SPR_BASE`, `SPR_FIRST`, `SPR_COUNT`
  - OAM address and decoded sprite entry
  - tile index and tile row
  - REP count/source/destination

Prefer CSV or line-oriented text in `/tmp`, for example:

- `/tmp/ct_trace.log`
- `/tmp/ct_latched_sprites_frame_####.txt`

The trace should be narrow:

- Only Clock Tower investigation.
- Only OAM ranges and tiles near `342..361` unless broader evidence appears.
- Keep it out of release paths or guard it with debug/example-only code.

## Validation Checklist

The issue was called fixed only after these checks:

- [x] `ct_probe` reproduces the same gameplay path from the user's save state.
- [x] A continuous frame sheet around the previous bad ranges shows no mixed
  character frames.
- [x] The user's video/still artifact has been explicitly compared against new
  output.
- [x] The user confirms in the GUI that walking no longer produces the
  afterimage.
- [x] `cargo test -p swanium-core` passes.
- [x] `cargo clippy -p swanium-core --all-targets -- -D warnings` passes.
- [x] `cargo fmt --all -- --check` passes.

## Commands Used Recently

```sh
cargo test -p swanium-core
cargo test -p swanium-core --test system_frame sprite_tile_write_during_frame_is_visible_next_frame
cargo test -p swanium-core --test system_frame vblank_irq_is_raised_after_visible_scanlines
cargo clippy -p swanium-core --all-targets -- -D warnings
cargo fmt --all -- --check
CT_PROBE_FRAMES=130 cargo run -p swanium-core --example ct_probe -- "$HOME/Library/Application Support/swanium/states/Clock_Tower__J___M____.ws.state" a
ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames,r_frame_rate,duration,width,height -of default=nokey=1:noprint_wrappers=1 "$HOME/Desktop/ClockTower.mov"
ffmpeg -y -i "$HOME/Desktop/ClockTower.mov" -vf fps=60 /tmp/ct_video_frames/frame_%04d.png
```

## References / Constraints

- Repository architecture notes:
  - `docs/dev/Blueprint.md`
  - `docs/dev/DevelopmentPlan.md`
  - `docs/dev/Status.md`
- Public test ROM policy:
  - `tests/README.md`
- Do not use GPLv3 MesenCE source.
- User-reported MesenCE behavior is allowed as a black-box comparison point.

## Open Questions

- Does the bad frame's exact latched sprite table contain mixed entries, or is
  the table coherent and the mix occurs during tile fetch/compositing?
- Does Clock Tower use REP MOVS/STOS to fill OAM/tile buffers during VBlank?
- Are large REP writes being applied atomically across scanline/frame
  boundaries in a way that the hardware would not?
- Is sprite table latching at the correct point in the frame?
- Should sprite tile bytes be live, line-latched, frame-latched, or fetched with
  another timing model?
- Are `SPR_BASE` writes supposed to affect the current frame immediately or only
  the next sprite evaluation period?
