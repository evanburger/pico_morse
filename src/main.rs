//! Receive ASCII characters via HTTP/TCP and blink the onboard LED with the corresponding morse encoding.
//! This only works with the Pico W board, not the regular Pico board.

#![no_main]
#![no_std]
#![allow(async_fn_in_trait)]

use core::str::from_utf8;
use log::{info,warn};

// WIFI + LED
use cyw43_pio::PioSpi;
use cyw43::Control;
use defmt::unwrap;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, Stack, StackResources};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler as PIOInterruptHandler, Pio};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// STANDARD_DURATION controls the blink speed, with smaller values being faster.
const STANDARD_DURATION: u64 = 100;  // Time in milliseconds for a dit-like unit.
const MEDIUM_DURATION: u64 = 3 * STANDARD_DURATION;  // Time in milliseconds for a dah-like unit.
const RESPONSE_BYTES: &[u8] = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".as_bytes();  // A minimum HTTP response so that the client knows everything was OK.
const ERROR_RESPONSE_BYTES: &[u8] = "HTTP/1.1 400 Bad Request\r\nX-Unknown-Morse-Character: ".as_bytes();  // A minimum HTTP response so that the client knows there was an error.
const PORT: u16 = 80;  // The TCP port to listen on.
const WIFI_NETWORK: &str = env!("WIFI_NETWORK");  // The SSID of the WiFi network to connect to.
const WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");  // The password of the WiFi network to connect to.

#[embassy_executor::task]
async fn wifi_task(runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}
// \WIFI + LED

// USB
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler as USBInterruptHandler};
use {defmt_rtt as _, panic_probe as _};
// \USB

// USB
#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Debug, driver);
}
// \USB

// All
bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PIOInterruptHandler<PIO0>;
    USBCTRL_IRQ => USBInterruptHandler<USB>;
});
// \All

async fn dit<'a>(control: &mut Control<'a>) {
    info!("dit");
    control.gpio_set(0, true).await;
    Timer::after(Duration::from_millis(STANDARD_DURATION)).await;
    control.gpio_set(0, false).await;
    Timer::after(Duration::from_millis(STANDARD_DURATION)).await;
}

async fn dah<'a>(control: &mut Control<'a>) {
    info!("dah");
    control.gpio_set(0, true).await;
    Timer::after(Duration::from_millis(MEDIUM_DURATION)).await;
    control.gpio_set(0, false).await;
    Timer::after(Duration::from_millis(STANDARD_DURATION)).await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // All
    let p = embassy_rp::init(Default::default());
    // \All
    // USB
    let driver = Driver::new(p.USB, Irqs);
    spawner.spawn(logger_task(driver)).unwrap();
    // \USB
    info!("running...");
    // WIFI + LED
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");
    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(&mut pio.common, pio.sm0, pio.irq0, cs, p.PIN_24, p.PIN_29, p.DMA_CH0);
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    unwrap!(spawner.spawn(wifi_task(runner)));
    info!("setting up...");
    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;
    let config = Config::dhcpv4(Default::default());
    let seed = 0x0123_4567_89ab_cdef;
    static STACK: StaticCell<Stack<cyw43::NetDriver<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<2>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        net_device,
        config,
        RESOURCES.init(StackResources::<2>::new()),
        seed,
    ));
    unwrap!(spawner.spawn(net_task(stack)));
    info!("ready");
    loop {
        match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
            Ok(_) => break,
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
    }
    info!("waiting for DHCP...");
    while !stack.is_config_up() {
        Timer::after_millis(100).await;
    }
    let address = stack.config_v4().unwrap().address.address();
    info!("Listening for TCP on {}:{}...", address, PORT);
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut buf = [0; 4096];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(60)));
        control.gpio_set(0, false).await;
        if let Err(e) = socket.accept(PORT).await {
            warn!("accept error: {:?}", e);
            continue;
        }
        info!("Received connection from {:?}", socket.remote_endpoint());
        control.gpio_set(0, true).await;
        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    warn!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!("read error: {:?}", e);
                    break;
                }
            };
            let message_bytes = &buf[..n];
            let message = from_utf8(message_bytes).unwrap();
            let mut has_error = false;
            info!("message: {:?}", message);
            for character in message.chars() {
                match character {
                    '.' => dit(&mut control).await,
                    '_' => dah(&mut control).await,
                    '+' => Timer::after(Duration::from_millis(MEDIUM_DURATION)).await,  // This is a space between letters.
                    '*' => Timer::after(Duration::from_millis(STANDARD_DURATION*7)).await,  // This is a space between words.
                    _ =>  {
                        warn!("unknown character: {:?}", &character);
                        match socket.write_all(ERROR_RESPONSE_BYTES).await {
                            Ok(()) => {}
                            Err(error) => {
                                warn!("write error: {:?}", error);
                            }
                        };
                        match socket.write_all(&[character as u8]).await {
                            Ok(()) => {}
                            Err(error) => {
                                warn!("write error: {:?}", error);
                            }
                        };
                        match socket.write_all("\r\n\r\n".as_bytes()).await {
                            Ok(()) => {}
                            Err(error) => {
                                warn!("write error: {:?}", error);
                            }
                        };
                        has_error = true;
                        break;
                    }
                }
            }
            if !has_error {
                match socket.write_all(RESPONSE_BYTES).await {
                    Ok(()) => {}
                    Err(error) => {
                        warn!("write error: {:?}", error);
                    }
                };
            }
        }
    }
    // \WIFI + LED
}
