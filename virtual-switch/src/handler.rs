use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use usbip::{SetupPacket, UsbEndpoint, UsbInterface, UsbInterfaceHandler};

use crate::descriptors::{REPORT_DESCRIPTOR, hid_class_descriptor};

/// HID GET_DESCRIPTOR request code.
const GET_DESCRIPTOR: u8 = 0x06;
/// HID SET_IDLE request code.
const SET_IDLE: u8 = 0x0A;
/// HID Report descriptor type (high byte of wValue).
const HID_REPORT_DESCRIPTOR_TYPE: u8 = 0x22;

#[derive(Debug)]
pub struct BootSwitchHandler {
    position: Arc<AtomicU8>,
}

impl BootSwitchHandler {
    pub fn new(position: Arc<AtomicU8>) -> Self {
        Self { position }
    }
}

impl UsbInterfaceHandler for BootSwitchHandler {
    fn get_class_specific_descriptor(&self) -> Vec<u8> {
        hid_class_descriptor()
    }

    fn handle_urb(
        &mut self,
        _interface: &UsbInterface,
        ep: UsbEndpoint,
        _transfer_buffer_length: u32,
        setup: SetupPacket,
        _req: &[u8],
    ) -> Result<Vec<u8>, std::io::Error> {
        if ep.is_ep0() {
            // Control transfers
            let descriptor_type = (setup.value >> 8) as u8;

            if setup.request == GET_DESCRIPTOR && descriptor_type == HID_REPORT_DESCRIPTOR_TYPE {
                log::debug!("GET_DESCRIPTOR: HID Report Descriptor");
                return Ok(REPORT_DESCRIPTOR.to_vec());
            }

            if setup.request == SET_IDLE {
                log::debug!("SET_IDLE");
                return Ok(vec![]);
            }

            log::warn!(
                "Unhandled EP0 request: request={:#x} value={:#x}",
                setup.request,
                setup.value
            );
            Ok(vec![])
        } else {
            // Interrupt IN — return 1-byte position report
            let pos = self.position.load(Ordering::Relaxed);
            Ok(vec![pos])
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
