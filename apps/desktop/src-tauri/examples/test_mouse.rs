use evdev::{Device, EventType, KeyCode};
use std::io::Write;

fn main() {
    let mut devices: Vec<Device> = evdev::enumerate()
        .filter_map(|(_, mut dev)| {
            let name = dev.name().unwrap_or("unknown").to_string();
            let keys = dev.supported_keys()?;
            if keys.contains(KeyCode::BTN_LEFT) || keys.contains(KeyCode::BTN_MIDDLE) {
                println!("  [{}] BTN_SIDE={} BTN_EXTRA={}",
                    name,
                    keys.contains(KeyCode::BTN_SIDE),
                    keys.contains(KeyCode::BTN_EXTRA));
                // Non-blocking so we can poll multiple devices
                let _ = dev.set_nonblocking(true);
                Some(dev)
            } else {
                None
            }
        })
        .collect();

    if devices.is_empty() {
        println!("No mouse devices found! Run: sudo usermod -aG input $USER");
        return;
    }

    println!("\n>>> Press extra mouse buttons now (Ctrl+C to stop) <<<\n");
    std::io::stdout().flush().ok();

    loop {
        for dev in &mut devices {
            match dev.fetch_events() {
                Ok(events) => {
                    for event in events {
                        if event.event_type() == EventType::KEY && event.value() == 1 {
                        let code = event.code();
                        let name = match code {
                            0x110 => "BTN_LEFT",
                            0x111 => "BTN_RIGHT",
                            0x112 => "BTN_MIDDLE",
                            0x113 => "BTN_SIDE",
                            0x114 => "BTN_EXTRA",
                            0x115 => "BTN_FORWARD",
                            0x116 => "BTN_BACK",
                            0x117 => "BTN_TASK",
                            _ => "OTHER",
                        };
                        println!("  PRESS: code=0x{:03x} ({}) -> maps to: {}",
                            code, name,
                            match code {
                                0x112 => "MouseMiddle",
                                0x113 => "MouseBack",
                                0x114 => "MouseForward",
                                0x115 => "Mouse6",
                                0x116 => "Mouse7",
                                0x117 => "Mouse8",
                                _ => "(check code)",
                            });
                        std::io::stdout().flush().ok();
                    }
                }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
