#![no_std]
#[macro_use] extern crate app_io;
extern crate alloc;

use alloc::{string::String, vec::Vec};
use mlx3::get_mlx3_nic;


pub fn main(_args: Vec<String>) -> isize {
    if let Some(nic_mutex) = get_mlx3_nic() {
        let mut nic = nic_mutex.lock();
        let stats = nic
            .get_stats()
            .expect("failed to get data from NIC");
        println!("CA '{}'", stats.name);
        println!("    Number of ports: {}", stats.ports.len());
        println!(
            "    Firmware version: {}.{}.{}",
            stats.firmware_version.0,
            stats.firmware_version.1,
            stats.firmware_version.2,
        );
        for port in stats.ports {
            println!("    Port {}:", port.number);
            let state = match port.link_up {
                true => "Active",
                false => "Down",
            };
            println!("        State: {state}");
            println!("        Capability mask: 0x{:x}", port.capability_mask);
            println!("        Link layer: {:?}", port.layer);
        }
    }
    0
}
