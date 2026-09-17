use std::{
    error::Error,
    ffi::{CStr, CString},
    time::{Duration, SystemTime},
};

use crate::{
    bluez::{find_slim_pen_bt, monitor_connected, monitor_disconnected},
    hid::digitizer::DigitizerDevice,
};
use clap::{Parser, Subcommand};
use evdev::{AbsoluteAxisCode, EventSummary, KeyCode};
use hidapi::HidApi;
use hidreport::{ArrayField, ConstantField, Field, Report, ReportDescriptor, VariableField};
use hut::Usage;

mod bluez;
mod error;
mod hid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut hid_api = HidApi::new()?;
    hid_api.refresh_devices()?;
    let digitizer = DigitizerDevice::try_new(&hid_api)?;
    loop {
        println!("Read - {:?}", &digitizer.read()?);
    }
    //Ok(())
}
