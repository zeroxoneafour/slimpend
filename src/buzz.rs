use hidapi::{HidApi, HidDevice};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum BuzzError {
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
 * Values 3-6 appear to be buzzes (in decreasing duration)
 * Values 7/8 appear to be double buzzes
 * Value 17 is a very long double buzz
 * Values 18-22 are shorter double buzzes (similar to first double buzz)
 * Value 0 is buzz as well, but is highly inconsistent.
 */
#[repr(u8)]
pub enum Waveform {
    Buzz(u8),
    DoubleBuzz(u8),
    LongDoubleBuzz,
}

impl TryFrom<Waveform> for u8 {
    type Error = BuzzError;
    fn try_from(value: Waveform) -> Result<u8, BuzzError> {
        match value {
            Waveform::Buzz(n) => {
                if n > 3 {
                    return Err(BuzzError::InvalidWaveform);
                }
                Ok(n + 3)
            }
            Waveform::DoubleBuzz(n) => {
                if n > 6 {
                    return Err(BuzzError::InvalidWaveform);
                }
                if n < 2 { Ok(n + 7) } else { Ok(n + 16) }
            }
            Waveform::LongDoubleBuzz => Ok(17),
        }
    }
}

pub fn hid_connect(addr: Option<&str>) -> Result<HidDevice, Box<dyn Error>> {
    let api = HidApi::new()?;
    let Some(dev_info) = api.device_list().find(|d| {
        d.vendor_id() == 0x045e
            && d.product_id() == 0x0c0f
            && (addr == None || d.serial_number() == addr)
    }) else {
        return Err(Box::new(BuzzError::DeviceNotFound));
    };
    let dev = dev_info.open_device(&api)?;
    Ok(dev)
}

pub fn buzz(dev: &HidDevice, intensity: u8, waveform: Waveform) -> Result<(), Box<dyn Error>> {
    // https://github.com/linux-surface/linux-surface/issues/1066
    // [65, (repeat), (intensity), (waveform), (cutoff), (major byte of retrigger interval), (minor byte of retrigger interval)]
    // not really known what anything after waveform does so we js dont send it
    let _ = dev.write(&[65, 1, intensity, waveform.try_into()?])?;
    Ok(())
}
