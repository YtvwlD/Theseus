//! A mlx3 driver for a ConnectX-3 card.
//! 
//! This is (very) roughly based on [the Nautilus driver](https://github.com/HExSA-Lab/nautilus/blob/master/src/dev/mlx3_ib.c)
//! and the existing mlx5 driver.

#![no_std]
extern crate alloc;

mod cmd;
mod device;
mod event_queue;
mod fw;
mod icm;
mod mcg;
mod port;
mod profile;

#[macro_use] extern crate log;

use alloc::vec::Vec;
use cmd::CommandInterface;
use event_queue::{init_eqs, EventQueue, Offsets};
use fw::{Hca, MappedFirmwareArea};
use icm::MappedIcmTables;
use memory::MappedPages;
use pci::PciDevice;
use port::Port;
use sync_irq::IrqSafeMutex;

use crate::device::{Ownership, ResetRegisters};
use crate::fw::Firmware;
use crate::profile::Profile;

/// Vendor ID for Mellanox
pub const MLX_VEND: u16 = 0x15b3;
/// Device ID for the ConnectX-3 NIC
pub const CONNECTX3_DEV: u16 = 0x1003;

/// Struct representing a ConnectX-3 card
pub struct ConnectX3Nic {
    config_regs: MappedPages,
    firmware_area: Option<MappedFirmwareArea>,
    icm_tables: Option<MappedIcmTables>,
    hca: Option<Hca>,
    doorbells: Vec<MappedPages>,
    blueflame: Vec<MappedPages>,
    eqs: Vec<EventQueue>,
    ports: Vec<Port>,
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
        // map the User Access Region
        let user_access_region = mlx3_pci_dev.pci_map_bar_mem(2)?;
        trace!("mlx3 user access region: {:?}", user_access_region);

        ResetRegisters::reset(mlx3_pci_dev, &mut config_regs)?;

        // TODO: This shouldn't be necessary.
        // We should be restoring the config space in reset(),
        // but even now these bits are always set.
        mlx3_pci_dev.pci_set_command_memory_space_bit();
        mlx3_pci_dev.pci_set_command_bus_master_bit();

        Ownership::get(&config_regs)?;
        let mut command_interface = CommandInterface::new(&mut config_regs)?;
        let firmware = Firmware::query(&mut command_interface)?;
        let firmware_area = firmware.map_area(&mut command_interface)?;
        let mut nic = Self {
            config_regs,
            firmware_area: Some(firmware_area),
            icm_tables: None,
            hca: None,
            doorbells: Vec::new(),
            blueflame: Vec::new(),
            eqs: Vec::new(),
            ports: Vec::new(),
        };
        let mut command_interface = CommandInterface::new(&mut nic.config_regs)?;
        let firmware_area = nic.firmware_area.as_mut().unwrap();
        firmware_area.run(&mut command_interface)?;
        let caps = firmware_area.query_capabilities(&mut command_interface)?;
        // In the Nautilus driver, some of the port setup already happens here.
        let mut offsets = Offsets::init(&caps);
        let mut profile = Profile::new(&caps)?;
        let aux_pages = firmware_area.set_icm(&mut command_interface, profile.total_size)?;
        let icm_aux_area = firmware_area.map_icm_aux(&mut command_interface, aux_pages)?;
        nic.icm_tables = Some(icm_aux_area.map_icm_tables(&mut command_interface, &profile, &caps)?);
        nic.hca = Some(profile.init_hca.init_hca(&mut command_interface)?);
        let hca = nic.hca.as_ref().unwrap();
        // give us the interrupt pin
        hca.query_adapter(&mut command_interface)?;
        let memory_regions = nic.icm_tables.as_mut().unwrap().memory_regions();
        // get the doorbells and the BlueFlame section
        (nic.doorbells, nic.blueflame) = caps.get_doorbells_and_blueflame(
            user_access_region
        )?;
        nic.eqs = init_eqs(
            &mut command_interface, &mut nic.doorbells, &caps, &mut offsets,
            memory_regions,
        )?;
        // In the Nautilus driver, CQs and QPs are already allocated here.
        hca.config_mad_demux(&mut command_interface, &caps)?;
        nic.ports = hca.init_ports(&mut command_interface, &caps)?;
        todo!()
    }
}

impl Drop for ConnectX3Nic {
    fn drop(&mut self) {
        let mut cmd = CommandInterface::new(&mut self.config_regs)
            .expect("failed to get command interface");
        while let Some(port) = self.ports.pop() {
            port
                .close(&mut cmd)
                .unwrap()
        }
        while let Some(eq) = self.eqs.pop() {
            eq
                .destroy(&mut cmd)
                .unwrap()
        }
        if let Some(hca) = self.hca.take() {
            hca
                .close(&mut cmd)
                .unwrap()
        }
        if let Some(icm_tables) = self.icm_tables.take() {
            icm_tables
                .unmap(&mut cmd)
                .unwrap()
        }
        if let Some(firmware_area) = self.firmware_area.take() {
            firmware_area
                .unmap(&mut cmd)
                .unwrap()
        }
    }
}
