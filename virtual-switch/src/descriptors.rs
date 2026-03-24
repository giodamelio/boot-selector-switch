/// Vendor-defined HID report descriptor.
/// Defines a single 8-bit input value (1-8) on usage page 0xFF00.
pub const REPORT_DESCRIPTOR: &[u8] = &[
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

/// Returns a 9-byte HID class descriptor referencing the report descriptor.
pub fn hid_class_descriptor() -> Vec<u8> {
    let report_len = REPORT_DESCRIPTOR.len() as u16;
    vec![
        // bLength
        0x09,
        // bDescriptorType: HID
        0x21,
        // bcdHID 1.11
        0x11,
        0x01,
        // bCountryCode
        0x00,
        // bNumDescriptors
        0x01,
        // bDescriptorType[0]: Report
        0x22,
        // wDescriptorLength (little-endian)
        report_len as u8,
        (report_len >> 8) as u8,
    ]
}
