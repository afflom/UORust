//! Temperature bands, counters and readings.

use crate::config as cfg;
use crate::config::ConfigError;

/// A closed band of temperatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    /// The lowest temperature in the band.
    pub low: u32,
    /// The highest temperature in the band.
    pub high: u32,
}

/// The band's width, `None` when the band is inverted.
pub fn width(band: Band) -> Option<u32> {
    band.high.checked_sub(band.low)
}

/// The midpoint of a band, `None` when the sum overflows.
pub fn checked_midpoint(band: Band) -> Option<u32> {
    let sum = band.low.checked_add(band.high)?;
    Some(sum / 2)
}

/// The next value of the 8-bit hardware tick counter, which wraps.
pub fn tick(counter: u8) -> u8 {
    counter.wrapping_add(1)
}

/// How far the reading lies below the band, never negative.
pub fn shortfall(band: Band, current: u32) -> u32 {
    band.low.saturating_sub(current)
}

/// True when a signed outdoor reading is at or below zero.
pub fn freezing(outdoor: i64) -> bool {
    outdoor <= 0
}

/// The change between two signed readings, `None` on overflow.
pub fn delta(before: i64, after: i64) -> Option<i64> {
    after.checked_sub(before)
}

/// The band with whether it is well-formed.
pub fn checked(band: Band) -> (Band, bool) {
    (band, band.low <= band.high)
}

/// True for the unit names of Celsius.
pub fn is_celsius(unit: &str) -> bool {
    unit == "C" || unit == "celsius"
}

/// How many readings lie above the target.
pub fn above(readings: &[u32], target: u32) -> u32 {
    match readings {
        [] => 0,
        [first, rest @ ..] => {
            if *first > target {
                above(rest, target).saturating_add(1)
            } else {
                above(rest, target)
            }
        }
    }
}

/// The first reading, if there is one.
pub fn first_reading(readings: &[u32]) -> Option<u32> {
    readings.first().copied()
}

/// The configured target, or the default when there is none.
pub fn target_or(setting: Option<u32>, default: u32) -> u32 {
    setting.unwrap_or(default)
}

/// Whether to heat, once the band has been read.
pub fn heat_for(band: Result<Band, ConfigError>, current: u32) -> Result<bool, ConfigError> {
    let band = band?;
    Ok(current < band.low)
}

/// The label of a fan speed setting.
pub fn fan_label(speed: u8) -> &'static str {
    match speed {
        0 => "off",
        1 | 2 => "low",
        _ => "high",
    }
}

/// A reading with its unit, such as `21 C`.
pub fn labelled(value: &str, unit: &str) -> String {
    format!("{value} {unit}")
}

/// A step of reading a band: the value, or why it could not be read.
pub type Read<T> = Result<T, ConfigError>;

/// The low end of a band read through the alias and the module path.
pub fn low_of(band: Read<crate::band::Band>) -> Read<u32> {
    let band = band?;
    Ok(band.low)
}

/// Whether the error is that the text was not a number.
pub fn not_a_number(error: cfg::ConfigError) -> bool {
    matches!(error, cfg::ConfigError::NotANumber)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_measure_and_count() {
        let band = Band { low: 18, high: 22 };
        assert_eq!(width(band), Some(4));
        assert_eq!(checked_midpoint(band), Some(20));
        assert_eq!(shortfall(band, 15), 3);
        assert_eq!(above(&[17, 19, 23], 18), 2);
        assert_eq!(first_reading(&[]), None);
        assert_eq!(target_or(None, 21), 21);
        assert_eq!(heat_for(Ok(band), 17).ok(), Some(true));
        assert!(checked(band).1);
        assert_eq!(low_of(Ok(band)).ok(), Some(18));
        assert!(not_a_number(ConfigError::NotANumber));
        assert!(!not_a_number(ConfigError::Unreadable));
    }

    #[test]
    fn counters_and_signed_readings() {
        assert_eq!(tick(255), 0);
        assert!(freezing(-3));
        assert!(!freezing(4));
        assert_eq!(delta(i64::MIN, 1), None);
        assert!(is_celsius("C"));
        assert!(!is_celsius("F"));
        assert_eq!(fan_label(2), "low");
        assert_eq!(labelled("21", "C"), "21 C");
    }
}
