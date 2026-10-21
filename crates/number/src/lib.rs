#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod bits_for_value;
pub mod machine_word;
mod power_of_two;
mod variablewidth;

pub use bits_for_value::bits_for_value;
pub use power_of_two::is_power_of_two;
pub use power_of_two::round_up_to_power_of_two;
pub use variablewidth::encoding_size;
pub use variablewidth::read_variablelength;
pub use variablewidth::write_variablelength;
