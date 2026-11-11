//! Temperature sensors the controller reads from.

/// A source of temperature readings.
pub trait Sensor {
    /// The current reading in whole degrees.
    fn read(&self) -> u32;

    /// The reading raised by a calibration offset.
    fn calibrated(&self, offset: u32) -> u32 {
        self.read() + offset
    }
}

/// A sensor that always reads the same value.
pub struct Fixed(pub u32);

impl Sensor for Fixed {
    fn read(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_sensor_reads_its_value() {
        assert_eq!(Fixed(20).read(), 20);
        assert_eq!(Fixed(20).calibrated(1), 21);
    }
}
