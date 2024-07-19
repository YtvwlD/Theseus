#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};
use ibverbs_sys::{ibv_get_device_list, ibv_get_device_name, ibv_open_device, ibv_query_device, ibv_query_port};


pub fn main(_args: Vec<String>) -> isize {
    let devices = ibv_get_device_list()
        .expect("failed to get device list");
    for device in devices {
        let device_name = ibv_get_device_name(&device)
            .expect("failed to get device name");
        println!("CA '{device_name}'");
        let context = ibv_open_device(&device)
            .expect("failed to open device");
        let device_stats = ibv_query_device(&context)
            .expect("failed to query device");
        println!("    Number of ports: {}", device_stats.phys_port_cnt);
        println!("    Firmware version: {}", device_stats.fw_ver);
        for port_num in 1..=device_stats.phys_port_cnt {
            println!("    Port {port_num}:");
            let port_stats = ibv_query_port(&context, port_num)
                .expect("failed to query port");
            println!("        State: {:?}", port_stats.state);
            println!("        Capability mask: 0x{:x}", port_stats.port_cap_flags);
            println!("        Link layer: {}", port_stats.link_layer);
        }
    }
    0
}
