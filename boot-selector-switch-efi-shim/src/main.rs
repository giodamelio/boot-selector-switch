#![no_std]
#![no_main]

mod logger;

use core::time::Duration;
use log::{error, info, warn};
use uefi::prelude::*;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    logger::init();
    info!("Hello from boot-selector-switch EFI shim!");
    info!("Phase 3 complete - EFI application is running.");
    boot::stall(Duration::from_secs(10));
    uefi::runtime::reset(uefi::runtime::ResetType::SHUTDOWN, Status::SUCCESS, None);
}
