//! This module consists of functions that create, work with and destroy event queues.
//! Additionally it holds the interrupt handling function to consume EQEs.

use core::mem::size_of;

use alloc::vec::Vec;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, VirtualAddress, DMA_FLAGS, PAGE_SIZE};
use modular_bitfield_msb::{bitfield, specifiers::{B10, B16, B2, B22, B24, B4, B40, B5, B6, B60, B7, B72, B96}};

use crate::{cmd::{CommandMailBox, Opcode}, icm::ICM_PAGE_SHIFT};

use super::{fw::{Capabilities, PAGE_SHIFT}, icm::MrTable};

/// Initialize the event queues.
/// This creates all of the EQs ahead of time,
/// passes their ownership to the hardware and calls MapEq.
pub(super) fn init_eqs(
    config_regs: &mut MappedPages, user_access_region: &mut MappedPages,
    caps: &Capabilities, offsets: &mut Offsets, memory_regions: &mut MrTable,
) -> Result<Vec<EventQueue>, &'static str> {
    const NUM_EQS: usize = 1;
    let mut eqs = Vec::with_capacity(NUM_EQS);
    for _ in 0..NUM_EQS {
        // TODO: use interrupts here
        let eq = EventQueue::new(
            config_regs, user_access_region, caps, offsets, memory_regions, None,
        )?;
        eqs.push(eq);
    }
    // TODO: call MapEq
    Ok(eqs)
}

#[derive(Debug)]
pub(super) struct EventQueue {
    number: usize,
    num_entries: usize,
    num_pages: usize,
    pages: MappedPages,
    physical: PhysicalAddress,
    mtt: usize,
    doorbell: VirtualAddress,
    consumer_index: usize,
    /// IRQ number on bus
    intr_vector: Option<u8>,
    /// IRQ we will see
    base_vector: Option<u8>,
    uar_map: VirtualAddress,
}

impl EventQueue {
    // Create a new event queue. If `base_vector` is given, it will be interrupt
    // driven, else it will be polled.
    fn new(
        config_regs: &mut MappedPages, user_access_region: &mut MappedPages,
        caps: &Capabilities, offsets: &mut Offsets,
        memory_regions: &mut MrTable, base_vector: Option<u8>,
    ) -> Result<Self, &'static str> {
        // EQE size is 32. There is 64 B support also available in CX3.
        const EQE_SIZE: usize = 32;
        const EQ_STATUS_OK: u8 = 0;
        const EQ_STATE_ARMED: u8 = 9;
        const EQ_STATE_FIRED: u8 = 0xa;
        let number = offsets.alloc_eqn();
        let num_entries = 4096; // NUM_ASYNC_EQE + NUM_SPARE_EQE
        let consumer_index = 0;
        let mut num_pages = (num_entries * EQE_SIZE).next_multiple_of(PAGE_SIZE) / PAGE_SIZE;
        // not needed if 128 EQE entries
        if num_pages == 0 {
            num_pages = 1;
        }
        let (pages, physical) = create_contiguous_mapping(
            num_pages * PAGE_SIZE + EQE_SIZE - 1, DMA_FLAGS,
        )?;
        // this assumes we only have *one* EQ which is not a reserved eq!
        // each uar has 4 eq doorbells if uar is reserved even eq cannot be used
        let uar_map = user_access_region
            .address_at_offset((number / 4) << PAGE_SHIFT)
            .ok_or("failed to get UAR")?;
        let doorbell = uar_map + (0x800 + 8 * (number % 4));
        // TODO: pass pages here
        let mtt = memory_regions.alloc_mtt(config_regs, caps, num_pages, physical)?;
        // TODO: register interrupt correctly
        // TODO: Should use MSI-X instead of legacy INTs
        let intr_vector = base_vector.and_then(|_| todo!());

        let mut ctx = EventQueueContext::new();
        ctx.set_status(EQ_STATUS_OK);
        ctx.set_state(if base_vector.is_some() {
            EQ_STATE_ARMED
        } else {
            EQ_STATE_FIRED
        });
        ctx.set_log_eq_size(num_entries.ilog2().try_into().unwrap());
        if let Some(base_vector) = base_vector {
            ctx.set_intr(base_vector.try_into().unwrap());
        }
        ctx.set_log_page_size(PAGE_SHIFT - ICM_PAGE_SHIFT);
        ctx.set_mtt_base_addr(mtt.try_into().unwrap());
        let mut cmd = CommandMailBox::new(config_regs)?;
        let (mut command_pages, command_physical) = create_contiguous_mapping(
            size_of::<EventQueueContext>(), DMA_FLAGS,
        )?;
        command_pages.as_slice_mut(
            0, size_of::<EventQueueContext>()
        )?.copy_from_slice(&ctx.bytes);
        cmd.execute_command(
            Opcode::Sw2HwEq, command_physical.value() as u64,
            number.try_into().unwrap(), 0,
        )?;

        let eq = Self {
            number, num_entries, num_pages, pages, physical, mtt, doorbell,
            consumer_index, intr_vector, base_vector, uar_map,
        };
        trace!("created new EQ: {:?}", eq);
        Ok(eq)
    }
}

impl Drop for EventQueue {
    fn drop(&mut self) {
        todo!()
    }
}

#[bitfield]
struct EventQueueContext {
    status: B4,
    #[skip] __: B16,
    state: B4,
    #[skip] __: B60,
    page_offset: B7,
    #[skip] __: u8,
    log_eq_size: B5,
    #[skip] __: B24,
    eq_period: u16,
    eq_max_count: u16,
    #[skip] __: B22,
    intr: B10,
    #[skip] __: B2,
    log_page_size: B6,
    #[skip] __: u16,
    // the last three bits must be zero
    mtt_base_addr: B40,
    #[skip] __: B72,
    consumer_index: B24,
    #[skip] __: u8,
    producer_index: B24,
    #[skip] __: B96,
}

pub(super) struct Offsets {
    next_cqn: usize,
    next_qpn: usize,
    next_dmpt: usize,
    next_eqn: usize,
    next_sqc_doorbell_index: usize,
    next_eq_doorbell_index: usize,
}

impl Offsets {
    /// Initialize the queue offsets.
    pub(super) fn init(caps: &Capabilities) -> Self {
        Self {
            // This should return the first non reserved cq, qp, eq number.
            next_cqn: 1 << caps.log2_rsvd_cqs(),
            next_qpn: 1 << caps.log2_rsvd_qps(),
            next_dmpt: 1 << caps.log2_rsvd_mrws(),
            next_eqn: caps.num_rsvd_eqs().into(),
            // For SQ and CQ Uar Doorbell index starts from 128
            next_sqc_doorbell_index: 128,
            // Each UAR has 4 EQ doorbells; so if a UAR is reserved,
            // then we can't use any EQs whose doorbell falls on that page,
            // even if the EQ itself isn't reserved.
            next_eq_doorbell_index: caps.num_rsvd_eqs() as usize / 4,
        }
    }
    
    /// Allocate an event queue number.
    fn alloc_eqn(&mut self) -> usize {
        let res = self.next_eqn;
        self.next_eqn += 1;
        res
    }
}
