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

/// The band with its high end moved: a struct update.
pub fn with_high(band: Band, high: u32) -> Band {
    Band { high, ..band }
}

/// The band with its low end moved: a field assignment.
pub fn with_low(band: Band, low: u32) -> Band {
    let mut out = band;
    out.low = low;
    out
}

/// Two runs of readings, one after the other.
pub fn joined(first: Vec<u32>, second: Vec<u32>) -> Vec<u32> {
    let mut out = first;
    out.extend(second);
    out
}

/// A band with a label it borrows: the lifetime is erased.
pub struct Labelled<'a> {
    /// What the band is called.
    pub label: &'a str,
    /// The band.
    pub band: Band,
}

/// The label of a labelled band.
pub fn label_of(view: Labelled<'_>) -> String {
    view.label.to_string()
}

/// The reading held inside the band, whether a limit moved it, and
/// whether the band is too narrow to hold anything but its low end.
pub fn clamped(band: Band, reading: u32) -> (u32, bool, bool) {
    let mut value = reading;
    let mut moved = false;
    let mut narrow = false;
    if value < band.low {
        value = band.low;
        moved = true;
    }
    if value > band.high {
        value = band.high;
        moved = true;
    }
    if let Some(w) = width(band) {
        if w < 2 {
            narrow = true;
        }
    }
    if narrow {
        value = band.low;
    } else if moved {
        narrow = false;
    }
    (value, moved, narrow)
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
        assert_eq!(clamped(band, 25), (22, true, false));
        assert_eq!(clamped(Band { low: 18, high: 19 }, 25), (18, true, true));
        let view = Labelled {
            label: "night",
            band: Band { low: 16, high: 18 },
        };
        assert_eq!(view.band.low, 16);
        assert_eq!(
            label_of(Labelled {
                label: "night",
                band: Band { low: 16, high: 18 }
            }),
            "night"
        );
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
