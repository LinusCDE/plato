use anyhow::Error;
use super::{Frontlight, LightLevels};

pub struct FakeFrontlight {
    value: f32,
}

impl FakeFrontlight {
    pub fn new(value: f32) -> Result<FakeFrontlight, Error> {
        Ok(FakeFrontlight { value })
    }
}

impl Frontlight for FakeFrontlight {
    fn set_intensity(&mut self, value: f32) {
        self.value = value;
    }

    fn set_warmth(&mut self, _value: f32) { }

    fn levels(&self) -> LightLevels {
        LightLevels {
            intensity: self.value,
            warmth: 0.0,
        }
    }
}
