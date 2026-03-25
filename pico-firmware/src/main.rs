#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Pull};
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

/// GPIO pins for switch positions 1-8 (active-low with pull-ups).
/// Using GPIO2-GPIO9 — adjust these to match your wiring.
const SWITCH_PINS: [u8; 8] = [2, 3, 4, 5, 6, 7, 8, 9];

/// Same HID report descriptor as virtual-switch:
/// Vendor-defined (0xFF00), single 8-bit input value (1-8).
const REPORT_DESCRIPTOR: &[u8] = &[
    0x06, 0x00, 0xFF, // Usage Page (Vendor Defined 0xFF00)
    0x09, 0x01, // Usage (Vendor Usage 1)
    0xA1, 0x01, // Collection (Application)
    0x09, 0x01, //   Usage (Vendor Usage 1)
    0x15, 0x01, //   Logical Minimum (1)
    0x25, 0x08, //   Logical Maximum (8)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x02, //   Input (Data, Variable, Absolute)
    0xC0, // End Collection
];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // Set up GPIO inputs with pull-ups for switch positions.
    // Each switch position connects one pin to ground (active-low).
    let pins: [Input<'_>; 8] = [
        Input::new(p.PIN_2, Pull::Up),
        Input::new(p.PIN_3, Pull::Up),
        Input::new(p.PIN_4, Pull::Up),
        Input::new(p.PIN_5, Pull::Up),
        Input::new(p.PIN_6, Pull::Up),
        Input::new(p.PIN_7, Pull::Up),
        Input::new(p.PIN_8, Pull::Up),
        Input::new(p.PIN_9, Pull::Up),
    ];

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
        report_descriptor: REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: 10,
        max_packet_size: 1,
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    let hid_writer = HidWriter::<_, 1>::new(&mut builder, hid_state, hid_config);

    // Build USB device
    let mut usb = builder.build();

    // Run USB device and report loop concurrently
    let usb_fut = usb.run();
    let report_fut = report_loop(hid_writer, pins);

    join(usb_fut, report_fut).await;
}

/// Read switch position from GPIO pins and send HID reports.
async fn report_loop<'a>(mut hid_writer: HidWriter<'a, Driver<'a, USB>, 1>, pins: [Input<'a>; 8]) {
    let mut last_position: u8 = 0;

    info!("Boot selector switch ready, scanning GPIO pins...");

    loop {
        // Scan pins for active-low position
        let mut position: u8 = 1; // Default to position 1 if no pin is active
        for (i, pin) in pins.iter().enumerate() {
            if pin.is_low() {
                position = (i as u8) + 1;
                break;
            }
        }

        // Only send report when position changes
        if position != last_position {
            info!("Switch position: {}", position);
            last_position = position;
            match hid_writer.write(&[position]).await {
                Ok(()) => {}
                Err(e) => warn!("HID write error: {}", e),
            }
        }

        // Poll at ~50Hz
        Timer::after_millis(20).await;
    }
}
