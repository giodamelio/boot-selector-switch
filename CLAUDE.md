# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Does

A physical rotary switch (6-position, connected to a Raspberry Pi Pico 2) selects which OS to boot. The Pico presents as a USB HID device (VID `0x6666`, PID `0xB007`). A custom UEFI application reads the switch position, sets systemd-boot's `LoaderEntryOneShot` EFI variable, then chain-loads systemd-boot.

## Build Commands

Everything builds through Nix flakes. There are no Makefiles or separate shell scripts.

```bash
nix build .#pico-firmware        # ARM ELF for Pico 2
nix build .#efi-shim             # UEFI .efi binary
nix build .#virtual-switch       # Linux USB/IP emulator
nix build .#test-esp             # FAT32 ESP disk image with test entries
nix build .#flash-pico-firmware  # Flash firmware via probe-rs over SWD

nix run .#test-vm                # QEMU with OVMF (no USB device)
nix run .#test-vm-usb            # QEMU with USB passthrough (needs sudo)
nix run .#virtual-switch-run     # Start virtual switch + attach via USB/IP

nix develop                      # Dev shell with all tools
```

Format and lint: `treefmt` (runs rustfmt, alejandra, statix).

Cargo commands work for the host-target crates: `cargo build -p virtual-switch`, `cargo check -p virtual-switch`. Cross-compilation targets (UEFI, ARM) are better built through Nix since they need special linker configuration.

## Architecture

```
UEFI Firmware → efi-shim (reads USB HID) → sets LoaderEntryOneShot → chain-loads systemd-boot
                    ↑
            USB HID device (1-byte position report, values 1-6)
                    ↑
        pico-firmware (real hardware) OR virtual-switch (USB/IP emulator)
```

**Four workspace crates:**

- **boot-selector-switch-efi-shim** — `#![no_std]` UEFI app. Discovers the switch by VID/PID, reads position from interrupt endpoint `0x81`, maps positions 1-3 to boot entries, position 6 toggles debug mode (persisted as EFI variable). Chain-loads systemd-boot.
- **pico-firmware** — `#![no_std]` Embassy-rs firmware for RP2350. 6 GPIO inputs (pins 2-7) with pull-ups scan the rotary switch. Reports position over USB HID. Uses defmt/RTT for debug logging.
- **virtual-switch** — Emulates the same USB HID device over USB/IP for development without hardware. Runs a TUI (inquire) to set position interactively.
- **test-efi-stub** — Minimal UEFI binaries that display a boot entry name and shut down. Built 3 times with different `TEST_ENTRY_TEXT` env var to produce nixos.efi, windows.efi, fedora.efi for the test ESP.

## Key Conventions

- **Nix-only build workflow** — no separate shell scripts for building/flashing.
- **Nix package names must match crate directory names** exactly.
- **HID descriptor** is duplicated in `virtual-switch/src/descriptors.rs` and `pico-firmware/src/hid_descriptor.rs` — keep them in sync. Vendor page `0xFF00`, single 8-bit input, range 1-6.
- **Crane builds** all set `cargoArtifacts = null` because UEFI/ARM linkers need entry points that dummy sources can't provide.
- **`qemu` feature flag** on efi-shim and test-efi-stub enables QEMU-specific behavior.
- **VM state** persists EFI variables in `.vm-state/OVMF_VARS.fd`.

## Cross-Compilation Targets

| Crate | Target | Notes |
|-------|--------|-------|
| boot-selector-switch-efi-shim | `x86_64-unknown-uefi` | no_std, uefi crate |
| test-efi-stub | `x86_64-unknown-uefi` | no_std, uefi crate |
| pico-firmware | `thumbv8m.main-none-eabihf` | no_std, embassy-rs, has own `.cargo/config.toml` and `memory.x` |
| virtual-switch | host (x86_64-linux) | std, tokio |
