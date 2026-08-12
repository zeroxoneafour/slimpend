use std::{error::Error, fmt};

use hidapi::{HidApi, HidDevice};

#[derive(Debug)]
enum BuzzError {
    DeviceNotFound,
    InvalidWaveform,
}

impl fmt::Display for BuzzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuzzError::DeviceNotFound => {
                write!(f, "Surface Slim Pen 2 not found!")
            }
            BuzzError::InvalidWaveform => {
                write!(
                    f,
                    "Invalid waveform (waveforms must be between 0 and 5 inclusive)"
                )
            }
        }
    }
}
impl Error for BuzzError {}

/*
 * Values 3-8 appear to be buzzes (in increasing duration)
 * Values 17-22 appear to be taps
 * Value 0 is buzz as well, but is highly inconsistent.
 */
#[repr(u8)]
pub enum Waveform {
    Buzz(u8),
    Tap(u8),
}

pub fn hid_connect() -> Result<HidDevice, Box<dyn Error>> {
    let api = HidApi::new()?;

    let Some(dev_info) = api
        .device_list()
        .find(|d| d.vendor_id() == 0x045e && d.product_id() == 0x0c0f)
    else {
        return Err(Box::new(BuzzError::DeviceNotFound));
    };
    let dev = dev_info.open_device(&api)?;
    Ok(dev)
}

pub fn buzz(dev: &HidDevice, intensity: u8, waveform: Waveform) -> Result<(), Box<dyn Error>> {
    let wf = match waveform {
        Waveform::Buzz(n) => {
            if n > 5 {
                return Err(Box::new(BuzzError::InvalidWaveform));
            }
            n + 3
        }
        Waveform::Tap(n) => {
            if n > 5 {
                return Err(Box::new(BuzzError::InvalidWaveform));
            }
            n + 17
        }
    };
    // https://github.com/linux-surface/linux-surface/issues/1066
    // [65, (repeat), (intensity), (waveform), (cutoff), (major byte of retrigger interval), (minor byte of retrigger interval)]
    // not really known what anything after waveform does so we js dont send it
    let _ = dev.write(&[65, 1, intensity, wf])?;
    Ok(())
}
