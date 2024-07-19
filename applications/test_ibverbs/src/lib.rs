#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};

pub fn main(_args: Vec<String>) -> isize {
    let context = ibverbs::devices()
        .expect("failed to list devices")
        .iter()
        .next()
        .expect("failed to get device")
        .open()
        .expect("failed to open device");
    0
}
