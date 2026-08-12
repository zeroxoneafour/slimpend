use bluer::{Device, Error};
use tokio_stream::StreamExt;

pub async fn find_slim_pen_bt() -> Result<Option<Device>, Error> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    for addr in adapter.device_addresses().await? {
        let Ok(device) = adapter.device(addr) else {
            continue;
        };
        let Ok(Some(device_name)) = device.name().await else {
            continue;
        };
        if device_name == "Surface Slim Pen 2" {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

pub async fn monitor_connected(dev: &Device) -> Result<(), Error> {
    let mut ev_stream = dev.events().await?;
    while let Some(event) = ev_stream.next().await {
        let bluer::DeviceEvent::PropertyChanged(prop) = event;
        if let bluer::DeviceProperty::Connected(connected) = prop
            && connected
        {
            return Ok(());
        }
    }
    return Ok(());
}

pub async fn monitor_disconnected(dev: &Device) -> Result<(), Error> {
    let mut ev_stream = dev.events().await?;
    while let Some(event) = ev_stream.next().await {
        let bluer::DeviceEvent::PropertyChanged(prop) = event;
        if let bluer::DeviceProperty::Connected(connected) = prop
            && !connected
        {
            return Ok(());
        }
    }
    return Ok(());
}
