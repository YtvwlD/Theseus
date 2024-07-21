#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};
use ibverbs::ibv_qp_type::IBV_QPT_UC;

pub fn main(_args: Vec<String>) -> isize {
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
    println!("Creating queue pair...");
    let qp = pd.create_qp(&cq, &cq, IBV_QPT_UC)
        .build()
        .expect("failed to create queue pair");
    println!("Lid: {}, QPN: {}", qp.endpoint().lid, qp.endpoint().num);
    0
}
