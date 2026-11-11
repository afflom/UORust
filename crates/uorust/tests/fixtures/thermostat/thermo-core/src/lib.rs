//! Thermostat control: decides from a reading and a target whether to heat.

pub mod band;
pub mod config;
pub mod sensor;

/// The controller's operating mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Neither heating nor cooling.
    #[default]
    Idle,
    /// The heater is on.
    Heating,
    /// The cooler is on.
    Cooling,
}

/// True when the reading is below the target.
///
/// ```
/// assert!(thermo_core::should_heat(18, 21));
/// ```
pub fn should_heat(current: u32, target: u32) -> bool {
    current < target
}

/// True when the reading lies inside the closed band.
pub fn within_band(current: u32, low: u32, high: u32) -> bool {
    low <= current && current <= high
}

/// The next mode, given whether the room is cold or hot.
pub fn next_mode(mode: Mode, cold: bool, hot: bool) -> Mode {
    match mode {
        Mode::Idle => {
            if cold {
                Mode::Heating
            } else if hot {
                Mode::Cooling
            } else {
                Mode::Idle
            }
        }
        Mode::Heating => {
            if cold {
                Mode::Heating
            } else {
                Mode::Idle
            }
        }
        Mode::Cooling => {
            if hot {
                Mode::Cooling
            } else {
                Mode::Idle
            }
        }
    }
}

/// Stops heating or cooling at once.
pub fn halt(mode: Mode) -> Mode {
    match mode {
        Mode::Heating => Mode::Idle,
        Mode::Cooling => Mode::Idle,
        Mode::Idle => Mode::Idle,
    }
}

/// The midpoint of a band, which wraps on overflow in release builds.
pub fn midpoint(low: u32, high: u32) -> u32 {
    (low + high) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cold room heats.
    #[test]
    fn heats_when_cold() {
        assert!(should_heat(18, 21));
        assert!(!should_heat(22, 21));
    }

    #[test]
    fn band_is_inclusive() {
        assert!(within_band(5, 5, 9));
        assert_eq!(within_band(10, 5, 9), false);
    }

    #[test]
    fn idle_room_that_is_cold_heats() {
        assert_eq!(next_mode(Mode::Idle, true, false), Mode::Heating);
        assert_eq!(halt(Mode::Cooling), Mode::Idle);
    }

    #[test]
    fn midpoint_of_a_band() {
        assert_eq!(midpoint(4, 8), 6);
    }
}
