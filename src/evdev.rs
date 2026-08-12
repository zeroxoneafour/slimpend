use evdev::Device;

pub fn open_evdev() -> Option<Device> {
    for (_, device) in evdev::enumerate() {
        if let Some(name) = device.name()
            && name.contains("IPTSD Virtual Stylus")
        {
            return Some(device);
        }
    }
    return None;
}
