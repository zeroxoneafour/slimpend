use std::{
    error::Error,
    thread::sleep,
    time::{Duration, SystemTime},
};

use crate::{
    bluez::{find_slim_pen_bt, monitor_connected, monitor_disconnected},
    buzz::{BuzzError, Waveform, buzz, hid_connect},
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
    #[arg(
        short = 'a',
        long,
        help = "Pen bluetooth name/alias",
        default_value_t = String::from("Surface Slim Pen 2"),
        global = true
    )]
    pen_alias: String,
    #[arg(
        short,
        long,
        help = "Waveform (between 0 and 3 inclusive)",
        default_value_t = 2,
        global = true
    )]
    waveform: u8,
    #[arg(
        short,
        long,
        help = "Intensity multiplier",
        default_value_t = 1.0,
        global = true
    )]
    intensity: f64,
    #[arg(
        short,
        long,
        help = "Try to keep the pen connection alive (not working rn and causes lag)",
        default_value_t = false,
        global = true
    )]
    keep_alive: bool,
    #[arg(
        long,
        alias = "ps",
        help = "Use square root curve for pressure sensitivity. Alias: --ps",
        global = true
    )]
    pressure_sqrt: bool,
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
            let dev = hid_connect(None)?;
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
        let addr = bt_device.address().to_string().to_ascii_lowercase();
        // sometimes takes many tries to connect
        let mut conn_tries = 1;
        let try_connect_res = loop {
            let Ok(Some(ret)) = try_connect_hid(&addr) else {
                if conn_tries > 100 {
                    break None;
                }
                conn_tries += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            break Some(ret);
        };
        let Some((hid_dev, ev_dev)) = try_connect_res else {
            println!(
                "Failed to connect to Slim Pen 2 (addr {}) after 100 tries",
                addr
            );
            println!("Make sure you are in input group and IPTSD is enabled");
            break Err(Box::new(BuzzError::DeviceNotFound));
        };
        println!(
            "Slim Pen 2 (addr {}) connected! (Took {} tries)",
            addr, conn_tries
        );
        if let Err(e) = tokio::select!(
            v = main_loop(&cli, &hid_dev, ev_dev) => { v },
            v = monitor_disconnected(&bt_device) => { match v {
                Ok(_) => Ok(()),
                Err(e) => Err(Box::new(e).into())
            } },
            v = keep_device_alive(&bt_device, cli.keep_alive) => { v }
        ) {
            println!("Ran into error - {}, restarting loop", e);
        } else {
            println!("Slim Pen 2 (addr {}) disconnected!", addr);
        }
    }
}

fn try_connect_hid(addr: &str) -> Result<Option<(HidDevice, ::evdev::Device)>, Box<dyn Error>> {
    let hid_dev = hid_connect(Some(&addr))?;
    let Some(ev_dev) = open_evdev() else {
        println!("No evdev device found matching \"IPTS Virtual Stylus\"");
        return Ok(None);
    };
    return Ok(Some((hid_dev, ev_dev)));
}

async fn main_loop(
    cli: &Cli,
    hid_dev: &HidDevice,
    ev_dev: ::evdev::Device,
) -> Result<(), Box<dyn Error>> {
    buzz(&hid_dev, 127, Waveform::LongDoubleBuzz)?;
    let mut ev_stream = ev_dev.into_event_stream()?;

    let mut old_x = 0;
    let mut old_y = 0;
    let mut x = 0;
    let mut y = 0;
    let mut btn_touch = false;
    let mut eraser = false;
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
                KeyCode::BTN_TOOL_RUBBER => {
                    eraser = !(value == 0);
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

                if eraser {
                    vib *= 0.25;
                } else {
                    let mut pressure_coeff = pressure as f64 / 4096.0;
                    if cli.pressure_sqrt {
                        pressure_coeff = pressure_coeff.sqrt();
                    }
                    vib *= pressure_coeff;
                }

                vib *= cli.intensity;

                if vib < 5.0 {
                    continue;
                }

                vib = vib.clamp(0.0, 255.0);

                buzz(&hid_dev, vib as u8, Waveform::Buzz(cli.waveform))?;

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
