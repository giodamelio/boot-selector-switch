use core::fmt::Write;
use log::{Level, Log, Metadata, Record};
use uefi::proto::console::text::Color;
use uefi::system;

static LOGGER: Logger = Logger;

pub fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

struct Logger;

impl Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        system::with_stdout(|stdout| match record.level() {
            Level::Error => {
                let _ = stdout.set_color(Color::LightRed, Color::Black);
                let _ = writeln!(stdout, "ERROR: {}", record.args());
                let _ = stdout.set_color(Color::LightGray, Color::Black);
            }
            Level::Warn => {
                let _ = stdout.set_color(Color::Yellow, Color::Black);
                let _ = writeln!(stdout, "WARN: {}", record.args());
                let _ = stdout.set_color(Color::LightGray, Color::Black);
            }
            Level::Info => {
                let _ = writeln!(stdout, "{}", record.args());
            }
            Level::Debug => {
                let _ = stdout.set_color(Color::Cyan, Color::Black);
                let _ = writeln!(stdout, "DEBUG: {}", record.args());
                let _ = stdout.set_color(Color::LightGray, Color::Black);
            }
            Level::Trace => {
                let _ = stdout.set_color(Color::DarkGray, Color::Black);
                let _ = writeln!(stdout, "TRACE: {}", record.args());
                let _ = stdout.set_color(Color::LightGray, Color::Black);
            }
        });
    }

    fn flush(&self) {}
}
