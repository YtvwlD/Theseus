//! This module contains functionality to interact with the firmware.

use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, DMA_FLAGS, PAGE_SIZE};
use zerocopy::{FromBytes, U16, U64};

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
        cmd.execute_command(Opcode::QueryFw, None, Some(physical))?;
        let mut fw = pages.as_type::<Firmware>(0)?.clone();
        fw.clr_int_bar = (fw.clr_int_bar >> 6) * 2;
        trace!("got firmware info: {fw:?}");
        Ok(fw)
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
