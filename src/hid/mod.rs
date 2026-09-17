use std::{error::Error, ffi::CString};

use hidapi::{HidApi, HidDevice};
use hidreport::{Field, Report, ReportDescriptor};
use hut::Usage;

pub(crate) mod digitizer;
mod pen;

fn find_device_by_usage<F>(
    hid_api: &HidApi,
    filter: F,
) -> Result<Option<(HidDevice, ReportDescriptor)>, Box<dyn Error>>
where
    F: Fn(Usage) -> bool,
{
    let all_devices: Vec<CString> = glob::glob("/dev/hidraw*")?
        .filter_map(Result::ok)
        .map(|path| CString::new(path.into_os_string().as_encoded_bytes()))
        .filter_map(Result::ok)
        .collect();
    Ok(all_devices.iter().find_map(|cstr| {
        let Ok(dev) = hid_api.open_path(&cstr) else {
            return None;
        };
        let mut report_desc = [0u8; 65536];
        let Ok(report_len) = dev.get_report_descriptor(&mut report_desc) else {
            return None;
        };
        let Ok(parsed) = ReportDescriptor::try_from(&report_desc[0..report_len]) else {
            return None;
        };
        if parsed.input_reports().iter().any(|report| {
            report.fields().iter().any(|field| {
                let usages = match field {
                    Field::Array(arr) => arr.usages(),
                    Field::Variable(var) => &[var.usage],
                    Field::Constant(c) => c.usages(),
                };
                usages.iter().any(|usage| {
                    let Ok(hut_usage) = hut::Usage::new_from_page_and_id(
                        usage.usage_page.into(),
                        usage.usage_id.into(),
                    ) else {
                        return false;
                    };
                    filter(hut_usage)
                })
            })
        }) {
            Some((dev, parsed))
        } else {
            None
        }
    }))
}
