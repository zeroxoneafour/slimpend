use clap::ValueEnum;
use hex_literal::hex;
use hidapi::{HidApi, HidDevice};
use std::{collections::HashMap, error::Error, fmt, time::Duration};

#[derive(Debug)]
pub enum BuzzError {
    DeviceNotFound,
}

impl fmt::Display for BuzzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuzzError::DeviceNotFound => {
                write!(f, "Surface Slim Pen 2 not found!")
            }
        }
    }
}
impl Error for BuzzError {}

/*
 * https://learn.microsoft.com/en-us/surface/surface-slim-pen2-haptics-dev-notes
 * https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/haptic-pen-implementation-guide
 * Values 3-6 appear to be buzzes (in decreasing duration)
 * Values 7/8 appear to be double buzzes
 * Value 17 is a very long double buzz
 * Values 18-22 are shorter double buzzes (similar to first double buzz)
 * Value 0 is buzz as well, but is highly inconsistent.
 */
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum, Default, Hash)]
pub enum Waveform {
    // 0x1001-0x1003
    // Required codes
    None = 0x01,
    Stop = 0x02,
    Click = 0x03,
    // 0x1006 - 0x100A
    // Optional codes
    Press = 0x04,
    #[default]
    Release = 0x05,
    Hover = 0x06,
    Success = 0x07,
    Error = 0x08,
    // 0x100B - 0x1011
    // "Continuous" (not working on linux)
    InkCont = 0x09,
    PencilCont = 0x0a,
    MarkerCont = 0x0b,
    ChiselMarkerCont = 0x0c,
    BrushCont = 0x0d,
    EraserCont = 0x0e,
    SparkleCont = 0x0f,
    // 0x1012-0x1015
    // Interrupting (no duration)
    Collide = 0x10,
    Align = 0x11,
    Step = 0x12,
    Grow = 0x13,
}

impl Waveform {
    pub fn all() -> [Waveform; 6] {
        [
            Waveform::Click,
            Waveform::Press,
            Waveform::Release,
            Waveform::Hover,
            Waveform::Success,
            Waveform::Error,
        ]
    }
}

pub struct BuzzDevice {
    hid_device: HidDevice,
    durations: HashMap<u8, u16>,
}

impl BuzzDevice {
    pub fn new(mac_addr: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let api = HidApi::new()?;
        let Some(dev_info) = api.device_list().find(|d| {
            d.vendor_id() == 0x045e
                && d.product_id() == 0x0c0f
                && (mac_addr == None || d.serial_number() == mac_addr)
                // filter for digitizer hid api
                && d.usage_page() == 0x0D
        }) else {
            return Err(Box::new(BuzzError::DeviceNotFound));
        };
        let hid_device = dev_info.open_device(&api)?;

        let mut waveforms_buf = [0u8; 512];
        waveforms_buf[0] = 0x42;
        let waveforms_len = hid_device.get_feature_report(&mut waveforms_buf)?;
        let waveforms: Vec<u16> = waveforms_buf[4..waveforms_len + 1]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes(chunk.try_into().unwrap()))
            .collect();
        let (durations_nums, waveform_ids) = waveforms.split_at(waveforms.len() / 2);
        let mut durations = HashMap::new();
        for i in 0..durations_nums.len() {
            if waveform_ids[i] < 0x1003 {
                continue;
            }
            durations.insert((waveform_ids[i] - 0x1003) as u8, durations_nums[i]);
        }
        println!("waveform durations - {:?}", durations);

        //initialize_device(&hid_device)?;

        Ok(BuzzDevice {
            hid_device,
            durations,
        })
    }

    // returns the amount of time to wait for
    pub fn buzz(&self, intensity: u8, waveform: Waveform) -> Result<Duration, Box<dyn Error>> {
        // https://github.com/linux-surface/linux-surface/issues/1066
        // [65, (repeat), (intensity), (waveform), (cutoff), (major byte of retrigger interval), (minor byte of retrigger interval)]
        // not really known what anything after waveform does so we js dont send it
        let _ = self
            .hid_device
            .write(&[65, 0, intensity, waveform as u8, 0, 0, 0])?;
        if let Some(duration) = self.durations.get(&(waveform as u8)) {
            Ok(Duration::from_millis(*duration as u64))
        } else {
            Ok(Duration::from_millis(0))
        }
    }
}

pub fn initialize_device(dev: &HidDevice) -> Result<(), Box<dyn Error>> {
    let mut chars = [0u8; 61];
    chars[0] = 42;
    let hex_writes = [
        hex!("0000ffa0000000000000000000000000"),
        hex!("0100ffa0000000000000000000000000"),
        hex!("000011a08b9600050100000014054300"),
        hex!("000011a08b9600050100000004054300"),
    ];
    for write in hex_writes {
        chars[1..write.len() + 1].copy_from_slice(&write);
        dev.write(&chars)?;
    }
    Ok(())
}
