#[macro_use]
mod box_array;

pub mod cpu;
pub mod memory;
pub mod lcd;
pub mod interrupt;
pub mod dac;
pub mod rtc;
mod irda;
mod timer;

#[macro_use]
extern crate log;
extern crate shaman;
extern crate rustc_serialize;

/// Maximal frequency of the CPU, this clock can be shifted left by a
/// factor 0...7 to give the effective CPU frequency.
pub const MASTER_CLOCK_HZ: u32 = 31232 << 7;
