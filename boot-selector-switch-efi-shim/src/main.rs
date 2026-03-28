#![no_std]
#![no_main]

extern crate alloc;

mod config;
mod logger;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use log::{debug, error, info, warn};
use uefi::Identify;
use uefi::boot;
use uefi::boot::{LoadImageSource, OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::cstr16;
use uefi::prelude::*;
use uefi::proto::BootPolicy;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::usb::io::UsbIo;
use uefi::runtime;
use uefi::runtime::VariableAttributes;
use uefi::system;

use config::load_config;

const SYSTEMD_BOOT_PATH: &uefi::CStr16 = cstr16!("\\EFI\\systemd\\systemd-bootx64.efi");

const SWITCH_VID: u16 = 0x6666;
const SWITCH_PID: u16 = 0xB007;
const SWITCH_ENDPOINT: u8 = 0x81;
const SWITCH_TIMEOUT_US: usize = 2_000_000;

const DEBUG_VAR_NAME: &uefi::CStr16 = cstr16!("BootSelectorDebug");

/// GUID for boot-selector-switch project EFI variables (BootSelectorDebug).
const BSS_VENDOR: runtime::VariableVendor =
    runtime::VariableVendor(uefi::guid!("614e5389-b94f-4994-9f26-558928eab8f1"));

/// GUID for systemd-boot loader variables (LoaderEntryOneShot).
const SYSTEMD_BOOT_VENDOR: runtime::VariableVendor =
    runtime::VariableVendor(uefi::guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"));

/// Number of consecutive identical reads required to trust the position.
const SWITCH_STABLE_READS: usize = 3;

/// Maximum number of read attempts before giving up on debouncing.
const SWITCH_MAX_READ_ATTEMPTS: usize = 50;

/// Main boot logic. Returns an error string on failure.
fn run() -> Result<(), String> {
    let config = match load_config() {
        Ok(c) => c,
        Err(e) => {
            warn!("Config loading failed, using default boot: {}", e);
            config::Config::empty()
        }
    };

    // Read debug mode from EFI variable and adjust log level
    let debug_mode = read_debug_mode();
    logger::set_debug(debug_mode);

    // Find switch position
    let mut position = find_switch_position();

    // In debug mode, show USB info and wait for enter before proceeding.
    // Re-read position after so the user can change the switch.
    if debug_mode {
        print_usb_devices();
        wait_for_enter();
        position = find_switch_position();
    }

    // Map position to entry via config, set LoaderEntryOneShot
    if let Some(pos) = position {
        info!("Switch position: {}", pos);
        if let Some(entry) = config.get_entry(pos) {
            debug!("Mapped to entry: {}", entry);

            let attrs = VariableAttributes::NON_VOLATILE
                | VariableAttributes::BOOTSERVICE_ACCESS
                | VariableAttributes::RUNTIME_ACCESS;

            info!("Setting LoaderEntryOneShot to {}", entry);
            runtime::set_variable(
                cstr16!("LoaderEntryOneShot"),
                &SYSTEMD_BOOT_VENDOR,
                attrs,
                entry.as_bytes(),
            )
            .map_err(|e| format!("Failed to set LoaderEntryOneShot: {:?}", e))?;
        } else {
            debug!("Position {} has no boot entry, using default boot", pos);
        }
    } else {
        debug!("No switch detected, using default boot");
    }

    // Build device path for systemd-boot
    let device_handle = {
        let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
            .map_err(|e| format!("Failed to open LoadedImage protocol: {:?}", e))?;
        loaded_image
            .device()
            .ok_or_else(|| String::from("LoadedImage has no device handle"))?
    };

    // Use non-exclusive access for DevicePath on the ESP device, since
    // systemd-boot may already hold this protocol open when we're chainloaded.
    let esp_device_path = unsafe {
        boot::open_protocol::<DevicePath>(
            OpenProtocolParams {
                handle: device_handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
        .map_err(|e| format!("Failed to open DevicePath on ESP device: {:?}", e))?
    };

    let mut buf = Vec::new();
    let mut builder = DevicePathBuilder::with_vec(&mut buf);
    for node in esp_device_path.node_iter() {
        builder = builder
            .push(&node)
            .map_err(|e| format!("Failed to push device path node: {:?}", e))?;
    }
    let boot_path = builder
        .push(&build::media::FilePath {
            path_name: SYSTEMD_BOOT_PATH,
        })
        .map_err(|e| format!("Failed to push FilePath node: {:?}", e))?
        .finalize()
        .map_err(|e| format!("Failed to finalize device path: {:?}", e))?;

    debug!("Loading systemd-boot from \\EFI\\systemd\\systemd-bootx64.efi");

    let loaded_handle = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath {
            device_path: boot_path,
            boot_policy: BootPolicy::BootSelection,
        },
    )
    .map_err(|e| format!("Failed to load systemd-boot image: {:?}", e))?;

    // Beep the position number so the user knows which entry was selected
    if let Some(pos) = position {
        system::with_stdout(|stdout| {
            for _ in 0..pos {
                let _ = stdout.output_string(cstr16!("\x07"));
                boot::stall(Duration::from_millis(100));
            }
        });
    }

    // Chain-load systemd-boot
    info!("Chain-loading systemd-boot");
    boot::start_image(loaded_handle)
        .map_err(|e| format!("Failed to start systemd-boot: {:?}", e))?;

    Err(String::from("systemd-boot returned unexpectedly"))
}

fn read_debug_mode() -> bool {
    let mut buf = [0u8; 8];
    match runtime::get_variable(DEBUG_VAR_NAME, &BSS_VENDOR, &mut buf) {
        Ok((data, _attrs)) => {
            let len = data.len();
            let enabled = buf[0] != 0;
            debug!(
                "Debug mode: {} ({} bytes)",
                if enabled { "enabled" } else { "disabled" },
                len
            );
            enabled
        }
        Err(_) => {
            debug!("Debug mode: disabled (variable not set)");
            false
        }
    }
}

fn write_debug_mode(enabled: bool) -> Result<(), String> {
    let attrs = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;
    let value: [u8; 1] = [if enabled { 0x01 } else { 0x00 }];
    runtime::set_variable(DEBUG_VAR_NAME, &BSS_VENDOR, attrs, &value)
        .map_err(|e| format!("Failed to set BootSelectorDebug variable: {:?}", e))
}

/// Print verbose USB device information for all connected devices.
fn print_usb_devices() {
    let handles = match boot::locate_handle_buffer(SearchType::ByProtocol(&UsbIo::GUID)) {
        Ok(h) => h,
        Err(_) => {
            debug!("No USB devices found");
            return;
        }
    };
    debug!("Found {} USB device(s)", handles.len());

    for handle in handles.iter() {
        let mut usb_io = match boot::open_protocol_exclusive::<UsbIo>(*handle) {
            Ok(io) => io,
            Err(_) => continue,
        };
        let desc = match usb_io.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };
        debug!(
            "USB: VID=0x{:04X} PID=0x{:04X} Class=0x{:02X} SubClass=0x{:02X} Protocol=0x{:02X}",
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
                        debug!("  Manufacturer: {:?}", s);
                    }
                }
                if desc.str_product != 0 {
                    if let Ok(s) = usb_io.string_descriptor(lang, desc.str_product) {
                        debug!("  Product: {:?}", s);
                    }
                }
                if desc.str_serial_number != 0 {
                    if let Ok(s) = usb_io.string_descriptor(lang, desc.str_serial_number) {
                        debug!("  Serial: {:?}", s);
                    }
                }
            }
        }
    }
}

/// Find the boot selector switch and read its position.
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
            debug!("Found boot selector switch");
            let mut buf = [0u8; 1];
            let mut last_value: Option<u8> = None;
            let mut stable_count: usize = 0;

            for _attempt in 0..SWITCH_MAX_READ_ATTEMPTS {
                match usb_io.sync_interrupt_receive(SWITCH_ENDPOINT, &mut buf, SWITCH_TIMEOUT_US) {
                    Ok(_) => {
                        let value = buf[0];
                        if last_value == Some(value) {
                            stable_count += 1;
                            if stable_count >= SWITCH_STABLE_READS {
                                debug!(
                                    "Switch position: {} (stable after {} reads)",
                                    value, stable_count
                                );
                                return Some(value);
                            }
                        } else {
                            debug!("Switch read: {} (waiting for stable)", value);
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
            error!(
                "Switch position not stable after {} attempts, last value: {:?}",
                SWITCH_MAX_READ_ATTEMPTS, last_value
            );
            return None;
        }
    }
    debug!("Boot selector switch not found");
    None
}

/// Wait for the user to press Enter.
fn wait_for_enter() {
    info!("Press Enter to continue...");
    system::with_stdin(|stdin| {
        loop {
            if let Ok(Some(key)) = stdin.read_key() {
                if let uefi::proto::console::text::Key::Printable(c) = key {
                    if u16::from(c) == u16::from(uefi::Char16::try_from('\r').expect("valid char"))
                    {
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
    uefi::helpers::init().expect("failed to initialize UEFI helpers");
    logger::init();
    info!("boot-selector-switch: starting");

    if let Err(e) = run() {
        // Enable debug mode so the next boot shows verbose output
        let _ = write_debug_mode(true);
        error!("{}", e);
        wait_for_enter();
    }

    Status::ABORTED
}
