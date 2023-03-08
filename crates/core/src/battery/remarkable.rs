use super::{Battery, Status};
use anyhow::{format_err, Error};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

const BATTERY_CAPACITY: &str = "capacity";
const BATTERY_STATUS: &str = "status";

// TODO: health, technology, time_to_full_now, time_to_empty_now
pub struct RemarkableBattery {
    capacity: File,
    status: File,
}

impl RemarkableBattery {
    pub fn new() -> Result<RemarkableBattery, Error> {
        let base = PathBuf::from(format!(
            "/sys/class/power_supply/{}",
            libremarkable::device::CURRENT_DEVICE.get_internal_battery_name()
        ));
        let capacity = File::open(base.join(BATTERY_CAPACITY))?;
        let status = File::open(base.join(BATTERY_STATUS))?;
        Ok(RemarkableBattery { capacity, status })
    }
}

impl Battery for RemarkableBattery {
    fn capacity(&mut self) -> Result<Vec<f32>, Error> {
        let mut buf = String::new();
        self.capacity.seek(SeekFrom::Start(0))?;
        self.capacity.read_to_string(&mut buf)?;
        Ok(vec![ buf.trim_end().parse::<f32>().unwrap_or(0.0) ])
    }

    fn status(&mut self) -> Result<Vec<Status>, Error> {
        let mut buf = String::new();
        self.status.seek(SeekFrom::Start(0))?;
        self.status.read_to_string(&mut buf)?;
        match buf.trim_end() {
            "Discharging" => Ok(vec![ Status::Discharging ]),
            "Charging" => Ok(vec![ Status::Charging ]),
            "Not charging" | "Full" => Ok(vec![ Status::Charged ]),
            _ => Err(format_err!("Unknown battery status.")),
        }
    }
}
