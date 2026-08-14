use std::{
    error::Error,
    time::{Duration, SystemTime},
};

use crate::{
    bluez::{find_slim_pen_bt, monitor_connected, monitor_disconnected},
    buzz::{BuzzDevice, BuzzError, Waveform},
};
use clap::{Parser, Subcommand};
use evdev::{AbsoluteAxisCode, EventSummary, KeyCode};

mod bluez;
mod buzz;

#[derive(Parser)]
struct Cli {
    #[arg(
        short = 'n',
        long,
        help = "Pen bluetooth name/alias",
        default_value_t = String::from("Surface Slim Pen 2"),
        global = true
    )]
    pen_alias: String,
    #[arg(
        short,
        long,
        help = "Waveform to use. Values can be integers 0-3 inclusive",
        default_value_t = 0u8,
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
    /*
    #[arg(
        short,
        long,
        help = "Try to keep the pen connection alive (not working rn and causes lag)",
        default_value_t = false,
        global = true
    )]
    keep_alive: bool,
    */
    #[arg(
        short,
        long = "pressure",
        help = "Nth root to apply to pressure multiplier",
        default_value_t = 1.0,
        global = true
    )]
    pressure_root: f64,
    #[arg(
        short,
        long = "distance",
        help = "Nth root to apply to distance multiplier",
        default_value_t = 3.0,
        global = true
    )]
    distance_root: f64,
    #[command(subcommand)]
    command: Option<Commands>,
}

impl TryFrom<u8> for Waveform {
    type Error = BuzzError;
    fn try_from(value: u8) -> Result<Self, BuzzError> {
        match value {
            0 => Ok(Waveform::Click),
            1 => Ok(Waveform::Release),
            2 => Ok(Waveform::Press),
            3 => Ok(Waveform::Hover),
            _ => Err(BuzzError::InvalidWaveform),
        }
    }
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
            let dev = BuzzDevice::new(None)?;
            let res = tokio::select! { v = buzz(&dev, cli.waveform) => v, _ = tokio::signal::ctrl_c() => Ok(()) };
            //println!("Sending stop waveform");
            //dev.buzz(0, Waveform::Stop)?;
            res
        }
        Some(Commands::Serve) | None => {
            if let None = &cli.command {
                println!("No command found, defaulting to serve");
            }
            tokio::select! { v = serve(&cli) => v, _ = tokio::signal::ctrl_c() => Ok(()) }
        }
    }
}

async fn buzz(dev: &BuzzDevice, waveform_option: u8) -> Result<(), Box<dyn Error>> {
    let waveform: Waveform = waveform_option.try_into()?;
    println!("buzzing on waveform 0x{:x}", waveform as u16);
    loop {
        tokio::time::sleep(dev.buzz(255, waveform)?).await;
    }
}

async fn serve(cli: &Cli) -> Result<(), Box<dyn Error>> {
    let Some(bt_device) = find_slim_pen_bt(&cli.pen_alias).await? else {
        println!("Please pair your slim pen 2 with bluetooth (or specify --pen-alias)!");
        return Ok(());
    };
    loop {
        if !bt_device.is_connected().await? {
            monitor_connected(&bt_device).await?;
        }
        let addr = bt_device.address().to_string().to_ascii_lowercase();
        // if it doesnt connect the first time try again as it takes a second for hids to register
        let Ok((buzz_dev, ev_dev)) = try_connect_hid(&addr) else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        println!("Slim Pen 2 ({}) connected!", addr);
        //buzz_dev.buzz_cont(255, Waveform::None)?;
        buzz_dev.buzz(255, Waveform::Error)?;
        if let Err(e) = tokio::select!(
            v = main_loop(&cli, &buzz_dev, ev_dev) => { v },
            v = monitor_disconnected(&bt_device) => { match v {
                Ok(_) => Ok(()),
                Err(e) => Err(Box::new(e).into())
            } },
            v = keep_device_alive(&buzz_dev, false) => { v }
        ) {
            println!("Ran into error - {}, restarting loop", e);
        } else {
            println!("Slim Pen 2 ({}) disconnected!", addr);
        }
    }
}

fn try_connect_hid(addr: &str) -> Result<(BuzzDevice, evdev::Device), Box<dyn Error>> {
    let buzz_dev = BuzzDevice::new(Some(&addr))?;
    let ev_dev = evdev::enumerate()
        .find_map(|(_, d)| {
            if let Some(dev_name) = d.name()
                && dev_name.contains("IPTSD Virtual Stylus")
            {
                Some(d)
            } else {
                None
            }
        })
        .ok_or("No evdev device found matching \"IPTS Virtual Stylus\"")?;
    return Ok((buzz_dev, ev_dev));
}

async fn main_loop(
    cli: &Cli,
    buzz_dev: &BuzzDevice,
    ev_dev: evdev::Device,
) -> Result<(), Box<dyn Error>> {
    let mut x_res = 0;
    let mut y_res = 0;
    let mut pressure_res = 0.0;
    for (axis_code, abs_info) in ev_dev.get_absinfo()? {
        match axis_code {
            AbsoluteAxisCode::ABS_X => {
                x_res = abs_info.maximum() - abs_info.minimum();
            }
            AbsoluteAxisCode::ABS_Y => {
                y_res = abs_info.maximum() - abs_info.minimum();
            }
            AbsoluteAxisCode::ABS_PRESSURE => {
                pressure_res = (abs_info.maximum() - abs_info.minimum()) as f64;
            }
            _ => {}
        }
    }
    let display_res = ((x_res.pow(2) + y_res.pow(2)) as f64).sqrt();

    let waveform: Waveform = cli.waveform.try_into()?;
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
    let mut buzz_duration = Duration::from_millis(0);

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
                let timestamp = sync.timestamp();
                if let Ok(duration) = timestamp.duration_since(last_timestamp)
                    && duration < buzz_duration
                {
                    continue;
                }

                if !btn_touch {
                    continue;
                }

                let mut vib = if btn_touch_justpressed {
                    btn_touch_justpressed = false;
                    // small buzz on pen touch
                    0.5
                } else {
                    let delta_x = x - old_x;
                    let delta_y = y - old_y;
                    // apply a sqrt transform after dividing dist by 1024, then renormalize to 256
                    // this helps gain a bit of vib at lower velocities
                    (((delta_x.pow(2) + delta_y.pow(2)) as f64).sqrt() / display_res)
                        .powf(1.0 / cli.distance_root)
                };

                vib *= if eraser {
                    0.4
                } else {
                    (pressure as f64 / pressure_res).powf(1.0 / cli.pressure_root)
                };

                vib *= cli.intensity;

                buzz_duration = buzz_dev.buzz((vib * 256.0) as u8, waveform)?;

                old_x = x;
                old_y = y;
                last_timestamp = timestamp;
            }
            _ => {}
        };
    }
    Ok(())
}

// ping loop to keep the connection alive
// does not work
async fn keep_device_alive(buzz_dev: &BuzzDevice, keep_alive: bool) -> Result<(), Box<dyn Error>> {
    loop {
        if keep_alive {
            buzz_dev.dump_feature_reports();
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
