use alloc::format;
use alloc::string::String;

use log::{debug, warn};
use uefi::boot;
use uefi::proto::media::file::{File, FileAttribute, FileMode};
use uefi::{CString16, cstr16};

const CONFIG_PATH: &uefi::CStr16 = cstr16!("\\EFI\\boot-selector-switch\\config.conf");

/// Parsed configuration from the config file.
/// Maps switch positions (1-6) to systemd-boot entry filenames.
pub struct Config {
    /// entries[i] is the boot entry for position (i+1).
    /// None = unmapped position.
    entries: [Option<CString16>; 6],
}

impl Config {
    pub fn empty() -> Self {
        Self {
            entries: [const { None }; 6],
        }
    }

    pub fn get_entry(&self, position: u8) -> Option<&CString16> {
        if position >= 1 && position <= 6 {
            self.entries[(position - 1) as usize].as_ref()
        } else {
            None
        }
    }
}

/// Read and parse the config file from the ESP.
pub fn load_config() -> Result<Config, String> {
    let mut fs = boot::get_image_file_system(boot::image_handle())
        .map_err(|e| format!("Could not open filesystem: {:?}", e))?;

    let mut root = fs
        .open_volume()
        .map_err(|e| format!("Could not open volume: {:?}", e))?;

    let handle = root
        .open(CONFIG_PATH, FileMode::Read, FileAttribute::empty())
        .map_err(|e| format!("Could not open config file: {:?}", e))?;

    let mut file = handle
        .into_regular_file()
        .ok_or_else(|| String::from("Config path is not a regular file"))?;

    let mut buf = [0u8; 512];
    let bytes_read = file
        .read(&mut buf)
        .map_err(|e| format!("Could not read config file: {:?}", e))?;

    parse_config(&buf[..bytes_read])
}

/// Parse config file contents into a Config struct.
fn parse_config(data: &[u8]) -> Result<Config, String> {
    let mut config = Config::empty();

    let text =
        core::str::from_utf8(data).map_err(|_| String::from("Config file is not valid UTF-8"))?;

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((pos_str, value)) = line.split_once('=') else {
            warn!("Ignoring malformed config line: {}", line);
            continue;
        };

        let pos_str = pos_str.trim();
        let value = value.trim();

        let Ok(pos) = pos_str.parse::<u8>() else {
            warn!("Ignoring non-numeric position: {}", pos_str);
            continue;
        };

        if pos < 1 || pos > 6 {
            warn!("Ignoring out-of-range position: {}", pos);
            continue;
        }

        match CString16::try_from(value) {
            Ok(entry) => {
                debug!("Config: position {} = {}", pos, value);
                config.entries[(pos - 1) as usize] = Some(entry);
            }
            Err(_) => {
                warn!("Could not convert entry name to UCS-2: {}", value);
            }
        }
    }

    Ok(config)
}
