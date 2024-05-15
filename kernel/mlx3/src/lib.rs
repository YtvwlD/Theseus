//! A mlx3 driver for a ConnectX-3 card.
//! 
//! This is (very) roughly based on [the Nautilus driver](https://github.com/HExSA-Lab/nautilus/blob/master/src/dev/mlx3_ib.c)
//! and the existing mlx5 driver.

#![no_std]

mod cmd;
mod device;
mod fw;

#[macro_use] extern crate log;

use pci::PciDevice;
use sync_irq::IrqSafeMutex;

use crate::device::{Ownership, ResetRegisters};
use crate::fw::Firmware;

/// Vendor ID for Mellanox
pub const MLX_VEND: u16 = 0x15b3;
/// Device ID for the ConnectX-3 NIC
pub const CONNECTX3_DEV: u16 = 0x1003;

/// Struct representing a ConnectX-3 card
pub struct ConnectX3Nic {

}

/// Functions that setup the struct.
impl ConnectX3Nic {
    /// Initializes the ConnectX-3 card that is connected as the given PciDevice.
    ///
    /// # Arguments
    /// * `mlx3_pci_dev`: Contains the pci device information.
    pub fn init(mlx3_pci_dev: &PciDevice) -> Result<&'static IrqSafeMutex<ConnectX3Nic>, &'static str> {
        // set the memory space bit for this PciDevice
        mlx3_pci_dev.pci_set_command_memory_space_bit();
        // set the bus mastering bit for this PciDevice, which allows it to use DMA
        mlx3_pci_dev.pci_set_command_bus_master_bit();

        // map the Global Device Configuration registers
        let mut config_regs = mlx3_pci_dev.pci_map_bar_mem(0)?;
        trace!("mlx3 configuration registers: {:?}", config_regs);
        // TODO: there's also the User Access Region in BAR 2

        ResetRegisters::reset(mlx3_pci_dev, &mut config_regs)?;

        // TODO: This shouldn't be necessary.
        // We should be restoring the config space in reset(),
        // but even now these bits are always set.
        mlx3_pci_dev.pci_set_command_memory_space_bit();
        mlx3_pci_dev.pci_set_command_bus_master_bit();

        Ownership::get(&config_regs)?;
        let firmware = Firmware::query(&mut config_regs)?;
        let firmware_area = firmware.map_area(&mut config_regs)?;
        firmware_area.unmap(&mut config_regs)?;

        todo!()
    }
}
