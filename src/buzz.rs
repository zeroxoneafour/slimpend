use hex_literal::hex;
use hidapi::{HidApi, HidDevice};
use std::{collections::HashMap, error::Error, fmt, hash::Hash, time::Duration};

#[derive(Debug)]
pub enum BuzzError {
    DeviceNotFound,
    InvalidWaveform,
}

impl fmt::Display for BuzzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuzzError::DeviceNotFound => {
                write!(f, "Surface Slim Pen 2 not found")
            }
            BuzzError::InvalidWaveform => {
                write!(f, "Waveform is invalid")
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
#[repr(u16)]
#[derive(Copy, Clone, Hash)]
#[allow(dead_code)]
pub enum Waveform {
    // 0x1001-0x1003
    // Required codes
    None = 0x1001,
    Stop = 0x1002,
    Click = 0x1003,
    // 0x1006 - 0x100A
    // Optional codes
    Press = 0x1006,
    Release = 0x1007,
    Hover = 0x1008,
    Success = 0x1009,
    Error = 0x100A,
    // 0x100B - 0x1011
    // "Continuous" (not working on linux)
    InkCont = 0x100B,
    PencilCont = 0x100C,
    MarkerCont = 0x100D,
    ChiselMarkerCont = 0x100E,
    BrushCont = 0x100F,
    EraserCont = 0x1010,
    SparkleCont = 0x1011,
    // 0x1012-0x1015
    // Interrupting (no duration)
    Collide = 0x1012,
    Align = 0x1013,
    Step = 0x1014,
    Grow = 0x1015,
}

pub struct BuzzDevice {
    hid_device: HidDevice,
    ordinals: HashMap<u16, u8>,
    durations: HashMap<u16, u16>,
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
        let mut ordinals = HashMap::new();
        // first num in array is guaranteed to be duration for click,
        // and will also establish the base ordinal (in this case 3)
        // so the ordinal for waveform_ids[1] is 4, etc.
        // the ordinal is passed instead of the waveform id in calls to buzz
        let ordinal_base = waveform_ids[0] as u8;
        ordinals.insert(0x1001, 1);
        ordinals.insert(0x1002, 2);
        ordinals.insert(0x1003, ordinal_base);
        durations.insert(0x1003, durations_nums[0]);
        for i in 1..durations_nums.len() {
            let wf_id = waveform_ids[i];
            if wf_id < 0x1003 {
                continue;
            }
            ordinals.insert(wf_id, i as u8 + ordinal_base);
            durations.insert(wf_id, durations_nums[i]);
        }

        //initialize_device(&hid_device)?;

        Ok(BuzzDevice {
            hid_device,
            ordinals,
            durations,
        })
    }

    // returns the amount of time to wait for
    pub fn buzz(&self, intensity: u8, waveform: Waveform) -> Result<Duration, Box<dyn Error>> {
        let wf_u16 = waveform as u16;
        let ordinal = self.ordinals.get(&wf_u16).ok_or("Failed to get ordinal")?;
        // https://github.com/linux-surface/linux-surface/issues/1066
        // [65, (repeat), (intensity), (waveform), (cutoff), (major byte of retrigger interval), (minor byte of retrigger interval)]
        // not really known what anything after waveform does so we js dont send it
        let _ = self
            .hid_device
            .write(&[65, 1, intensity, *ordinal, 0, 0, 0])?;
        if let Some(duration) = self.durations.get(&wf_u16) {
            Ok(Duration::from_millis(*duration as u64))
        } else {
            Ok(Duration::from_millis(20))
        }
    }

    // not working right now
    #[allow(dead_code)]
    pub fn buzz_cont(&self, intensity: u8, waveform: Waveform) -> Result<(), Box<dyn Error>> {
        let wf_u16 = waveform as u16;
        let ordinal = self.ordinals.get(&wf_u16).ok_or("Failed to get ordinal")?;
        self.hid_device
            .send_feature_report(&[65, 0, intensity, *ordinal, 0xD0, 0x05])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn dump_feature_reports(&self) -> Vec<Vec<u8>> {
        let mut ret = Vec::new();
        let mut buf = [0u8; 256];
        for i in 0u8..255 {
            buf[0] = i;
            if let Ok(len) = self.hid_device.get_feature_report(&mut buf) {
                ret.push(buf[0..len + 1].to_vec());
            }
        }
        ret
    }
}

#[allow(dead_code)]
fn initialize_device(dev: &HidDevice) -> Result<(), Box<dyn Error>> {
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
