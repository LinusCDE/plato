use super::{Battery, Status};
use anyhow::{bail, format_err, Error};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

const BATTERY_CAPACITY: &str = "capacity";
const BATTERY_STATUS: &str = "status";

// TODO: health, technology, time_to_full_now, time_to_empty_now
pub struct RemarkableBattery {
    capacity: Option<File>,
    status: Option<File>,
}

impl RemarkableBattery {
    pub fn new() -> Result<RemarkableBattery, Error> {
        let base = PathBuf::from(format!(
            "/sys/class/power_supply/{}",
            libremarkable::device::CURRENT_DEVICE.get_internal_battery_name()
        ));
        if ! base.exists() {
            return Ok(RemarkableBattery { capacity: None, status: None });
        }
        let capacity = File::open(base.join(BATTERY_CAPACITY))?;
        let status = File::open(base.join(BATTERY_STATUS))?;
        Ok(RemarkableBattery { capacity: Some(capacity), status: Some(status) })
    }
}

impl Battery for RemarkableBattery {
    fn capacity(&mut self) -> Result<Vec<f32>, Error> {
        if let Some(capacity) = &mut self.capacity {
            let mut buf = String::new();
            capacity.seek(SeekFrom::Start(0))?;
            capacity.read_to_string(&mut buf)?;
            Ok(vec![buf.trim_end().parse::<f32>().unwrap_or(0.0) ])
        } else {
            bail!("No battery found")
        }
    }

    fn status(&mut self) -> Result<Vec<Status>, Error> {
        if let Some(status) = &mut self.status {
            let mut buf = String::new();
            status.seek(SeekFrom::Start(0))?;
            status.read_to_string(&mut buf)?;
            match buf.trim_end() {
                "Discharging" => Ok(vec![Status::Discharging]),
                "Charging" => Ok(vec![Status::Charging]),
                "Not charging" | "Full" => Ok(vec![Status::Charged]),
                _ => Err(format_err!("Unknown battery status.")),
            }
        }else {
            bail!("No battery found")
        }
    }
}
