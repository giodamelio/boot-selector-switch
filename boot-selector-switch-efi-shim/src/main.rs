#![no_std]
#![no_main]

extern crate alloc;

mod logger;

use alloc::vec::Vec;
use core::time::Duration;
use log::{error, info};
use uefi::Identify;
use uefi::boot;
use uefi::boot::{LoadImageSource, SearchType};
use uefi::prelude::*;
use uefi::proto::BootPolicy;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::usb::io::UsbIo;
use uefi::runtime;
use uefi::runtime::VariableAttributes;
use uefi::system;

const SYSTEMD_BOOT_PATH: &uefi::CStr16 = uefi::cstr16!("\\EFI\\systemd\\systemd-bootx64.efi");

const SWITCH_VID: u16 = 0x6666;
const SWITCH_PID: u16 = 0xB007;
const SWITCH_ENDPOINT: u8 = 0x81;
const SWITCH_TIMEOUT_US: usize = 2_000_000;

const DEBUG_VAR_NAME: &uefi::CStr16 = uefi::cstr16!("BootSelectorDebug");

fn debug_vendor() -> runtime::VariableVendor {
    // Boot-selector-switch project GUID (distinct from systemd-boot's GUID)
    runtime::VariableVendor(uefi::guid!("614e5389-b94f-4994-9f26-558928eab8f1"))
}

fn read_debug_mode() -> bool {
    let mut buf = [0u8; 1];
    match runtime::get_variable(DEBUG_VAR_NAME, &debug_vendor(), &mut buf) {
        Ok((_, _)) => buf[0] != 0,
        Err(_) => false,
    }
}

fn write_debug_mode(enabled: bool) {
    let attrs = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;
    let value: [u8; 1] = [if enabled { 0x01 } else { 0x00 }];
    runtime::set_variable(DEBUG_VAR_NAME, &debug_vendor(), attrs, &value)
        .expect("Failed to set BootSelectorDebug variable");
}

/// Print verbose USB device information for all connected devices.
fn print_usb_devices() {
    let handles = match boot::locate_handle_buffer(SearchType::ByProtocol(&UsbIo::GUID)) {
        Ok(h) => h,
        Err(_) => {
            info!("[debug] No USB devices found");
            return;
        }
    };
    info!("[debug] Found {} USB device(s)", handles.len());

    for handle in handles.iter() {
        let mut usb_io = match boot::open_protocol_exclusive::<UsbIo>(*handle) {
            Ok(io) => io,
            Err(_) => continue,
        };
        let desc = match usb_io.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };
        info!(
            "[debug]   USB: VID=0x{:04X} PID=0x{:04X} Class=0x{:02X} SubClass=0x{:02X} Protocol=0x{:02X}",
            desc.id_vendor,
            desc.id_product,
            desc.device_class,
            desc.device_subclass,
            desc.device_protocol
        );
        if let Ok(langs) = usb_io.supported_languages() {
            if let Some(&lang) = langs.first() {
                if desc.str_manufacturer != 0 {
                    if let Ok(s) = usb_io.string_descriptor(lang, desc.str_manufacturer) {
                        info!("[debug]     Manufacturer: {:?}", s);
                    }
                }
                if desc.str_product != 0 {
                    if let Ok(s) = usb_io.string_descriptor(lang, desc.str_product) {
                        info!("[debug]     Product: {:?}", s);
                    }
                }
                if desc.str_serial_number != 0 {
                    if let Ok(s) = usb_io.string_descriptor(lang, desc.str_serial_number) {
                        info!("[debug]     Serial: {:?}", s);
                    }
                }
            }
        }
    }
}

/// Number of consecutive identical reads required to trust the position.
/// Drains stale reports that may be buffered from before a reboot
/// (the Pico may not lose power across warm reboots).
const SWITCH_STABLE_READS: usize = 3;

/// Find the boot selector switch and read its position.
/// Reads multiple reports to ensure we have a fresh, stable value.
fn find_switch_position() -> Option<u8> {
    let handles = boot::locate_handle_buffer(SearchType::ByProtocol(&UsbIo::GUID)).ok()?;

    for handle in handles.iter() {
        let mut usb_io = match boot::open_protocol_exclusive::<UsbIo>(*handle) {
            Ok(io) => io,
            Err(_) => continue,
        };
        let desc = match usb_io.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if desc.id_vendor == SWITCH_VID && desc.id_product == SWITCH_PID {
            info!("Found boot selector switch");
            let mut buf = [0u8; 1];
            let mut last_value: Option<u8> = None;
            let mut stable_count: usize = 0;

            // Read until we get SWITCH_STABLE_READS consecutive identical values
            loop {
                match usb_io.sync_interrupt_receive(SWITCH_ENDPOINT, &mut buf, SWITCH_TIMEOUT_US) {
                    Ok(_) => {
                        let value = buf[0];
                        if last_value == Some(value) {
                            stable_count += 1;
                            if stable_count >= SWITCH_STABLE_READS {
                                info!(
                                    "Switch position: {} (stable after {} reads)",
                                    value, stable_count
                                );
                                return Some(value);
                            }
                        } else {
                            info!("Switch read: {} (waiting for stable)", value);
                            last_value = Some(value);
                            stable_count = 1;
                        }
                    }
                    Err(e) => {
                        error!("Failed to read HID report: {:?}", e);
                        return None;
                    }
                }
            }
        }
    }
    info!("Boot selector switch not found");
    None
}

/// Wait for the user to press Enter.
fn wait_for_enter() {
    info!("Press Enter to chain-load systemd-boot...");
    system::with_stdin(|stdin| {
        loop {
            if let Ok(Some(key)) = stdin.read_key() {
                if let uefi::proto::console::text::Key::Printable(c) = key {
                    if c == uefi::Char16::try_from('\r').unwrap() {
                        break;
                    }
                }
            }
            boot::stall(Duration::from_millis(50));
        }
    });
}

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    logger::init();
    info!("boot-selector-switch: starting");

    // Step 1: Read debug mode from EFI variable
    let mut debug_mode = read_debug_mode();

    // Step 2: Find switch position
    let mut position = find_switch_position();

    // Step 3: If position == 6, toggle debug mode and re-read
    if position == Some(6) {
        debug_mode = !debug_mode;
        write_debug_mode(debug_mode);
        if debug_mode {
            info!("Debug mode ENABLED");
        } else {
            info!("Debug mode DISABLED");
        }

        info!("Set switch to desired position, then press Enter...");
        wait_for_enter();

        // Re-read switch position
        position = find_switch_position();
    }

    // Step 4: If debug mode, print verbose USB info, beep, and wait for Enter.
    // Re-read position after the wait so the user can change the switch.
    if debug_mode {
        print_usb_devices();
        system::with_stdout(|stdout| {
            let _ = stdout.output_string(uefi::cstr16!("\x07"));
        });
        wait_for_enter();
        position = find_switch_position();
    }

    // Step 5: Map position to entry, set LoaderEntryOneShot
    let chosen = match position {
        Some(pos) => {
            info!("Switch position: {}", pos);
            match pos {
                1 => Some(uefi::cstr16!("nixos-latest.conf")),
                2 => Some(uefi::cstr16!("windows.conf")),
                3 => Some(uefi::cstr16!("netbootxyz.conf")),
                _ => {
                    info!("Position {} unmapped, skipping", pos);
                    None
                }
            }
        }
        None => {
            info!("No switch detected, using default boot");
            None
        }
    };

    if let Some(entry) = chosen {
        let vendor = runtime::VariableVendor(uefi::guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"));

        let attrs = VariableAttributes::NON_VOLATILE
            | VariableAttributes::BOOTSERVICE_ACCESS
            | VariableAttributes::RUNTIME_ACCESS;

        info!("Setting LoaderEntryOneShot to {:?}", entry);
        runtime::set_variable(
            uefi::cstr16!("LoaderEntryOneShot"),
            &vendor,
            attrs,
            entry.as_bytes(),
        )
        .expect("Failed to set LoaderEntryOneShot");
    }

    // Step 6: Load systemd-boot image
    let device_handle = {
        let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
            .expect("Failed to open LoadedImage protocol");
        loaded_image
            .device()
            .expect("LoadedImage has no device handle")
    };

    let esp_device_path = boot::open_protocol_exclusive::<DevicePath>(device_handle)
        .expect("Failed to open DevicePath protocol on ESP device");

    let mut buf = Vec::new();
    let mut builder = DevicePathBuilder::with_vec(&mut buf);
    for node in esp_device_path.node_iter() {
        builder = builder
            .push(&node)
            .expect("Failed to push device path node");
    }
    let boot_path = builder
        .push(&build::media::FilePath {
            path_name: SYSTEMD_BOOT_PATH,
        })
        .expect("Failed to push FilePath node")
        .finalize()
        .expect("Failed to finalize device path");

    info!("Loading systemd-boot from \\EFI\\systemd\\systemd-bootx64.efi");

    let loaded_handle = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath {
            device_path: boot_path,
            boot_policy: BootPolicy::BootSelection,
        },
    )
    .expect("Failed to load systemd-boot image");

    // Step 7: Beep the position number so the user knows which entry was selected
    if let Some(pos) = position {
        system::with_stdout(|stdout| {
            for _ in 0..pos {
                let _ = stdout.output_string(uefi::cstr16!("\x07"));
                boot::stall(Duration::from_millis(100));
            }
        });
    }

    // Step 8: Start systemd-boot
    info!("Chain-loading systemd-boot");
    boot::start_image(loaded_handle).expect("Failed to start systemd-boot");

    error!("systemd-boot returned unexpectedly");
    Status::ABORTED
}
