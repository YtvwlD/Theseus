//! This module consists of functions that work close to the hardware of the hca.
use byteorder::BigEndian;
use memory::MappedPages;
use pci::PciDevice;
use sleep::{sleep, Duration};
use volatile::{ReadOnly, WriteOnly};
use zerocopy::{U32, FromBytes};

const RESET_BASE: usize = 0xf0000;
const OWNER_BASE: usize = 0x8069c;
pub(super) const DEFAULT_UAR_PAGE_SHIFT: u8 = 12;
pub(super) const PAGE_SHIFT: u8 = 12;

#[derive(FromBytes)]
#[repr(C, packed)]
pub(super) struct ResetRegisters {
    _padding: [u8; 0x10],
    reset: WriteOnly<U32<BigEndian>>,
    _padding2: [u8; 0x3e8],
    semaphore: ReadOnly<U32<BigEndian>>,
}

impl ResetRegisters {
    pub(super) fn reset(mlx3_pci_dev: &PciDevice, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        trace!("Initiating card reset for ConnectX-3...");

        // get the reset registers
        let reset_registers: &mut ResetRegisters = config_regs.as_type_mut(RESET_BASE)?;

        // TODO: save config space

        // grab HW semaphore to lock out flash updates
        let mut sem = 1;
        for _ in 0..1000 {
            sem = reset_registers.semaphore.read().get();
            if sem == 0 {
                break;
            }
            trace!("waiting for semaphore...");
            sleep(Duration::from_millis(1)).unwrap();
        }
        if sem != 0 {
            return Err("Failed to acquire HW semaphore");
        }

        // actually hit reset
        reset_registers.reset.write(1.into());
        // docs say to wait one second before accessing device
        sleep(Duration::from_secs(1)).unwrap();

        for _ in 0..100 {
            // wait for it to respond to PCI cycles
            if mlx3_pci_dev.pci_read_vendor_id() != 0xffff {
                return Ok(())
            }
            trace!("waiting for card...");
            sleep(Duration::from_millis(1)).unwrap();
        }
        Err("Card failed to reset")
    }
}

#[derive(FromBytes)]
#[repr(transparent)]
pub(super) struct Ownership {
    value: ReadOnly<U32<BigEndian>>,
}

impl Ownership {
    pub(super) fn get(config_regs: &MappedPages) -> Result<(), &'static str> {
        let ownership: &Ownership = config_regs.as_type(OWNER_BASE)?;
        if ownership.value.read().get() == 0 {
            Ok(())
        } else {
            Err("We don't have card ownership")
        }
    }
}

pub(super) fn uar_index_to_hw(index: usize) -> usize {
    index << (PAGE_SHIFT - DEFAULT_UAR_PAGE_SHIFT)
}
