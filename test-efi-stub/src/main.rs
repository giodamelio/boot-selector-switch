#![no_std]
#![no_main]

mod logger;

use core::time::Duration;
use log::info;
use uefi::boot;
use uefi::prelude::*;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();
    logger::init();

    let text = env!("TEST_ENTRY_TEXT");
    info!("=== {} ===", text);

    for n in (1..=10).rev() {
        info!("Shutting down in {}...", n);
        boot::stall(Duration::from_secs(1));
    }

    uefi::runtime::reset(uefi::runtime::ResetType::SHUTDOWN, Status::SUCCESS, None)
}
