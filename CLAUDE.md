# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Does

A physical rotary switch (6-position, connected to a Raspberry Pi Pico 2) selects which OS to boot. The Pico presents as a USB HID device (VID `0x6666`, PID `0xB007`). A custom UEFI application reads the switch position, sets systemd-boot's `LoaderEntryOneShot` EFI variable, then chain-loads systemd-boot.

## Build Commands

Everything builds through Nix flakes. There are no Makefiles or separate shell scripts.

```bash
nix build .#pico-firmware        # ARM ELF for Pico 2
nix build .#efi-shim             # UEFI .efi binary (production, no qemu)
nix build .#efi-shim-qemu        # UEFI .efi binary (with uefi/qemu feature)
nix build .#virtual-switch       # Linux USB/IP emulator
nix build .#flash-pico-firmware  # Flash firmware via probe-rs over SWD

nix run .#test-vm                # NixOS QEMU VM with OVMF (no USB device)
nix run .#test-vm-usb            # NixOS QEMU VM with USB passthrough (needs sudo)
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

**Position mapping (hardcoded in efi-shim):**

| Position | Entry | Description |
|----------|-------|-------------|
| 1 | nixos.conf | NixOS (default) |
| 2 | windows.conf | Windows |
| 3 | netbootxyz.conf | netboot.xyz |
| 6 | — | Toggle debug mode |

**Four workspace crates:**

- **boot-selector-switch-efi-shim** — `#![no_std]` UEFI app. Discovers the switch by VID/PID, reads position from interrupt endpoint `0x81`, maps positions 1-3 to boot entries, position 6 toggles debug mode (persisted as EFI variable). Chain-loads systemd-boot.
- **pico-firmware** — `#![no_std]` Embassy-rs firmware for RP2350. 6 GPIO inputs (pins 2-7) with pull-ups scan the rotary switch. Reports position over USB HID. Uses defmt/RTT for debug logging.
- **virtual-switch** — Emulates the same USB HID device over USB/IP for development without hardware. Runs a TUI (inquire) to set position interactively.
- **test-efi-stub** — Minimal UEFI binary that displays a boot entry name and shuts down. Built with `TEST_ENTRY_TEXT` env var for the Windows test entry in the VM.

**NixOS module** (`nixos-module/default.nix`):

Exposed as `nixosModules.default` and `nixosModules.boot-selector-switch` from the flake. Installs the efi-shim to the ESP as `EFI/BOOT/BOOTX64.EFI` via systemd-boot's `extraFiles`, overwriting systemd-boot's fallback. Options under `boot.loader.boot-selector-switch`:
- `enable` — enable the module
- `package` — the efi-shim package to install
- `positionMap` — documentation-only attrset mapping positions to entry filenames (will drive config file generation in the future)

**Test VM** (`nixos-module/test-vm.nix`):

A real NixOS system with systemd-boot, the boot-selector-switch module, a Windows test stub, and netboot.xyz. Uses the NixOS `qemu-vm.nix` virtualisation module.

## Key Conventions

- **Nix-only build workflow** — no separate shell scripts for building/flashing.
- **Nix package names must match crate directory names** exactly.
- **HID descriptor** is duplicated in `virtual-switch/src/descriptors.rs` and `pico-firmware/src/hid_descriptor.rs` — keep them in sync. Vendor page `0xFF00`, single 8-bit input, range 1-6.
- **Crane builds** all set `cargoArtifacts = null` because UEFI/ARM linkers need entry points that dummy sources can't provide.
- **`qemu` feature flag** on efi-shim and test-efi-stub enables QEMU-specific behavior. Production efi-shim (`efi-shim`) omits it; test VM uses `efi-shim-qemu`.
- **VM state** persists EFI variables in `.vm-state/` (managed by NixOS qemu-vm module).

## Cross-Compilation Targets

| Crate | Target | Notes |
|-------|--------|-------|
| boot-selector-switch-efi-shim | `x86_64-unknown-uefi` | no_std, uefi crate |
| test-efi-stub | `x86_64-unknown-uefi` | no_std, uefi crate |
| pico-firmware | `thumbv8m.main-none-eabihf` | no_std, embassy-rs, has own `.cargo/config.toml` and `memory.x` |
| virtual-switch | host (x86_64-linux) | std, tokio |
