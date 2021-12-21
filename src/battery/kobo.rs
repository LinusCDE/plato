use std::fs::File;
use std::path::Path;
use std::io::{Read, Seek, SeekFrom};
use anyhow::{Error, format_err};
use crate::device::CURRENT_DEVICE;
use super::{Battery, Status};

const BATTERY_INTERFACE_A: &str = "/sys/class/power_supply/mc13892_bat";
const BATTERY_INTERFACE_B: &str = "/sys/class/power_supply/battery";

const BATTERY_CAPACITY: &str = "capacity";
const BATTERY_STATUS: &str = "status";

// TODO: health, technology, time_to_full_now, time_to_empty_now
pub struct KoboBattery {
    capacity: File,
    status: File,
}

impl KoboBattery {
    pub fn new() -> Result<KoboBattery, Error> {
        let base = if CURRENT_DEVICE.mark() != 8 {
            Path::new(BATTERY_INTERFACE_A)
        } else {
            Path::new(BATTERY_INTERFACE_B)
        };
        let capacity = File::open(base.join(BATTERY_CAPACITY))?;
        let status = File::open(base.join(BATTERY_STATUS))?;
        Ok(KoboBattery { capacity, status })
    }
}

impl Battery for KoboBattery {
    fn capacity(&mut self) -> Result<f32, Error> {
        let mut buf = String::new();
        self.capacity.seek(SeekFrom::Start(0))?;
        self.capacity.read_to_string(&mut buf)?;
        Ok(buf.trim_end().parse::<f32>().unwrap_or(0.0))
    }

    fn status(&mut self) -> Result<Status, Error> {
        let mut buf = String::new();
        self.status.seek(SeekFrom::Start(0))?;
        self.status.read_to_string(&mut buf)?;
        match buf.trim_end() {
            "Discharging" => Ok(Status::Discharging),
            "Charging" => Ok(Status::Charging),
            "Not charging" | "Full" => Ok(Status::Charged),
            _ => Err(format_err!("unknown battery status")),

        }
    }
}
