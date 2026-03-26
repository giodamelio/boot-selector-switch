#![no_std]
#![no_main]

mod hid_descriptor;

use core::sync::atomic::{AtomicU8, Ordering};

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{AnyPin, Input, Pull};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_time::Timer;
use embassy_usb::Builder;
use embassy_usb::class::hid::{HidBootProtocol, HidSubclass, HidWriter, State};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

/// GPIO pins for each switch position (active-low with pull-ups).
/// This is the single source of truth — edit only this line to change positions.
pub const SWITCH_PINS: [u8; 6] = [2, 3, 4, 5, 6, 7];

/// Current switch position, updated by the pin scanner task.
static POSITION: AtomicU8 = AtomicU8::new(0);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // Initialize GPIO inputs from SWITCH_PINS.
    // SAFETY: Each pin number appears exactly once in SWITCH_PINS, so there are no
    // aliased peripherals. We use steal() to allow data-driven pin initialization.
    let pins: [Input<'static>; SWITCH_PINS.len()] =
        core::array::from_fn(|i| Input::new(unsafe { AnyPin::steal(SWITCH_PINS[i]) }, Pull::Up));

    // Spawn pin scanner — runs independently of USB
    spawner.spawn(scan_pins(pins).expect("failed to spawn scan_pins task"));

    // Create USB driver
    let driver = Driver::new(p.USB, Irqs);

    // Configure USB device
    let mut config = embassy_usb::Config::new(0x6666, 0xB007);
    config.manufacturer = Some("Boot Selector");
    config.product = Some("Boot Selector Switch");
    config.serial_number = Some("001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Allocate USB buffers
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        MSOS_DESC.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    );

    // Create HID writer (1-byte reports)
    static HID_STATE: StaticCell<State> = StaticCell::new();
    let hid_state = HID_STATE.init(State::new());
    let hid_config = embassy_usb::class::hid::Config {
        report_descriptor: hid_descriptor::REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: 50,
        max_packet_size: 1,
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    let hid_writer = HidWriter::<_, 1>::new(&mut builder, hid_state, hid_config);

    // Build USB device
    let mut usb = builder.build();

    // Run USB stack and HID report writer concurrently
    join(usb.run(), hid_report_loop(hid_writer)).await;
}

/// Continuously scan GPIO pins and update the shared POSITION atomic.
/// Minimal debounce: ignores break-before-make gaps (no pin active).
/// Further debouncing is the host's responsibility.
#[embassy_executor::task]
async fn scan_pins(pins: [Input<'static>; SWITCH_PINS.len()]) {
    let mut last_position: u8 = 0;

    info!(
        "Boot selector switch ready, scanning {} positions...",
        SWITCH_PINS.len()
    );

    loop {
        // First active-low pin wins; 0 means no pin active
        let mut position: u8 = 0;
        for (i, pin) in pins.iter().enumerate() {
            if pin.is_low() {
                position = (i as u8) + 1;
                break;
            }
        }

        // Ignore no-pin-active reads (break-before-make transition),
        // only update on actual position changes
        if position != 0 && position != last_position {
            info!("Switch position: {}", position);
            last_position = position;
            POSITION.store(position, Ordering::Relaxed);
        }

        Timer::after_millis(5).await;
    }
}

/// Respond to host IN polls with the current switch position.
/// write() blocks until the host actually polls the interrupt endpoint,
/// so the value is always fresh — no stale data sits in a queue.
/// If no host is connected, the endpoint is disabled and we wait to retry.
async fn hid_report_loop(mut writer: HidWriter<'static, Driver<'static, USB>, 1>) {
    loop {
        let position = POSITION.load(Ordering::Relaxed);
        if position == 0 {
            // No position read yet, wait before checking again
            Timer::after_millis(10).await;
            continue;
        }

        match writer.write(&[position]).await {
            Ok(()) => {}
            Err(_) => {
                // Endpoint disabled (no host connected) — back off and retry
                Timer::after_millis(100).await;
            }
        }
    }
}
