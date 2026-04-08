// try_slint.rs - Slint UI on 1.54" ST7789 via SPI on Raspberry Pi CM0 (Linux)
//
// Wiring (same as embassy_f4_slint project):
// - LCD VCC  -> 3.3V
// - LCD GND  -> GND
// - LCD SCL  -> SPI SCLK (GPIO 11)
// - LCD SDA  -> SPI MOSI (GPIO 10)
// - LCD CS   -> SPI CS0  (GPIO 8)
// - LCD DC   -> GPIO 24
// - LCD RST  -> GPIO 25
// - LCD BL   -> GPIO 23 (backlight)

use std::error::Error;
use std::process::Command;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::RawU16;
use embedded_graphics_core::prelude::*;
use embedded_graphics_core::primitives::Rectangle;
use embedded_hal_bus::spi::ExclusiveDevice;
use mipidsi::interface::SpiInterface;
use cm0::gpio::Gpio;
use cm0::hal::Delay;
use cm0::spi::{Bus, Mode, SlaveSelect, Spi};
use slint::platform::software_renderer::{LineBufferProvider, MinimalSoftwareWindow, Rgb565Pixel};

const SCREEN_WIDTH: u16 = 240;
const SCREEN_HEIGHT: u16 = 240;

// Pin definitions (same as embassy_f4_slint)
const PIN_DC: u8 = 24;
const PIN_RST: u8 = 25;
const PIN_BL: u8 = 23;

slint::include_modules!();

// --- Slint Platform ---
struct MyPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl slint::platform::Platform for MyPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        core::time::Duration::ZERO
    }
}

// --- Display wrapper for Slint line-by-line rendering ---
struct DisplayWrapper<'a, T> {
    display: &'a mut T,
    line_buffer: &'a mut [Rgb565Pixel],
}

impl<T: DrawTarget<Color = Rgb565>> LineBufferProvider for DisplayWrapper<'_, T> {
    type TargetPixel = Rgb565Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        render_fn(&mut self.line_buffer[range.clone()]);

        let rect = Rectangle::new(
            Point::new(range.start as i32, line as i32),
            Size::new(range.len() as u32, 1),
        );

        self.display
            .fill_contiguous(
                &rect,
                self.line_buffer[range]
                    .iter()
                    .map(|p| RawU16::new(p.0).into()),
            )
            .map_err(drop)
            .unwrap();
    }
}

fn get_local_time() -> String {
    let output = Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .expect("Failed to run date command");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Slint on ST7789 1.54\" via SPI (CM0 Linux)");
    println!("SCL -> GPIO 11, SDA -> GPIO 10, CS -> GPIO 8");
    println!("DC -> GPIO 24, RST -> GPIO 25, BL -> GPIO 23");
    println!("Press Ctrl+C to exit\n");

    // Initialize GPIO
    let gpio = Gpio::new()?;
    let dc_pin = gpio.get(PIN_DC)?.into_output();
    let rst_pin = gpio.get(PIN_RST)?.into_output();
    let mut bl_pin = gpio.get(PIN_BL)?.into_output();

    // Initialize SPI (20MHz, Mode 3 - same as embassy project)
    let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 20_000_000, Mode::Mode3)?;

    // Create SPI device with CS pin
    let cs_pin = gpio.get(8)?.into_output();
    let spi_device = ExclusiveDevice::new_no_delay(spi, cs_pin)?;

    // SPI transfer buffer for mipidsi (same as embassy: 960 bytes)
    let mut spi_buf = [0u8; 960]; // 240 pixels * 2 bytes * 2
    let di = SpiInterface::new(spi_device, dc_pin, &mut spi_buf);

    // Initialize ST7789 via mipidsi (exact same config as embassy)
    let mut display = mipidsi::Builder::new(
        mipidsi::models::ST7789,
        di
    )
    .display_size(240, 240)
    .display_offset(0, 0)
    .orientation(mipidsi::options::Orientation::new().rotate(mipidsi::options::Rotation::Deg0))
    .invert_colors(mipidsi::options::ColorInversion::Inverted)
    .reset_pin(rst_pin)
    .init(&mut Delay)
    .expect("Failed to init display");

    bl_pin.set_high();
    println!("ST7789 display initialized");

    // Clear display
    display.clear(Rgb565::BLACK).unwrap();

    // Setup Slint platform
    let window = MinimalSoftwareWindow::new(Default::default());
    window.set_size(slint::PhysicalSize::new(
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    ));

    slint::platform::set_platform(Box::new(MyPlatform { window: window.clone() }))
        .unwrap();

    // Create Slint UI (uses same .slint file from embassy_f4_slint)
    let ui = AppWindow::new().expect("Failed to load UI");
    println!("Slint UI created");

    // Rendering loop (same pattern as embassy)
    let mut line_buffer = [Rgb565Pixel(0); SCREEN_WIDTH as usize];
    let mut last_time = String::new();

    loop {
        slint::platform::update_timers_and_animations();

        let time_str = get_local_time();
        if time_str != last_time {
            last_time = time_str.clone();
            ui.set_clock_text(time_str.as_str().into());
        }

        // Render frame
        window.draw_if_needed(|renderer| {
            renderer.render_by_line(DisplayWrapper {
                display: &mut display,
                line_buffer: &mut line_buffer,
            });
        });

        if !window.has_active_animations() {
            thread::sleep(Duration::from_millis(33));
        }
    }
}
