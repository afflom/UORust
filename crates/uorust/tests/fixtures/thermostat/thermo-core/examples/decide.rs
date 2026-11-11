//! Prints the decision for one reading.

fn main() {
    println!("{}", thermo_core::should_heat(18, 21));
}
