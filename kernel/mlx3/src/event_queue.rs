//! This module consists of functions that create, work with and destroy event queues.
//! Additionally it holds the interrupt handling function to consume EQEs.

use alloc::vec::Vec;
use bitflags::bitflags;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, VirtualAddress, DMA_FLAGS, PAGE_SIZE};
use modular_bitfield_msb::{bitfield, specifiers::{B10, B16, B2, B22, B24, B4, B40, B5, B6, B60, B7, B72, B96}};

use crate::{cmd::{CommandInterface, Opcode}, icm::ICM_PAGE_SHIFT};

use super::{fw::{Capabilities, PAGE_SHIFT}, icm::MrTable};

/// Initialize the event queues.
/// This creates all of the EQs ahead of time,
/// passes their ownership to the hardware and calls MapEq.
pub(super) fn init_eqs(
    cmd: &mut CommandInterface, user_access_region: &mut MappedPages,
    caps: &Capabilities, offsets: &mut Offsets, memory_regions: &mut MrTable,
) -> Result<Vec<EventQueue>, &'static str> {
    const NUM_EQS: usize = 1;
    let mut eqs = Vec::with_capacity(NUM_EQS);
    for _ in 0..NUM_EQS {
        // TODO: use interrupts here
        let eq = EventQueue::new(
            cmd, user_access_region, caps, offsets, memory_regions, None,
        )?;
        eqs.push(eq);
    }
    // map all events to the first (and only) event queue
    eqs[0].map(cmd)?;
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
    /// event bitmask
    async_ev_mask: AsyncEventMask,
}

impl EventQueue {
    // Create a new event queue. If `base_vector` is given, it will be interrupt
    // driven, else it will be polled.
    fn new(
        cmd: &mut CommandInterface, user_access_region: &mut MappedPages,
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
        let mtt = memory_regions.alloc_mtt(cmd, caps, num_pages, physical)?;
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
        cmd.execute_command(
            Opcode::Sw2HwEq, &ctx.bytes[..],
            number.try_into().unwrap(),
        )?;

        let async_ev_mask = AsyncEventMask::empty();
        let eq = Self {
            number, num_entries, num_pages, pages, physical, mtt, doorbell,
            consumer_index, intr_vector, base_vector, uar_map, async_ev_mask,
        };
        trace!("created new EQ: {:?}", eq);
        Ok(eq)
    }
    
    /// Map all event types to this EQ.
    // TODO: should parameterize the types of events given to this EQ
    fn map(&mut self, cmd: &mut CommandInterface) -> Result<(), &'static str> {
        // TODO: unmask IRQ
        self.async_ev_mask = AsyncEventMask::all();
        let unmap = false;
        cmd.execute_command(
            Opcode::MapEq, self.async_ev_mask.bits(),
            ((unmap as u32) << 31) | u32::try_from(self.number).unwrap(),
        )?;
        Ok(())
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

#[repr(u64)]
enum EventType {
    // completion
    Completion = 0x00,

    // IB affiliated events
    PathMigrationSucceeded = 0x01,
    CommunicationEstablished = 0x02,
    SendQueueDrained = 0x03,
    SrqLastWqe = 0x13,
    SrqLimit = 0x14,

    // QP affiliated errors
    CqError = 0x04,
    WqCatastrophicError = 0x05,
    EecCatastrophicError = 0x06,
    PathMigrationFailed = 0x07,
    WqInvalidRequestError = 0x10,
    WqAccessViolation = 0x11,
    SrqCatastropicError = 0x12,

    // unaffiliated events and errors
    InternalError = 0x08,
    PortChange = 0x09,
    // EqOverflow = 0x0f,
    // EccDetect = 0x0e,
    // VepUpdate = 0x19,
    // OpRequired = 0x1a,
    FatalWarning = 0x1b,
    FlrEvent = 0x1c,
    PortManagementChange = 0x1d,
    RecoverableEvent = 0x3e,
    // None = 0xff,

    // HCA interface
    CommandInterfaceCompletion = 0x0a,
    CommunicationChannelWritten = 0x18,

}

bitflags! {
    #[derive(Debug)]
    pub struct AsyncEventMask: u64 {
        // IB affiliated
        const PATH_MIGRATION_SUCCEEDED = 1 << EventType::PathMigrationSucceeded as u64;
        const COMMUNICATION_ESTABLISHED = 1 << EventType::CommunicationEstablished as u64;
        const SEND_QUEUE_DRAINED = 1 << EventType::SendQueueDrained as u64;
        const SRQ_LAST_WQE = 1 << EventType::SrqLastWqe as u64;
        const SRQ_LIMIT = 1 << EventType::SrqLimit as u64;
        
        // QP affiliated errors
        const CQ_ERROR = 1 << EventType::CqError as u64;
        const WQ_CATASTROPHIC_ERROR = 1 << EventType::WqCatastrophicError as u64;
        const EEC_CATASTROPHIC_ERROR = 1 << EventType::EecCatastrophicError as u64;
        const PATH_MIGRATION_FAILED = 1 << EventType::PathMigrationFailed as u64;
        const WQ_INVALID_REQUEST_ERROR = 1 << EventType::WqInvalidRequestError as u64;
        const WQ_ACCESS_VIOLATION = 1 << EventType::WqAccessViolation as u64;
        const SRQ_CATASTROPHIC_ERROR = 1 << EventType::SrqCatastropicError as u64;

        // unaffiliated events and errors
        const INTERNAL_ERROR = 1 << EventType::InternalError as u64;
        const PORT_CHANGE = 1 << EventType::PortChange as u64;
        const FATAL_WARNING = 1 << EventType::FatalWarning as u64;
        const FLR_EVENT = 1 << EventType::FlrEvent as u64;
        const PORT_MANAGEMENT_CHANGE = 1 << EventType::PortManagementChange as u64;
        const RECOVERABLE_EVENT = 1 << EventType::RecoverableEvent as u64;

        // HCA interface
        const COMMAND_INTERFACE_COMPLETION = 1 << EventType::CommandInterfaceCompletion as u64;
        const COMMUNICATION_CHANNEL_WRITTEN = 1 << EventType::CommunicationChannelWritten as u64;
    }
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
