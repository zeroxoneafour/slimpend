use std::{
    error::Error,
    thread::sleep,
    time::{Duration, SystemTime},
};

use crate::{
    bluez::{find_slim_pen_bt, monitor_connected, monitor_disconnected},
    buzz::{Waveform, buzz, hid_connect},
    evdev::open_evdev,
};
use ::evdev::{AbsoluteAxisCode, EventSummary, KeyCode};
use clap::{Parser, Subcommand};
use hidapi::HidDevice;

mod bluez;
mod buzz;
mod evdev;

#[derive(Parser)]
struct Cli {
    #[arg(short = 'a', long, help = "Pen bluetooth name", default_value_t = String::from("Surface Slim Pen 2"))]
    pen_alias: String,
    #[arg(
        short,
        long,
        help = "Waveform (between 0 and 3 inclusive)",
        default_value_t = 3
    )]
    waveform: u8,
    #[arg(short, long, help = "Intensity multiplier", default_value_t = 1.0)]
    intensity: f64,
    #[arg(
        short,
        long,
        help = "Try to keep the pen connection alive (not working rn and causes lag)",
        default_value_t = false
    )]
    keep_alive: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Buzz your pen for debug purposes
    Buzz,
    /// Start the daemon (if no command provided, this is default)
    Serve,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Buzz) => {
            let dev = hid_connect()?;
            for wf in 0u8..4 {
                println!("testing buzz waveform {}", wf);
                buzz(&dev, 255, Waveform::Buzz(wf))?;
                sleep(Duration::from_millis(500));
            }
            for wf in 0u8..7 {
                println!("testing double buzz waveform {}", wf);
                buzz(&dev, 255, Waveform::DoubleBuzz(wf))?;
                sleep(Duration::from_millis(500));
            }
            println!("testing long double buzz waveform");
            buzz(&dev, 255, Waveform::LongDoubleBuzz)?;
            sleep(Duration::from_millis(500));
            Ok(())
        }
        Some(Commands::Serve) => server(&cli).await,
        None => {
            println!("No command found, defaulting to server mode");
            server(&cli).await
        }
    }
}

async fn server(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let Some(bt_device) = find_slim_pen_bt(&cli.pen_alias).await? else {
        println!("Please pair your slim pen 2 with bluetooth (or specify --pen-alias)!");
        return Ok(());
    };
    loop {
        if !bt_device.is_connected().await? {
            monitor_connected(&bt_device).await?;
        }
        // wait a second to let hid create shi
        tokio::time::sleep(Duration::from_millis(100)).await;
        let hid_dev = hid_connect()?;
        println!("Slim pen 2 connected!");
        buzz(&hid_dev, 127, Waveform::LongDoubleBuzz)?;
        tokio::select!(
            v = main_loop(&hid_dev, &cli) => { v? },
            v = monitor_disconnected(&bt_device) => { v? },
            v = keep_device_alive(&bt_device, cli.keep_alive) => { v? }
        );
        println!("Slim pen 2 disconnected!");
    }
}

async fn main_loop(hid_dev: &HidDevice, cli: &Cli) -> Result<(), Box<dyn Error>> {
    let Some(ev_device) = open_evdev() else {
        println!("No evdev device found matching \"IPTS Virtual Stylus\"");
        return Ok(());
    };
    let mut ev_stream = ev_device.into_event_stream()?;

    let mut old_x = 0;
    let mut old_y = 0;
    let mut x = 0;
    let mut y = 0;
    let mut btn_touch = false;
    let mut btn_touch_justpressed = false;
    let mut pressure = 0;
    let mut last_timestamp = SystemTime::now();

    while let Ok(ev) = ev_stream.next_event().await {
        match ev.destructure() {
            EventSummary::AbsoluteAxis(_, code, value) => match code {
                AbsoluteAxisCode::ABS_X => {
                    x = value;
                }
                AbsoluteAxisCode::ABS_Y => {
                    y = value;
                }
                AbsoluteAxisCode::ABS_PRESSURE => {
                    pressure = value;
                }
                _ => {}
            },
            EventSummary::Key(_, key, value) => match key {
                KeyCode::BTN_TOUCH => {
                    btn_touch = !(value == 0);
                    if btn_touch {
                        btn_touch_justpressed = true;
                        // resync old_x and old_y on touch
                        old_x = x;
                        old_y = y;
                    }
                }
                _ => {}
            },
            EventSummary::Synchronization(sync, _, _) => {
                if !btn_touch {
                    continue;
                }

                let timestamp = sync.timestamp();
                // 20 ms is sufficient time for buzz(0-3) to complete
                // adding more buzzes before this time causes a backlog
                // leading to buzzing after picking up pen
                if timestamp.duration_since(last_timestamp)? < Duration::from_millis(20) {
                    continue;
                }

                let delta_x = x - old_x;
                old_x = x;
                let delta_y = y - old_y;
                old_y = y;

                let mut vib = if btn_touch_justpressed {
                    btn_touch_justpressed = false;
                    // small buzz on pen touch
                    96.0
                } else {
                    // apply a sqrt transform after dividing dist by 1024, then renormalize to 256
                    // this helps gain a bit of vib at lower velocities
                    (((delta_x.pow(2) + delta_y.pow(2)) as f64).sqrt() / 1024.0).sqrt() * 256.0
                };

                // use square root scaling for pressure as well (nvm no we dont)
                let pressure_coeff = pressure as f64 / 4096.0;
                vib *= pressure_coeff;

                vib *= cli.intensity;

                if vib < 5.0 {
                    continue;
                }

                vib = vib.clamp(0.0, 255.0);

                buzz(hid_dev, vib as u8, Waveform::Buzz(cli.waveform))?;

                last_timestamp = timestamp;
            }
            _ => {}
        };
    }
    Ok(())
}

// ping loop that reads all GATT descriptors to keep the connection alive
// does not work right now
async fn keep_device_alive(bt_dev: &bluer::Device, keep_alive: bool) -> Result<(), Box<dyn Error>> {
    loop {
        if keep_alive {
            let services = bt_dev.services().await?;
            for service in services {
                let characteristics = service.characteristics().await?;
                for characteristic in characteristics {
                    let descriptors = characteristic.descriptors().await?;
                    for descriptor in descriptors {
                        descriptor.read().await?;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
