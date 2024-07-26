//! This module consists of functions that create, work with and destroy
//! completion queues. Furthermore its functions can consume and print
//! completion queue elements.

use core::mem::size_of;

use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use modular_bitfield_msb::{bitfield, specifiers::{B2, B24, B3, B40, B48, B5, B6}};
use volatile::WriteOnly;
use zerocopy::{FromBytes, U32};

use super::{
    cmd::{CommandInterface, Opcode},
    device::{uar_index_to_hw, PAGE_SHIFT},
    event_queue::EventQueue,
    fw::{Capabilities, DoorbellPage},
    icm::{MrTable, ICM_PAGE_SHIFT},
    Offsets,
};

#[derive(Debug)]
pub(super) struct CompletionQueue {
    number: usize,
    num_entries: usize,
    num_pages: usize,
    memory: Option<(MappedPages, PhysicalAddress)>,
    uar_idx: usize,
    doorbell_page: MappedPages,
    mtt: u64,
    arm_sequence_number: u32,
    consumer_index: u32,
    // TODO: bind the lifetime to the one of the event queue
    eq_number: Option<usize>,
}

impl CompletionQueue {
    /// Create a new completion queue.
    /// 
    /// This is quite like creating an event queue.
    pub(super) fn new(
        cmd: &mut CommandInterface, caps: &Capabilities, offsets: &mut Offsets,
        memory_regions: &mut MrTable, eq: Option<&EventQueue>,
        num_entries: usize,
    ) -> Result<Self, &'static str> {
        // CQE size is 32. There is 64 B support also available in CX3.
        const CQE_SIZE: usize = 32;
        let number = offsets.alloc_cqn();
        let uar_idx = offsets.alloc_scq_db();
        let num_pages = (num_entries * CQE_SIZE).next_multiple_of(PAGE_SIZE) / PAGE_SIZE;
        let memory = create_contiguous_mapping(
            num_pages * PAGE_SIZE + CQE_SIZE - 1, DMA_FLAGS,
        )?;
        let mtt = memory_regions.alloc_mtt(cmd, caps, num_pages, memory.1)?;
        let (mut doorbell_page, doorbell_address) = create_contiguous_mapping(
            size_of::<CompletionQueueDoorbell>(), DMA_FLAGS
        )?;
        let doorbell: &mut CompletionQueueDoorbell = doorbell_page
            .as_type_mut(0)?;
        doorbell.update_consumer_index.write(0.into());
        doorbell.arm_consumer_index.write(0.into());
        let arm_sequence_number = 0;
        let consumer_index = 0;

        let mut ctx = CompletionQueueContext::new();
        ctx.set_log_size(num_entries.ilog2().try_into().unwrap());
        ctx.set_usr_page(uar_index_to_hw(uar_idx).try_into().unwrap());
        let mut eq_number = None;
        if let Some(eq) = eq {
            ctx.set_comp_eqn(eq.number().try_into().unwrap());
            eq_number = Some(eq.number());
        }
        ctx.set_log_page_size(PAGE_SHIFT - ICM_PAGE_SHIFT);
        ctx.set_mtt_base_addr(mtt);
        ctx.set_doorbell_record_addr(doorbell_address.value() as u64);
        cmd.execute_command(
            Opcode::Sw2HwCq, (), &ctx.bytes[..], number.try_into().unwrap(),
        )?;

        let cq = Self {
            number, num_entries, num_pages, memory: Some(memory), uar_idx,
            doorbell_page, mtt, arm_sequence_number, consumer_index, eq_number,
        };
        trace!("created new CQ: {:?}", cq);
        Ok(cq)
    }

    /// Destroy this completion queue.
    pub(super) fn destroy(
        mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        // TODO: should make sure to undo all card state tied to this CQ
        cmd.execute_command(
            Opcode::Hw2SwCq, (), (), self.number.try_into().unwrap(),
        )?;
        // actually free the mememory
        self.memory.take().unwrap();
        Ok(())
    }

    /// Arm this completion queue by writing the consumer index to the
    /// appropriate doorbell.
    pub(super) fn arm(
        &mut self, doorbells: &mut [MappedPages],
    ) -> Result<(), &'static str> {
        const _DOORBELL_REQUEST_NOTIFICATION_SOLICITED: u32 = 0x1;
        const DOORBELL_REQUEST_NOTIFICATION: u32 = 0x2;
        let sn = self.arm_sequence_number & 3;
        let ci = self.consumer_index & 0xffffff;
        let cmd = DOORBELL_REQUEST_NOTIFICATION;
        let doorbell_record: &mut CompletionQueueDoorbell = self.doorbell_page
            .as_type_mut(0)?;
        doorbell_record.arm_consumer_index.write(
            (sn << 28 | cmd << 24 | ci).into()
        );
        // TODO: barrier
        let doorbell: &mut DoorbellPage = doorbells[self.uar_idx]
            .as_type_mut(0)?;
        doorbell.cq_sn_cmd_num.write(
            (sn << 28 | cmd << 24 | u32::try_from(self.number).unwrap()).into()
        );
        doorbell.cq_consumer_index.write(ci.into());
        Ok(())
    }

    /// Get the number of this completion queue.
    pub(super) fn number(&self) -> usize {
        self.number
    }
}

impl Drop for CompletionQueue {
    fn drop(&mut self) {
        if self.memory.is_some() {
            panic!("please destroy instead of dropping")
        }
    }
}

#[bitfield]
struct CompletionQueueContext {
    flags: u32,
    #[skip] __: B48,
    page_offset: u16,
    #[skip] __: B3,
    log_size: B5,
    usr_page: B24,
    cq_period: u16,
    cq_max_count: u16,
    #[skip] __: B24,
    comp_eqn: u8,
    #[skip] __: B2,
    log_page_size: B6,
    #[skip] __: u16,
    // the last three bits must be zero
    mtt_base_addr: B40,
    #[skip] __: u8,
    last_notified_index: B24,
    #[skip] __: u8,
    solicit_producer_index: B24,
    #[skip] __: u8,
    consumer_index: B24,
    #[skip] __: u8,
    producer_index: B24,
    #[skip] __: u64,
    // the last three bits must be zero
    doorbell_record_addr: u64,
}

#[derive(FromBytes)]
#[repr(C, packed)]
struct CompletionQueueDoorbell {
    update_consumer_index: WriteOnly<U32<BigEndian>>,
    arm_consumer_index: WriteOnly<U32<BigEndian>>,
}
