//! This module contains functionality to interact with the firmware.

use core::mem::size_of;

use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use zerocopy::{FromBytes, U16, U32, U64};

use crate::cmd::{CommandMailBox, Opcode};

#[derive(Clone, FromBytes)]
#[repr(C, packed)]
pub(super) struct Firmware {
    pages: U16<BigEndian>,
    major: U16<BigEndian>,
    sub_minor: U16<BigEndian>,
    minor: U16<BigEndian>,
    _padding1: u16,
    ix_rev: U16<BigEndian>,
    _padding2: [u8; 22], // contains the build timestamp
    clr_int_base: U64<BigEndian>,
    clr_int_bar: u8,
    // many fields follow
}

impl Firmware {
    pub(super) fn query(config_regs: &mut MappedPages) -> Result<Self, &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        // TODO: this should not be in high memory (?)
        let (pages, physical) = create_contiguous_mapping(PAGE_SIZE, DMA_FLAGS)?;
        trace!("asking the card to put information about its firmware into {pages:?}@{physical}...");
        cmd.execute_command(Opcode::QueryFw, None, 0, Some(physical))?;
        let mut fw = pages.as_type::<Firmware>(0)?.clone();
        fw.clr_int_bar = (fw.clr_int_bar >> 6) * 2;
        trace!("got firmware info: {fw:?}");
        Ok(fw)
    }
    
    pub(super) fn map_area(self, config_regs: &mut MappedPages) -> Result<MappedFirmwareArea, &'static str> {
        const MAX_CHUNK_LOG2: u32 = 18;
        trace!("mapping firmware area...");

        let mut cmd = CommandMailBox::new(config_regs)?;
        let size = PAGE_SIZE * usize::from(self.pages);
        let (pages, physical) = create_contiguous_mapping(size, DMA_FLAGS)?;
        let mut align = physical.value().trailing_zeros();
        if align > MAX_CHUNK_LOG2 {
            trace!("alignment greater than max chunk size, defaulting to 256KB");
            align = MAX_CHUNK_LOG2;
        }

        let mut count = size / (1 << align);
        if size % (1 << align) != 0 {
            count += 1;
        }
        // TODO: we can batch as many vpm entries as fit in a mailbox (1 page)
        // rather than 1 chunk per mailbox, this will make bootup faster
        let (mut vpm_pages, vpm_physical) = create_contiguous_mapping(size_of::<VirtualPhysicalMapping>(), DMA_FLAGS)?;
        let vpm: &mut VirtualPhysicalMapping = vpm_pages.as_type_mut(0)?;
        let mut pointer = physical;
        for _ in 0..count {
            vpm.physical_address_high.set((pointer.value() >> 32).try_into().unwrap());
            vpm.physical_address_low.set((pointer.value() & 0xffffffff).try_into().unwrap());
            cmd.execute_command(Opcode::MapFa, Some(vpm_physical), 1, None)?;
            pointer += 1 << align;
        }
        trace!("mapped {} pages for firmware area", self.pages);

        Ok(MappedFirmwareArea { pages, physical, })
    }
}

impl core::fmt::Debug for Firmware {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("Firmware")
            .field("clr_int_bar", &self.clr_int_bar)
            .field("clr_int_base", &format_args!("{:#x}", self.clr_int_base))
            .field("version", &format_args!("{}.{}.{}", self.major, self.minor, self.sub_minor))
            .field("ix_rev", &self.ix_rev.get())
            .field("size", &format_args!(
                "{}.{} KB",
                (self.pages.get() as usize * PAGE_SIZE) / 1024,
                (self.pages.get() as usize * PAGE_SIZE) % 1024,
            ))
            .finish()
    }
}


#[derive(Clone, FromBytes, Default)]
#[repr(C, packed)]
struct VirtualPhysicalMapping {
    virtual_address_high: U32<BigEndian>,
    // actually just 20 bits
    virtual_address_low: U32<BigEndian>,
    physical_address_high: U32<BigEndian>,
    // actually just 20 bits and then log2size
    physical_address_low: U32<BigEndian>,
}

/// A mapped firmware area.
/// 
/// Instead of dropping, please unmap the area from the card.
pub(super) struct MappedFirmwareArea {
    pages: MappedPages,
    physical: PhysicalAddress,
}

impl MappedFirmwareArea {
    /// Unmaps the area from the card. Further usage requires a software reset.
    pub(super) fn unmap(self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        trace!("unmapping firmware area...");
        let mut cmd = CommandMailBox::new(config_regs)?;
        cmd.execute_command(Opcode::UnmapFa, None, 0, None)?;
        Ok(())
    }
}

impl Drop for MappedFirmwareArea {
    fn drop(&mut self) {
        panic!("please unmap instead of dropping")
    }
}
