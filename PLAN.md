# Boot Switch: Physical OS Selector for systemd-boot

## Project Overview

A physical switch mounted on a PC case that selects which OS to boot, implemented as a custom EFI application that runs before systemd-boot. The switch connects to a Raspberry Pi Pico 2 microcontroller, which presents as a USB HID device. A small EFI shim reads the device, sets a systemd-boot EFI variable, and chain-loads systemd-boot.

### Boot Chain

```
UEFI Firmware
  → switch-boot.efi (custom EFI shim)
    → reads USB HID device (Pico 2) to get switch position
    → sets LoaderEntryOneShot EFI variable
    → chain-loads systemd-boot
      → systemd-boot reads LoaderEntryOneShot
      → boots the selected entry
      → clears LoaderEntryOneShot automatically
```

### Components

| Component | Language | Role |
|-----------|----------|------|
| EFI shim (`switch-boot.efi`) | Rust (`uefi` crate, `x86_64-unknown-uefi` target) | Reads USB HID, sets EFI var, chain-loads systemd-boot |
| Virtual switch (`virtual-switch`) | Rust (`usbip` crate + `inquire` TUI) | Emulates the Pico's HID device over USB/IP for testing |
| Pico 2 firmware | Rust (embassy-rs or similar) | Reads GPIO rotary switch, presents as USB HID vendor device |
| NixOS boot entry | Nix | Generates a stable `nixos-current.conf` entry on the ESP |
| Test VM launcher | Nix flake | Builds everything and launches QEMU with OVMF |

---

## Development Phases

### Phase 1 — Project scaffolding

Set up the Nix flake and Rust workspace. Everything else builds on this.

- Create `flake.nix` using **flake-parts** for modular flake structure
- Use **crate2nix** to generate Nix build expressions from Cargo.toml
- Set up a **devShell** with Rust toolchain (stable + `x86_64-unknown-uefi` target), `usbip` userspace tools, `usbhid-dump`, QEMU, OVMF, `dosfstools` (for building ESP images), and `efibootmgr`
- Create the **Cargo workspace** root with `virtual-switch` and `efi-shim` as members
- Verify: `nix develop` drops you into a shell with all tools available, `cargo build` succeeds for the workspace (even if the crates are just stubs)

**Project structure after this phase:**

```
boot-switch/
├── flake.nix                    # flake-parts + crate2nix, devShell, packages
├── flake.lock
├── Cargo.toml                   # Workspace: members = ["virtual-switch", "efi-shim"]
├── efi-shim/
│   ├── Cargo.toml               # uefi crate, target = x86_64-unknown-uefi
│   └── src/
│       └── main.rs              # Stub for now
├── virtual-switch/
│   ├── Cargo.toml               # usbip, inquire, tokio dependencies
│   └── src/
│       └── main.rs              # Stub for now
├── pico-firmware/               # (Phase 8) Pico 2 Rust firmware, separate from workspace
├── test-vm/
│   ├── build-esp.sh             # Script to assemble the FAT32 ESP image
│   ├── loader.conf              # systemd-boot config for test VM
│   └── entries/                 # Dummy boot entries
│       ├── nixos-current.conf
│       ├── windows.conf
│       └── fedora.conf
└── nixos/                       # (Phase 9) NixOS module for nixos-current.conf generation
```

Note: `efi-shim` targets `x86_64-unknown-uefi` and `virtual-switch` targets normal `x86_64-unknown-linux-gnu`. They coexist in the same workspace; you build them with different `--target` flags. The `pico-firmware` needs an ARM target and lives outside the workspace.

### Phase 2 — Virtual switch (standalone, no EFI)

Build and test the `virtual-switch` binary completely independently. This validates the entire USB device emulation layer before any EFI work begins. See [Virtual Switch Design](#reference-virtual-switch-design) for implementation details.

- Implement `UsbInterfaceHandler` for the boot switch HID device using the `usbip` crate
- Add `inquire` TUI for interactive position selection (8 positions) in a loop
- Run the USB/IP server, attach via `usbip attach`, verify with `usbhid-dump` and `/dev/hidrawN`
- Verify: changing the position in the TUI updates live HID reports on the host

### Phase 3 — Hello World EFI

Build a minimal EFI application that prints to the UEFI console and exits. Verify it runs in QEMU with OVMF.

- Write a `#![no_std]` / `#![no_main]` EFI binary using the `uefi` crate
- Build the ESP disk image with the shim at `\EFI\BOOT\BOOTX64.EFI` (simplest for initial testing)
- Launch QEMU with OVMF and the ESP image
- Verify: you see output on the UEFI console

### Phase 4 — Chain-load systemd-boot

Have the shim locate and launch `systemd-bootx64.efi` from the ESP. See [Chain-Loading systemd-boot](#chain-loading-systemd-boot) for details.

- Move the shim to `\EFI\switch-boot\switch-boot.efi`, put systemd-boot at `\EFI\systemd\` and `\EFI\BOOT\`
- Implement `load_image()` + `start_image()` chain-loading
- Add dummy boot entries (`nixos-current.conf`, `windows.conf`, `fedora.conf`) to the ESP
- Verify: systemd-boot's menu appears showing the dummy entries

### Phase 5 — Set EFI variable (hardcoded)

Hardcode the shim to set `LoaderEntryOneShot` to a specific entry before chain-loading. See [Setting the EFI Variable](#setting-the-efi-variable) for details.

- Write the EFI variable using `runtime::set_variable()`
- Set `loader.conf` timeout so you can see which entry is selected
- Verify: systemd-boot auto-selects the hardcoded entry (it will fail to actually boot since there's no real kernel, but the selection confirms it works)

### Phase 6 — USB device enumeration in EFI

Add code to enumerate USB devices visible in the UEFI environment.

- Use `boot::locate_handle_buffer()` to find `UsbIo` handles, print VID/PID of each
- Start the virtual switch, attach via USB/IP, pass through to QEMU
- Verify: the virtual HID device appears with VID `0x6666` PID `0xB007`

### Phase 7 — Full end-to-end with virtual switch

Read the HID report from the virtual switch and wire everything together. See [Reading the USB HID Device](#reading-the-usb-hid-device) for details.

- Implement VID/PID matching and `UsbIo::sync_interrupt_transfer()` to read the 1-byte report
- Map position to entry ID, set `LoaderEntryOneShot`, chain-load systemd-boot
- Add the 2-second timeout with fallback to normal boot
- Verify: change position in TUI → reboot VM → correct entry selected in systemd-boot menu

### Phase 8 — Pico 2 firmware

Write the real hardware firmware. The EFI shim is unchanged from Phase 7. See [USB HID Device Design (Pico 2)](#reference-usb-hid-device-design-pico-2) for details.

- Implement Pico 2 firmware with 8-position rotary switch on GPIO, USB HID output using same VID/PID and report descriptor
- Test by plugging Pico into host and passing through to QEMU (`-device usb-host,vendorid=0x6666,productid=0xB007`)
- Verify: same behavior as virtual switch

### Phase 9 — Real hardware deployment

Install on the actual machine. See [Architecture Details](#reference-architecture-details) for ESP layout and boot order.

- Install `switch-boot.efi` to ESP at `\EFI\switch-boot\`
- Set UEFI boot order with `efibootmgr` (shim first, systemd-boot as fallback)
- Generate stable `nixos-current.conf` entry via NixOS config
- Mount switch and Pico in PC case
- Verify: full boot chain works with physical switch

---

## Reference: Architecture Details

### Why LoaderEntryOneShot (not LoaderEntryDefault)

- **Safety:** If the shim crashes or the Pico is disconnected, no stale variable persists. systemd-boot falls back to its normal default.
- **Effective persistence:** The shim runs every boot and sets the variable fresh each time, so from the user's perspective it behaves like a persistent selection.
- **Clean semantics:** systemd-boot clears it after reading, so there's never leftover state.

### ESP Layout

```
/EFI/
  BOOT/
    BOOTX64.EFI          ← systemd-boot (untouched, fallback)
  systemd/
    systemd-bootx64.efi  ← systemd-boot (chain-loaded by shim)
  switch-boot/
    switch-boot.efi      ← the custom EFI shim
/loader/
  loader.conf
  entries/
    nixos-current.conf    ← stable NixOS entry
    nixos-generation-*.conf  ← normal NixOS generations
    windows.conf          ← Windows entry (if applicable)
```

### UEFI Boot Order

```bash
# Create entry for the shim
efibootmgr --create --disk /dev/nvme0n1 --part 1 \
  --label "Boot Switch" \
  --loader '\EFI\switch-boot\switch-boot.efi'

# Set it as first priority, systemd-boot as fallback
efibootmgr --bootorder 0005,0001,0000
```

If the shim fails, UEFI firmware falls through to the next entry (systemd-boot directly). If that fails, `\EFI\BOOT\BOOTX64.EFI` is the final fallback.

---

## Reference: EFI Shim Design

### Crates

- **`uefi`** — main high-level crate. Provides `boot::*` and `runtime::*` freestanding functions (new API style), protocol access, and the `#[entry]` macro.
- **`uefi-raw`** — lower-level FFI types. Fallback if `uefi` doesn't wrap something you need.

### Entry Point

```rust
#![no_std]
#![no_main]

use uefi::prelude::*;

#[entry]
fn main() -> Status {
    // 1. Try to find and read the USB HID switch device (~2s timeout)
    // 2. If found, set LoaderEntryOneShot
    // 3. Chain-load systemd-boot (always, regardless of step 1/2 success)
}
```

### Reading the USB HID Device

The Pico presents as a vendor-defined HID device (usage page 0xFF00+) with VID `0x6666`, PID `0xB007`. The EFI shim:

1. Uses `boot::locate_handle_buffer()` to find all handles with `UsbIo` protocol
2. For each, calls `UsbIo::get_device_descriptor()` and checks VID/PID
3. Once found, calls `UsbIo::sync_interrupt_transfer()` on the interrupt IN endpoint to read the 1-byte HID report (the switch position)

**Timeout behavior:** Poll for the device for ~2 seconds. If not found (Pico disconnected, failure, etc.), skip variable setting and chain-load systemd-boot normally. The switch is never a boot blocker.

### Setting the EFI Variable

```rust
use uefi::runtime::{self, VariableAttributes};
use uefi::{guid, cstr16};

let vendor_guid = guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f");

// Map switch position to entry ID (positions 1-8, 0 = unmapped/skip)
let entry_id: Option<&CStr16> = match switch_position {
    1 => Some(cstr16!("nixos.conf")),
    2 => Some(cstr16!("windows.conf")),
    3 => Some(cstr16!("fedora.conf")),
    // positions 4-8 unmapped for now
    _ => None,  // Unknown/unmapped position, skip variable setting
};

let attrs = VariableAttributes::NON_VOLATILE
    | VariableAttributes::BOOTSERVICE_ACCESS
    | VariableAttributes::RUNTIME_ACCESS;

runtime::set_variable(
    cstr16!("LoaderEntryOneShot"),
    &vendor_guid,
    attrs,
    entry_id.as_bytes(),
);
```

**Variable details:**
- **Name:** `LoaderEntryOneShot` (UTF-16LE)
- **Vendor GUID:** `4a67b082-0a4c-41cf-b6c7-440b29bb8c4f` (systemd-boot's shared GUID)
- **Attributes:** NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS
- **Data:** The full entry filename including the `.conf` suffix as UTF-16LE bytes (e.g., `nixos.conf`, `windows.conf`).

### Chain-Loading systemd-boot

```rust
// Load systemd-boot from its standard location on the ESP
let device_path = /* construct device path to \EFI\systemd\systemd-bootx64.efi */;
let image_handle = boot::load_image(boot::image_handle(), &device_path)?;
boot::start_image(image_handle)?;
```

The shim loads `\EFI\systemd\systemd-bootx64.efi` (not `\EFI\BOOT\BOOTX64.EFI`). systemd-boot locates the ESP from its own loaded image's device path, so chain-loading works transparently — systemd-boot doesn't know or care that it was launched by another EFI app.

---

## Reference: Virtual Switch Design

### USB Identity

- **VID/PID:** `0x6666:0xB007` (`0x6666` is a widely-used prototype/testing VID for personal hardware projects, `0xB007` = boot)
- **Device class:** HID
- **Usage page:** Vendor-defined (0xFF00)
- **Report descriptor:** Single 8-bit input field representing switch position (values 1-8)

### Implementation (`usbip` crate)

**Crate:** [`usbip` 0.8.0](https://docs.rs/usbip/0.8.0/usbip/) — handles the entire USB/IP wire protocol, descriptor generation, and URB routing. You implement `UsbInterfaceHandler` for your custom device logic.

Modeled after the crate's `UsbHidKeyboardHandler` in `hid.rs`. Key differences:

- Report descriptor uses vendor-defined usage page (`0xFF00`) with a single 8-bit input field
- Interrupt IN handler returns a 1-byte report with the current switch position
- The current position is stored in an `Arc<AtomicU8>` shared between the USB/IP handler and the TUI thread

### Interactive TUI (`inquire`)

The binary uses the `inquire` crate to present a simple interactive selector in the terminal. On startup, it launches the USB/IP server in a background tokio task, then enters a loop where the user can select a switch position:

```
Boot Switch Virtual Device (USB/IP)
Server listening on 0.0.0.0:3240
VID: 0x6666  PID: 0xB007

Current position: 1 (nixos-current)

? Select switch position:
> 1: nixos-current
  2: windows
  3: fedora
  4: (unmapped)
  5: (unmapped)
  6: (unmapped)
  7: (unmapped)
  8: (unmapped)
```

Selecting a new position updates the `AtomicU8` immediately. The next HID interrupt IN transfer returns the new value. No restart needed. Unmapped positions cause the EFI shim to skip setting the variable, resulting in systemd-boot's normal default.

### Testing the virtual switch independently

```bash
# One-time: load the virtual host controller module
sudo modprobe vhci-hcd

# Terminal 1: Start the virtual switch with interactive TUI
cargo run --bin virtual-switch

# Terminal 2: Attach the virtual device to the host USB bus
sudo usbip attach -r 127.0.0.1 -b 0-0-0

# Terminal 2: Verify with usbhid-dump
sudo usbhid-dump -m 6666:b007 -ed   # dump report descriptor
sudo usbhid-dump -m 6666:b007 -es   # stream live reports

# Or read raw from hidraw
sudo cat /dev/hidrawN | xxd

# Cleanup
sudo usbip detach -p 0
```

---

## Reference: USB HID Device Design (Pico 2)

### Hardware

- Raspberry Pi Pico 2
- 8-position rotary switch wired to GPIO pins
- Powered via USB from the motherboard (standby 5V keeps it alive when PC is off)
- Not all positions need to be mapped — unmapped positions mean "no selection, boot systemd-boot's default"

### Behavior

- Continuously sends 1-byte HID reports with the current switch position (values 1-8)
- Uses a vendor-defined usage page, so the OS sees it as a generic HID device in `/dev/hidraw*` — no phantom keystrokes, no interference with normal use
- Same VID/PID and report descriptor as the virtual switch — the EFI shim code is identical for both

---

## Reference: QEMU Test VM

The Nix flake provides a `test-vm` target that:

1. Builds the EFI binary (`cargo build --target x86_64-unknown-uefi`)
2. Assembles an ESP disk image (FAT32) containing:
   - The EFI shim at `\EFI\switch-boot\switch-boot.efi`
   - systemd-boot at `\EFI\systemd\systemd-bootx64.efi` and `\EFI\BOOT\BOOTX64.EFI` (from nixpkgs `systemd` package)
   - `loader.conf` with a timeout so you can see the menu
   - Dummy boot entries: `nixos-current.conf`, `windows.conf`, `fedora.conf` (these don't need real kernels — systemd-boot will show them in the menu, and attempting to boot one confirms the correct entry was selected)
3. Launches QEMU with:
   - OVMF firmware (from nixpkgs)
   - The ESP image as a drive
   - USB xHCI controller
   - USB passthrough of the virtual HID device
   - Serial console for debug output from the shim

```bash
qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file=OVMF_CODE.fd \
  -drive if=pflash,format=raw,file=OVMF_VARS.fd \
  -drive format=raw,file=esp.img \
  -device qemu-xhci,id=xhci \
  -device usb-host,bus=xhci.0,vendorid=0x6666,productid=0xB007 \
  -nographic
```

---

## Reference: NixOS Integration

### Stable Boot Entry

The NixOS configuration should generate a fixed-name `nixos-current.conf` entry on the ESP that always points to the latest generation's kernel and initrd. This keeps the EFI shim simple — it just maps switch position 1 to `nixos-current` without needing to enumerate or sort generation entries.

This can be done via a custom boot entry in the NixOS configuration or a small activation script that maintains the symlink/entry after each rebuild.

---

## Open Questions

- **Entry mapping configuration:** The mapping from switch positions to boot entry IDs is currently hardcoded in the EFI shim. A config file on the ESP or a compile-time config would be more flexible. Decide whether this is worth the complexity.
- **OVMF USB timing:** OVMF may enumerate USB devices at different speeds than real firmware. The 2-second timeout in the shim should be tested and adjusted if needed.
- **Standby USB power:** Verify that the motherboard provides 5V standby on the USB port used for the Pico so it's powered when the PC is off. Most do, but some ports may not.
