use crate::SWITCH_PINS;

/// HID report descriptor: vendor-defined (0xFF00), single 8-bit input.
/// Logical Maximum is derived from SWITCH_PINS.
pub const REPORT_DESCRIPTOR: &[u8] = &[
    0x06,
    0x00,
    0xFF, // Usage Page (Vendor Defined 0xFF00)
    0x09,
    0x01, // Usage (Vendor Usage 1)
    0xA1,
    0x01, // Collection (Application)
    0x09,
    0x01, //   Usage (Vendor Usage 1)
    0x15,
    0x01, //   Logical Minimum (1)
    0x25,
    SWITCH_PINS.len() as u8, //   Logical Maximum (number of positions)
    0x75,
    0x08, //   Report Size (8)
    0x95,
    0x01, //   Report Count (1)
    0x81,
    0x02, //   Input (Data, Variable, Absolute)
    0xC0, // End Collection
];
