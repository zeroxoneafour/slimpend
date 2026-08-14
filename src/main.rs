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
        default_value_t = 3u8,
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
        long = "pressure",
        help = "Nth power to apply to pressure multiplier",
        default_value_t = 1.0,
        global = true
    )]
    pressure_pow: f64,
    #[arg(
        short = 'd',
        long = "velocity",
        help = "Nth power to apply to velocity multiplier",
        default_value_t = 1.0,
        global = true
    )]
    velocity_pow: f64,
    #[command(subcommand)]
    command: Option<Commands>,
}

impl TryFrom<u8> for Waveform {
    type Error = BuzzError;
    fn try_from(value: u8) -> Result<Self, BuzzError> {
        match value {
            0 => Ok(Waveform::Click),
            1 => Ok(Waveform::Press),
            2 => Ok(Waveform::Release),
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

    // validate cli arguments before running a command
    let _: Waveform = cli.waveform.try_into()?;

    match &cli.command {
        Some(Commands::Buzz) => {
            let dev = BuzzDevice::new(None)?;
            let res =
                tokio::select! { v = buzz(&dev, &cli) => v, _ = tokio::signal::ctrl_c() => Ok(()) };
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

async fn buzz(dev: &BuzzDevice, cli: &Cli) -> Result<(), Box<dyn Error>> {
    let waveform: Waveform = cli.waveform.try_into()?;
    let intensity = (1.0 * cli.intensity) as u8;
    println!(
        "buzzing on waveform 0x{:x} with intensity {}",
        waveform as u16, intensity
    );
    loop {
        tokio::time::sleep(dev.buzz(intensity, waveform)?).await;
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
        buzz_dev.buzz(255, Waveform::Success)?;
        if let Err(e) = tokio::select!(
            v = main_loop(&cli, &buzz_dev, ev_dev) => { v },
            v = monitor_disconnected(&bt_device) => { match v {
                Ok(_) => Ok(()),
                Err(e) => Err(Box::new(e).into())
            } }
        ) {
            println!("Ran into error - {}, restarting loop", e);
            tokio::time::sleep(Duration::from_secs(1)).await;
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
    let mut pressure = 0;
    // track buzz and velocity timestamp seperately
    // so we can reset velocity timestamp whenever old_x/old_y reset
    let mut buzz_timestamp = SystemTime::now();
    let mut velocity_timestamp = SystemTime::now();
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
            EventSummary::Key(ev, key, value) => match key {
                KeyCode::BTN_TOUCH => {
                    btn_touch = !(value == 0);
                    if btn_touch {
                        // resync old_x and old_y on touch
                        old_x = x;
                        old_y = y;
                        // eraser sends a ton of BTN_TOUCHes that cause velocity timestamp to update too fast,
                        // resulting in a near-infinite reported velocity at time of contact
                        if !eraser {
                            velocity_timestamp = ev.timestamp();
                        }
                    } else {
                        pressure = 0;
                    }
                }
                KeyCode::BTN_TOOL_RUBBER => {
                    eraser = !(value == 0);
                }
                _ => {}
            },
            EventSummary::Synchronization(sync, _, _) => {
                let timestamp = sync.timestamp();
                if let Ok(duration) = timestamp.duration_since(buzz_timestamp)
                    && duration < buzz_duration
                {
                    continue;
                }

                if !btn_touch || pressure == 0 {
                    continue;
                }

                let mut vib = {
                    let delta_x = x - old_x;
                    let delta_y = y - old_y;
                    let delta_p = ((delta_x.pow(2) + delta_y.pow(2)) as f64).sqrt() / display_res;
                    let Ok(delta_t) = timestamp.duration_since(velocity_timestamp) else {
                        continue;
                    };
                    // velocity is in display diagonals/s
                    // divided by <arbitrary value> (here 20) to prevent super high values
                    let velocity = delta_p / (delta_t.as_micros() as f64 / 10.0f64.powi(6)) / 20.0;
                    // update this up here so that returns due to 0 velocity
                    // dont cause velocity to carry over into next frames
                    old_x = x;
                    old_y = y;
                    velocity_timestamp = timestamp;
                    // if the pen basically is not moving, then don't send pressure signals
                    if velocity < 0.005 {
                        continue;
                    }
                    velocity.powf(cli.velocity_pow)
                };

                vib *= (pressure as f64 / pressure_res).powf(cli.pressure_pow);

                vib *= cli.intensity;

                let zero_vib = waveform.buzzless_intensity();
                let vib_u8 =
                    (vib.clamp(0.0, 1.0) * (255.0 - zero_vib as f64)).ceil() as u8 + zero_vib;
                //println!("{}", vib_u8);
                buzz_duration = buzz_dev.buzz(vib_u8, waveform)?;

                buzz_timestamp = timestamp;
            }
            _ => {}
        };
    }
    Ok(())
}
