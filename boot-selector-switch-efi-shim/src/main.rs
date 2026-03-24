#![no_std]
#![no_main]

extern crate alloc;

mod logger;

use alloc::vec::Vec;
use core::time::Duration;
use log::{error, info};
use uefi::boot;
use uefi::boot::LoadImageSource;
use uefi::prelude::*;
use uefi::proto::BootPolicy;
use uefi::proto::device_path::DevicePath;
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::loaded_image::LoadedImage;
use uefi::runtime;
use uefi::runtime::VariableAttributes;
use uefi::system;

const SYSTEMD_BOOT_PATH: &uefi::CStr16 = uefi::cstr16!("\\EFI\\systemd\\systemd-bootx64.efi");

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    logger::init();
    info!("boot-selector-switch: starting chain-load of systemd-boot");

    // Pick a random boot entry using the UEFI real-time clock
    let time = runtime::get_time().expect("Failed to get UEFI time");
    let seed = time.second() as u64 + time.minute() as u64 * 60 + time.nanosecond() as u64;
    let entries: [&uefi::CStr16; 3] = [
        uefi::cstr16!("nixos.conf"),
        uefi::cstr16!("windows.conf"),
        uefi::cstr16!("fedora.conf"),
    ];
    let index = (seed % 3) as usize;
    let chosen = entries[index];

    // systemd-boot's shared vendor GUID
    let vendor = runtime::VariableVendor(uefi::guid!("4a67b082-0a4c-41cf-b6c7-440b29bb8c4f"));

    let attrs = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;

    // Beep to indicate the shim is running
    system::with_stdout(|stdout| {
        let _ = stdout.output_string(uefi::cstr16!("\x07"));
    });

    info!("Setting LoaderEntryOneShot to entry index {}", index);
    runtime::set_variable(
        uefi::cstr16!("LoaderEntryOneShot"),
        &vendor,
        attrs,
        chosen.as_bytes(),
    )
    .expect("Failed to set LoaderEntryOneShot");

    // Get the device handle for the ESP from our loaded image.
    // We must drop the ScopedProtocol before calling load_image to avoid
    // an exclusive-access conflict.
    let device_handle = {
        let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
            .expect("Failed to open LoadedImage protocol");
        loaded_image
            .device()
            .expect("LoadedImage has no device handle")
    };

    // Get the device path for the ESP device.
    let esp_device_path = boot::open_protocol_exclusive::<DevicePath>(device_handle)
        .expect("Failed to open DevicePath protocol on ESP device");

    // Build the full device path to systemd-boot by copying the ESP device
    // path nodes and appending a FilePath node for the systemd-boot binary.
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

    // Load and start systemd-boot. On success, start_image does not return.
    let loaded_handle = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath {
            device_path: boot_path,
            boot_policy: BootPolicy::BootSelection,
        },
    )
    .expect("Failed to load systemd-boot image");

    // TODO: remove or shorten this delay before shipping
    info!("Chain-loading systemd-boot in 5 seconds...");
    boot::stall(Duration::from_secs(5));
    info!("Starting systemd-boot now.");
    boot::start_image(loaded_handle).expect("Failed to start systemd-boot");

    // Should not be reached.
    error!("systemd-boot returned unexpectedly");
    Status::ABORTED
}
