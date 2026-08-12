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

mod bluez;
mod buzz;
mod evdev;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Buzz,
    Serve,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Buzz) => {
            let dev = hid_connect()?;
            for wf in 0u8..6 {
                println!("testing buzz waveform {}", wf);
                buzz(&dev, 255, Waveform::Buzz(wf))?;
                sleep(Duration::from_millis(500));
            }
            for wf in 0u8..6 {
                println!("testing tap waveform {}", wf);
                buzz(&dev, 255, Waveform::Tap(wf))?;
                sleep(Duration::from_millis(500));
            }
            Ok(())
        }
        Some(Commands::Serve) => server().await,
        None => {
            println!("No command found, defaulting to server mode");
            server().await
        }
    }
}

async fn server() -> Result<(), Box<dyn Error>> {
    let Some(bt_device) = find_slim_pen_bt().await? else {
        println!("Please pair your slim pen 2 with bluetooth before starting the server!");
        return Ok(());
    };
    loop {
        if !bt_device.is_connected().await? {
            monitor_connected(&bt_device).await?;
        }
        // wait a second to let hid create shi
        tokio::time::sleep(Duration::from_millis(100)).await;
        println!("Slim pen 2 connected!");
        tokio::select!(v = main_loop() => { v? }, v = monitor_disconnected(&bt_device) => { v? });
        println!("Slim pen 2 disconected!");
    }
}

async fn main_loop() -> Result<(), Box<dyn Error>> {
    let hid_dev = hid_connect()?;
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
                let timestamp = sync.timestamp();
                // 20 ms is sufficient time for buzz(0) to complete
                // adding more buzzes before this time causes a backlog
                // leading to buzzing after picking up pen
                if timestamp.duration_since(last_timestamp)? < Duration::from_millis(20) {
                    continue;
                }
                last_timestamp = timestamp;

                if !btn_touch {
                    continue;
                }

                let delta_x = x - old_x;
                old_x = x;
                let delta_y = y - old_y;
                old_y = y;

                let mut vib = if btn_touch_justpressed {
                    btn_touch_justpressed = false;
                    // small buzz on pen touch
                    64.0
                } else {
                    ((delta_x.pow(2) + delta_y.pow(2)) as f64).sqrt() / 5.0
                };

                // use cube root scaling for pressure
                let pressure_coeff = (pressure as f64 / 4096.0).cbrt();
                vib *= pressure_coeff;

                if vib < 5.0 {
                    continue;
                }

                vib = vib.clamp(0.0, 255.0);

                buzz(&hid_dev, vib as u8, Waveform::Buzz(0))?;
            }
            _ => {}
        };
    }
    Ok(())
}
