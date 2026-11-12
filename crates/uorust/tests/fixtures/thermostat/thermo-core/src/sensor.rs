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

/// Two reading counts together, which can leave `usize` on a narrow target.
pub fn total(first: usize, second: usize) -> usize {
    first + second
}

/// How far each reading at or above `floor` lies above it, skipping the
/// others with a `let ... else { continue; }`.
pub fn at_least(readings: &[u32], floor: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for r in readings {
        let Some(kept) = r.checked_sub(floor) else {
            continue;
        };
        out.push(kept);
    }
    out
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
