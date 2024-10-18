#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

mod connection;
mod rdma;

use alloc::{string::String, vec::Vec};

fn crc32_generic(seed: u32, data: &[u8], polynomial: u32) -> u32 {
    let mut crc = seed;
    for idx in 0..data.len() {
        crc ^= u32::from(data[idx]);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (if crc & 1 != 0 { polynomial } else { 0 });
        }
    }
    crc
}

fn crc32(seed: u32, data: &[u8]) -> u32 {
    const POLYNOMIAL: u32 = 0xedb88320;
    assert_eq!(seed, 0);
    crc32_generic(seed, data, POLYNOMIAL)
}

fn read_int(prompt: &str) -> u32 {
    println!("{}", prompt);
    let mut buf = [0u8; 10];
    let stdin = app_io::stdin().expect("failed to open stdin");
    stdin.read(&mut buf).expect("failed to read from stdin");
    String::from_utf8(buf.to_vec())
        .expect("failed to parse string")
        .trim_end_matches("\0")
        .trim()
        .parse()
        .expect("not a valid integer")
}

pub fn main(args: Vec<String>) -> isize {
    let context = ibverbs::devices()
        .expect("failed to list devices")
        .iter()
        .next()
        .expect("failed to get device")
        .open()
        .expect("failed to open device");
    let pd = context.alloc_pd()
        .expect("failed to allocate protection domain");
    let cq = context.create_cq(4096, 0)
    .expect("failed to create completion queue");
    if args.iter().find(|a| a == &"--rdma").is_some() {
        rdma::run_test(pd, cq, args)
    } else {
        connection::run_test(pd, cq, args)
    }
}
