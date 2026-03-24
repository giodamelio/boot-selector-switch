mod descriptors;
mod handler;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use usbip::{UsbDevice, UsbEndpoint, UsbInterfaceHandler, UsbIpServer};

use handler::BootSwitchHandler;

#[tokio::main]
async fn main() {
    env_logger::init();

    let position = Arc::new(AtomicU8::new(1));

    let handler: Arc<Mutex<Box<dyn UsbInterfaceHandler + Send>>> = Arc::new(Mutex::new(Box::new(
        BootSwitchHandler::new(position.clone()),
    )));

    let mut device = UsbDevice::new(0).with_interface(
        0x03, // HID class
        0x00, // No subclass (not boot interface)
        0x00, // No protocol
        Some("Boot Selector Switch"),
        vec![UsbEndpoint {
            address: 0x81,         // IN endpoint 1
            attributes: 0x03,      // Interrupt
            max_packet_size: 0x08, // 8 bytes
            interval: 10,          // 10ms
        }],
        handler,
    );
    device.vendor_id = 0x6666;
    device.product_id = 0xB007;

    let server = Arc::new(UsbIpServer::new_simulated(vec![device]));

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 3240);
    log::info!("Starting USB/IP server on {}", addr);
    tokio::spawn(usbip::server(addr, server));

    // Wait for the server to be listening before starting the TUI,
    // so log output doesn't stomp over the inquire prompt.
    while tokio::net::TcpStream::connect(addr).await.is_err() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    log::info!("USB/IP server ready");

    let tui_position = position.clone();
    tokio::task::spawn_blocking(move || {
        run_tui(tui_position);
    })
    .await
    .expect("TUI task panicked");
}

fn run_tui(position: Arc<AtomicU8>) {
    loop {
        let current = position.load(Ordering::Relaxed);

        let input = inquire::Text::new(&format!(
            "Position [1-8, q to quit] (current: {}):",
            current
        ))
        .prompt();

        match input {
            Ok(s) if s.trim().eq_ignore_ascii_case("q") => {
                println!("Exiting.");
                return;
            }
            Ok(s) => match s.trim().parse::<u8>() {
                Ok(n) if (1..=8).contains(&n) => {
                    position.store(n, Ordering::Relaxed);
                    log::info!("Position changed to {}", n);
                }
                _ => println!("Invalid input. Enter 1-8 or q."),
            },
            Err(
                inquire::InquireError::OperationCanceled
                | inquire::InquireError::OperationInterrupted,
            ) => {
                println!("Exiting.");
                return;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        }
    }
}
