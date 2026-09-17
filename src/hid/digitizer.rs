use hidapi::{HidApi, HidDevice};
use hidreport::{Report, ReportDescriptor, ReportId};
use hut::Usage;

use crate::{error::SlimpendError, hid::find_device_by_usage};

pub(crate) struct DigitizerDevice {
    hid_device: HidDevice,
    report_desc: ReportDescriptor,
}

pub(crate) enum DigitizerInput {
    Touchscreen,
}

impl DigitizerDevice {
    pub(crate) fn try_new(hid_api: &HidApi) -> Result<Self, SlimpendError> {
        let Ok(Some((hid_device, report_desc))) =
            find_device_by_usage(hid_api, |usage| match usage {
                Usage::Digitizers(d) => match d {
                    hut::Digitizers::Eraser => true,
                    _ => false,
                },
                _ => false,
            })
        else {
            return Err(SlimpendError::DeviceNotFound("Digitzer"));
        };
        println!(
            "{:?}",
            report_desc
                .input_reports()
                .iter()
                .find_map(|r| {
                    if let Some(id) = r.report_id()
                        && u8::from(id) == 18
                    {
                        Some(r.fields())
                    } else {
                        None
                    }
                })
                .unwrap(),
        );
        Ok(DigitizerDevice {
            hid_device,
            report_desc,
        })
    }

    pub(crate) fn read(&self) -> Result<Vec<u8>, hidapi::HidError> {
        let mut data = [0u8; 8192];
        let len = self.hid_device.read(&mut data)?;
        Ok(Vec::from(&data[0..len]))
    }
}
