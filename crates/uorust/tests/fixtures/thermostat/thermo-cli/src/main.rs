//! Prints whether to heat, for a reading given on the command line.

use thermo_core::config::target_from_env;
use thermo_core::should_heat;

fn main() {
    let current: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    let target = target_from_env().unwrap_or(21);
    if should_heat(current, target) {
        println!("heat");
    } else {
        println!("off");
        std::process::exit(1);
    }
}
