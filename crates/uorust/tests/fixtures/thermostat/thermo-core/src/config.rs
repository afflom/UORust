//! Reads the target temperature from a file or the environment.

use std::fs;

/// Why a target could not be read.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Unreadable,
    /// The text is not a whole number of degrees.
    NotANumber,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ConfigError {}

/// Parses a target such as `21`.
pub fn parse_target(text: &str) -> Result<u32, ConfigError> {
    text.trim().parse().map_err(|_| ConfigError::NotANumber)
}

/// Reads the target from a file.
pub fn load_target(path: &std::path::Path) -> Result<u32, ConfigError> {
    let text = fs::read_to_string(path).map_err(|_| ConfigError::Unreadable)?;
    parse_target(&text)
}

/// Reads the target from `THERMO_TARGET`.
pub fn target_from_env() -> Option<u32> {
    std::env::var("THERMO_TARGET").ok()?.parse().ok()
}
