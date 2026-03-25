#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // Pico 2 onboard LED is on GPIO25
    let mut led = Output::new(p.PIN_25, Level::Low);

    info!("Blink test starting!");

    let mut count: u32 = 0;
    loop {
        info!("Blink #{}", count);
        led.set_high();
        Timer::after_millis(500).await;
        led.set_low();
        Timer::after_millis(500).await;
        count += 1;
    }
}
