#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};


pub fn main(_args: Vec<String>) -> isize {
    println!("Hello World!");
    0
}
