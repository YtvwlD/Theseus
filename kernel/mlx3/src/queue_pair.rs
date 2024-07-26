//! This module consists of functions that create, work with and destroy queue
//! pairs. Its functions can change the state of a QP and query and print some
//! QP infos.

use core::mem::size_of;

use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use mlx_infiniband::{ibv_qp_cap, ibv_qp_state, ibv_qp_type};
use volatile::WriteOnly;
use zerocopy::{FromBytes, U16};

use crate::completion_queue::CompletionQueue;

use super::{cmd::CommandInterface, fw::Capabilities, icm::MrTable, Offsets};

const IB_SQ_MIN_WQE_SHIFT: u32 = 6;
const IB_MAX_HEADROOM: u32 = 2048;
const IB_SQ_MAX_SPARE: u32 = ib_sq_headroom(IB_SQ_MIN_WQE_SHIFT);

const fn ib_sq_headroom(shift: u32) -> u32 {
    (IB_MAX_HEADROOM >> shift) + 1
}

#[derive(Debug)]
pub(super) struct QueuePair {
    number: usize,
    state: ibv_qp_state,
    qp_type: ibv_qp_type::Type,
    // TODO: this seems deprecated
    is_special: bool,
    // TODO: bind the lifetime to the one of the completion queues
    send_cq_number: usize,
    receive_cq_number: usize,
    memory: Option<(MappedPages, PhysicalAddress)>,
    uar_idx: usize,
    doorbell_page: MappedPages,
    mtt: u64,
}

impl QueuePair {
    /// Create a new queue pair.
    /// 
    /// This includes allocating the area for the buffer itself and allocating
    /// an MTT entry for the buffer. It does *not* allocate a send queue or
    /// receive queue for the work queue.
    /// 
    /// This is similar to creating a completion queue or an event queue.
    pub(super) fn new(
        cmd: &mut CommandInterface, caps: &Capabilities, offsets: &mut Offsets,
        memory_regions: &mut MrTable, qp_type: ibv_qp_type::Type,
        send_cq: &CompletionQueue, receive_cq: &CompletionQueue,
        ib_caps: &mut ibv_qp_cap,
    ) -> Result<Self, &'static str> {
        let number = offsets.alloc_qpn();
        let uar_idx = offsets.alloc_scq_db();
        let state = ibv_qp_state::IBV_QPS_RESET;
        let send_cq_number = send_cq.number();
        let receive_cq_number = receive_cq.number();
        let is_special = false;
        let mut rq = WorkQueue::new_receive_queue(caps, ib_caps)?;
        let mut sq = WorkQueue::new_send_queue(caps, ib_caps, qp_type)?;
        if rq.wqe_shift > sq.wqe_shift {
            rq.offset = 0;
            sq.offset = rq.size();
        } else {
            rq.offset = sq.size();
            sq.offset = 0;
        }
        let buf_size = (rq.size() + sq.size()).try_into().unwrap();
        let mut memory = create_contiguous_mapping(
            buf_size, DMA_FLAGS,
        )?;
        // zero the queue
        memory.0
            .as_slice_mut(0, buf_size)
            .expect("failed to write to memory")
            .fill(0u8);
        let mtt = memory_regions.alloc_mtt(
            cmd, caps, buf_size / PAGE_SIZE, memory.1,
        )?;
        let (mut doorbell_page, _doorbell_address) = create_contiguous_mapping(
            size_of::<QueuePairDoorbell>(), DMA_FLAGS
        )?;
        let doorbell: &mut QueuePairDoorbell = doorbell_page
            .as_type_mut(0)?;
        doorbell.receive_wqe_index.write(0.into());
        let qp = Self {
            number, state, qp_type, is_special, send_cq_number,
            receive_cq_number, memory: Some(memory), uar_idx, doorbell_page,
            mtt,
        };
        trace!("created new QP: {qp:?}");
        Ok(qp)
    }

    /// Destroy this completion queue.
    pub(super) fn destroy(
        mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        // TODO: transition to RESET if not already there
        // actually free the memory
        self.memory.take().unwrap();
        Ok(())
    }
    
    /// Get the number of this queue pair.
    pub(super) fn number(&self) -> usize {
        self.number
    }
}

impl Drop for QueuePair {
    fn drop(&mut self) {
        if self.memory.is_some() {
            panic!("please destroy instead of dropping")
        }
    }
}

#[derive(FromBytes)]
#[repr(C, packed)]
struct QueuePairDoorbell {
    _reserved: u16,
    receive_wqe_index: WriteOnly<U16<BigEndian>>,
}

struct WorkQueue {
    wqe_cnt: u32,
    max_post: u32,
    max_gs: u32,
    offset: u32,
    wqe_shift: u32,
    spare_wqes: Option<u32>,
}

impl WorkQueue {
    /// Compute the size of the receive queue and return it.
    fn new_receive_queue(
        hca_caps: &Capabilities, ib_caps: &mut ibv_qp_cap,
    ) -> Result<Self, &'static str> {
        // check the RQ size before proceeding
        if ib_caps.max_recv_wr > 1 << u32::from(hca_caps.log_max_qp_sz()) - IB_SQ_MAX_SPARE
         || ib_caps.max_recv_sge > hca_caps.max_sg_sq().into()
         || ib_caps.max_recv_sge > hca_caps.max_sg_rq().into() {
            return Err("RQ size is invalid")
        }
        let mut wqe_cnt = ib_caps.max_recv_wr;
        if wqe_cnt < 1 {
            wqe_cnt = 1;
        }
        wqe_cnt = wqe_cnt.next_power_of_two();
        let mut max_gs = ib_caps.max_recv_sge;
        if max_gs < 1 {
            max_gs = 1;
        }
        max_gs = max_gs.next_power_of_two();
        let wqe_shift = (
            max_gs * u32::try_from(size_of::<WqeDataSegment>()).unwrap()
        ).ilog2();
        let mut max_post = 1 << u32::from(
            hca_caps.log_max_qp_sz()
        ) - IB_SQ_MAX_SPARE;
        if max_post > wqe_cnt {
            max_post = wqe_cnt;
        }
        // update the caps
        ib_caps.max_recv_wr = max_post;
        ib_caps.max_recv_sge = *[
            max_gs, hca_caps.max_sg_sq().into(), hca_caps.max_sg_rq().into(),
        ].iter().min().unwrap();
        Ok(Self {
            wqe_cnt, max_post, max_gs, offset: 0, wqe_shift,
            spare_wqes: None,
        })
    }
    
    /// Compute the size of the receive queue and return it.
    fn new_send_queue(
        hca_caps: &Capabilities, ib_caps: &mut ibv_qp_cap,
        qp_type: ibv_qp_type::Type,
    ) -> Result<Self, &'static str> {
        // check the SQ size before proceeding
        if ib_caps.max_send_wr > 1 << u32::from(hca_caps.log_max_qp_sz()) - IB_SQ_MAX_SPARE
         || ib_caps.max_send_sge > hca_caps.max_sg_sq().into()
         || ib_caps.max_send_sge > hca_caps.max_sg_rq().into() {
            return Err("SQ size is invalid")
        }
        let size = ib_caps.max_send_sge * u32::try_from(
            size_of::<WqeDataSegment>()
        ).unwrap() + send_wqe_overhead(qp_type);
        if size > hca_caps.max_desc_sz_sq().into() {
            return Err("SQ size is invalid")
        }
        let wqe_shift = size.next_power_of_two().ilog2();
        // We need to leave 2 KB + 1 WR of headroom in the SQ to allow HW to prefetch.
        let spare_wqes = ib_sq_headroom(wqe_shift);
        let wqe_cnt = (ib_caps.max_send_wr + spare_wqes).next_power_of_two();
        let max_gs = (u32::from(*[
            hca_caps.max_desc_sz_sq(), 1 << wqe_shift
        ].iter().min().unwrap()) - send_wqe_overhead(qp_type)) / u32::try_from(
            size_of::<WqeDataSegment>()
        ).unwrap();
        let max_post = wqe_cnt - spare_wqes;
        // update the caps
        ib_caps.max_send_wr = max_post;
        ib_caps.max_send_sge = *[
            max_gs, hca_caps.max_sg_sq().into(), hca_caps.max_sg_rq().into(),
        ].iter().min().unwrap();
        Ok(Self {
            wqe_cnt, max_post, max_gs, offset: 0, wqe_shift,
            spare_wqes: Some(spare_wqes),
        })
    }
    
    /// Get the size.
    fn size(&self) -> u32 {
        self.wqe_cnt << self.wqe_shift
    }
}

fn send_wqe_overhead(qp_type: ibv_qp_type::Type) -> u32 {
    // UD WQEs must have a datagram segment.
    // RC and UC WQEs might have a remote address segment.
    // MLX WQEs need two extra inline data segments (for the UD header and space
    // for the ICRC).
    match qp_type {
        ibv_qp_type::IBV_QPT_UD => {
            size_of::<WqeControlSegment>() + size_of::<WqeDatagramSegment>()
        },
        ibv_qp_type::IBV_QPT_UC => {
            size_of::<WqeControlSegment>() + size_of::<WqeRemoteAddressSegment>()
        },
        ibv_qp_type::IBV_QPT_RC => {
            size_of::<WqeControlSegment>() /* + size_of::<WqeMaskedAtomicSegment>() */
            + size_of::<WqeRemoteAddressSegment>()
        },
        _ => {
            size_of::<WqeControlSegment>()
        },
    }.try_into().unwrap()
}

#[repr(C)]
struct WqeControlSegment {
    owner_opcode: u32,
    vlan_cv_f_ds: u32,
    flags: u32,
    flags2: u32,
}

#[repr(C)]
struct WqeDataSegment {
    byte_count: u32,
    lkey: u32,
    addr: u64,
}

const ETH_ALEN: usize = 6;

#[repr(C)]
struct WqeDatagramSegment {
    av: [u32; 8],
    dst_qpn: u32,
    qkey: u32,
    vlan: u16,
    mac: [u8; ETH_ALEN],
}

#[repr(C)]
struct WqeRemoteAddressSegment {
    va: u64,
    key: u32,
    rsvd: u32,
}
