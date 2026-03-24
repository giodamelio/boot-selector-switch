#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main() -> u64 {
    0 // EFI_SUCCESS
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
