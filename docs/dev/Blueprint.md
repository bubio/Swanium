# Swanium

> A modern, cross-platform WonderSwan / WonderSwan Color emulator
> written in Rust.

## Vision

Swanium is a modern emulator for WonderSwan and WonderSwan Color.

The project is intended not only to emulate the hardware accurately, but
also to serve as a learning project for modern Rust application
architecture.

### Goals

-   Cross-platform support
    -   Windows
    -   macOS
    -   Linux
-   Modern Rust codebase
-   Clean separation between emulator core and frontend
-   Easy to maintain and extend
-   Fast startup and low latency
-   Future support for debugging tools and developer features

------------------------------------------------------------------------

# Technology Stack

  Component   Library

----------- -----------------

  Language    Rust (Stable)
  GUI         Slint
  Graphics    wgpu
  Audio       cpal
  Gamepad     gilrs
  Build       Cargo Workspace

------------------------------------------------------------------------

# Architecture

    +--------------------+       +-------------+
    |      Slint GUI     |<------| Input/gilrs |
    | commands + display |       +-------------+
    +---------+----------+
              | input/commands       latest framebuffer
              v                              ^
    +---------+------------------------------+--+
    |       Frontend emulation/audio worker     |
    | real-time + audio-ring pacing             |
    +---------+-------------------------+--------+
              |                         |
              v                         v
    +----------------------------+   +------------------+
    |      Emulator Core         |   | Audio producer   |
    |----------------------------|   | resampler + ring |
    | CPU / Memory / PPU / APU   |   +--------+---------+
    | Cartridge / RTC            |            |
    +----------------------------+            v
                                      +-------+-------+
                                      | cpal callback |
                                      +---------------+

The Slint event loop never owns or advances mutable emulator state. A dedicated
frontend worker owns `System` and the producer half of the audio path, while the
GUI publishes atomic input snapshots and displays the newest completed frame.
The cpal stream/callback lifetime remains on the platform thread and consumes a
shared ring buffer. Slow or temporarily blocked GUI rendering can therefore
skip visual frames without starving audio generation.

The emulator core should have no dependency on GUI or host-audio libraries.

------------------------------------------------------------------------

# Cargo Workspace Layout

``` text
swanium/
├── Cargo.toml
├── rust-toolchain.toml
├── README.md
├── LICENSE
├── crates/
│   ├── core/
│   ├── frontend/
│   ├── audio/
│   ├── video/
│   ├── input/
│   └── common/
├── assets/
│   ├── icons/
│   ├── fonts/
│   └── shaders/
├── docs/
└── tests/
```

## Responsibilities

### core

-   CPU emulation
-   Memory map
-   Interrupts
-   Timers
-   Video
-   Audio generation
-   Cartridge
-   Save RAM

### frontend

-   Slint UI
-   Emulation/audio worker and command/snapshot boundary
-   Menus
-   Settings
-   ROM management
-   Save states
-   Debug windows

### audio

-   cpal backend
-   Ring buffer
-   Audio synchronization

### video

-   wgpu rendering
-   Scaling
-   Filters
-   Future shader support

### input

-   Keyboard
-   gilrs gamepad support

### common

-   Shared utilities
-   Configuration
-   Logging

------------------------------------------------------------------------

# Future Ideas

-   Save States
-   Rewind
-   Fast Forward
-   Shader support
-   LCD simulation
-   Screenshot
-   Video recording
-   Audio recording
-   Cheat support
-   Debugger
-   Memory Viewer
-   Disassembler

------------------------------------------------------------------------

# Development Principles

-   Prefer stable Rust.
-   Keep the emulator core platform-independent.
-   Minimize unsafe code.
-   Write clear, maintainable code.
-   Test each subsystem independently.

------------------------------------------------------------------------

# Project Name

**Swanium**

The name is inspired by **WonderSwan**, combined with the software-style
suffix **-ium** to create a unique and memorable project name.
