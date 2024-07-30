//! This module consists of functions that create, work with and destroy queue
//! pairs. Its functions can change the state of a QP and query and print some
//! QP infos.

use core::mem::size_of;

use bitflags::bitflags;
use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use mlx_infiniband::{ibv_access_flags, ibv_qp_attr, ibv_qp_attr_mask, ibv_qp_cap, ibv_qp_state, ibv_qp_type};
use modular_bitfield_msb::{bitfield, prelude::{B12, B16, B17, B19, B2, B20, B24, B3, B4, B40, B48, B5, B56, B6, B7, B72}};
use volatile::WriteOnly;
use zerocopy::{AsBytes, FromBytes, U16, U32};

use crate::{cmd::Opcode, completion_queue::CompletionQueue, device::{uar_index_to_hw, PAGE_SHIFT}, icm::ICM_PAGE_SHIFT};

use super::{cmd::CommandInterface, fw::Capabilities, icm::MrTable, Offsets};

const IB_SQ_MIN_WQE_SHIFT: u32 = 6;
const IB_MAX_HEADROOM: u32 = 2048;
const IB_SQ_MAX_SPARE: u32 = ib_sq_headroom(IB_SQ_MIN_WQE_SHIFT);

const fn ib_sq_headroom(shift: u32) -> u32 {
    (IB_MAX_HEADROOM >> shift) + 1
}

#[derive(Debug)]
pub(super) struct QueuePair {
    number: u32,
    state: ibv_qp_state,
    qp_type: ibv_qp_type::Type,
    // TODO: this seems deprecated
    is_special: bool,
    sq: WorkQueue,
    rq: WorkQueue,
    // TODO: bind the lifetime to the one of the completion queues
    send_cq_number: u32,
    receive_cq_number: u32,
    memory: Option<(MappedPages, PhysicalAddress)>,
    uar_idx: usize,
    doorbell_page: MappedPages,
    doorbell_address: PhysicalAddress,
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
        let number = offsets.alloc_qpn().try_into().unwrap();
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
        let (mut doorbell_page, doorbell_address) = create_contiguous_mapping(
            size_of::<QueuePairDoorbell>(), DMA_FLAGS
        )?;
        let doorbell: &mut QueuePairDoorbell = doorbell_page
            .as_type_mut(0)?;
        doorbell.receive_wqe_index.write(0.into());
        let qp = Self {
            number, state, qp_type, is_special, sq, rq, send_cq_number,
            receive_cq_number, memory: Some(memory), uar_idx,
            doorbell_page, doorbell_address, mtt,
        };
        trace!("created new QP: {qp:?}");
        Ok(qp)
    }

    /// Modify this queue pair.
    /// 
    /// This is used by ibv_modify_qp.
    pub(super) fn modify(
        &mut self, cmd: &mut CommandInterface,
        attr: &ibv_qp_attr, attr_mask: ibv_qp_attr_mask,
    ) -> Result<(), &'static str> {
        // TODO: this discards any parameters that aren't needed for the current transition
        // TODO: perhaps query before so that we have the current state
        const _PATH_MIGRATION_STATE_ARMED: u8 = 0x0;
        const _PATH_MIGRATION_STATE_REARM: u8 = 0x1;
        const PATH_MIGRATION_STATE_MIGRATED: u8 = 0x3;
        // create the context
        let mut context = QueuePairContext::new();
        let mut param_mask = OptionalParameterMask::empty();
        // get the right state transition
        let opcode = match (self.state, attr_mask.contains(
                ibv_qp_attr_mask::IBV_QP_STATE
        ), attr.qp_state) {
            // initialize
            (ibv_qp_state::IBV_QPS_RESET, true, ibv_qp_state::IBV_QPS_INIT) => {
                // set required fields
                context.set_service_type(match self.qp_type {
                    ibv_qp_type::IBV_QPT_RC => 0x0,
                    ibv_qp_type::IBV_QPT_UC => 0x1,
                    ibv_qp_type::IBV_QPT_UD => 0x3,
                });
                context.set_path_migration_state(PATH_MIGRATION_STATE_MIGRATED);
                context.set_usr_page(uar_index_to_hw(
                    self.uar_idx
                ).try_into().unwrap());
                // TODO: protection domain
                context.set_cqn_send(self.send_cq_number);
                // RC needs remote read
                if self.qp_type == ibv_qp_type::IBV_QPT_RC {
                    // TODO: this might have been set in an earlier call
                    assert!(attr_mask.contains(
                        ibv_qp_attr_mask::IBV_QP_ACCESS_FLAGS
                    ));
                    context.set_remote_read(attr.qp_access_flags.contains(
                        ibv_access_flags::IBV_ACCESS_REMOTE_READ
                    ));
                }
                // RC and UC need remote write
                if self.qp_type == ibv_qp_type::IBV_QPT_RC
                    || self.qp_type == ibv_qp_type::IBV_QPT_UC {
                    // TODO: this might have been set in an earlier call
                    assert!(attr_mask.contains(
                        ibv_qp_attr_mask::IBV_QP_ACCESS_FLAGS
                    ));
                    context.set_remote_write(attr.qp_access_flags.contains(
                        ibv_access_flags::IBV_ACCESS_REMOTE_WRITE
                    ));
                }
                // RC needs remote atomic
                if self.qp_type == ibv_qp_type::IBV_QPT_RC {
                    // TODO: this might have been set in an earlier call
                    assert!(attr_mask.contains(
                        ibv_qp_attr_mask::IBV_QP_ACCESS_FLAGS
                    ));
                    context.set_remote_atomic(
                        attr.qp_access_flags.contains(
                            ibv_access_flags::IBV_ACCESS_REMOTE_ATOMIC
                        )
                    );
                }
                context.set_cqn_receive(self.receive_cq_number);
                // UD needs qkey
                if self.qp_type == ibv_qp_type::IBV_QPT_UD {
                    // TODO: this might have been set in an earlier call
                    assert!(attr_mask.contains(
                        ibv_qp_attr_mask::IBV_QP_QKEY
                    ));
                    context.set_qkey(attr.qkey);
                }
                // TODO: RC and UD need srq
                // TODO: RC and UD need srqn
                // TODO: fre
                assert_ne!(self.sq.wqe_cnt, 0);
                context.set_log_sq_size(
                    self.sq.wqe_cnt.ilog2().try_into().unwrap()
                );
                assert_ne!(self.rq.wqe_cnt, 0);
                context.set_log_rq_size(
                    self.rq.wqe_cnt.ilog2().try_into().unwrap()
                );
                context.set_log_sq_stride(
                    (self.sq.wqe_shift - 4).try_into().unwrap()
                );
                context.set_log_rq_stride(
                    (self.rq.wqe_shift - 4).try_into().unwrap()
                );
                // since we can't allocate protection domains,
                // allow using the reserved lkey to refer directly to physical
                // addresses
                context.set_reserved_lkey(true);
                // TODO: sq_wqe_counter, rq_wqe_counter, is
                // TODO: hs, vsd, rss for UD
                context.set_sq_no_prefetch(false);
                // TODO: page_offset, pkey_index, disable_pkey_check
                // TOODO: rss context for UD
                context.set_log_page_size(PAGE_SHIFT - ICM_PAGE_SHIFT);
                context.set_mtt_base_addr(self.mtt);
                context.set_db_record_addr(
                    self.doorbell_address.value().try_into().unwrap()
                );

                // Before passing a kernel QP to the HW, make sure that the
                // ownership bits of the send queue are set and the SQ headroom
                // is stamped so that the hardware doesn't start processing
                // stale work requests.
                for i in 0..self.sq.wqe_cnt {
                    let ctrl = self.sq.get_element(
                        self.memory.as_mut().unwrap(), i,
                    )?;
                    ctrl.owner_opcode = (1 << 31).into();
                    ctrl.vlan_cv_f_ds = u32::to_be(
                        1 << (self.sq.wqe_shift - 4)
                    ).into();
                    ctrl.stamp();
                }
                Opcode::Rst2InitQp
            },
            // or just stay in the current state
            // We can't even set anything here.
            (ibv_qp_state::IBV_QPS_RESET, false, _) => Opcode::Any2RstQp,

            // init -> rtr
            (ibv_qp_state::IBV_QPS_INIT, true, ibv_qp_state::IBV_QPS_RTR) => {
                todo!()
            }
            // or just stay in the current state
            (ibv_qp_state::IBV_QPS_INIT, true, ibv_qp_state::IBV_QPS_INIT)
             | (ibv_qp_state::IBV_QPS_INIT,false, _) => {
                // can update qkey for UD
                if self.qp_type == ibv_qp_type::IBV_QPT_UD {
                    if attr_mask.contains(ibv_qp_attr_mask::IBV_QP_QKEY) {
                        context.set_qkey(attr.qkey);
                        param_mask.insert(OptionalParameterMask::QKEY);
                    }
                }
                // can update pkey_index
                if attr_mask.contains(ibv_qp_attr_mask::IBV_QP_PKEY_INDEX) {
                    let mut primary_path = context.primary_path_one();
                    primary_path.set_pkey_index(
                        attr.pkey_index.try_into().unwrap()
                    );
                    context.set_primary_path_one(primary_path);
                    param_mask.insert(OptionalParameterMask::PKEY_INDEX);
                }
                // can update access flags for RC and UC
                if self.qp_type == ibv_qp_type::IBV_QPT_RC
                    || self.qp_type == ibv_qp_type::IBV_QPT_UC {
                    if attr_mask.contains(
                        ibv_qp_attr_mask::IBV_QP_ACCESS_FLAGS
                    ) {
                        context.set_remote_write(
                            attr.qp_access_flags.contains(
                                ibv_access_flags::IBV_ACCESS_REMOTE_WRITE
                            )
                        );
                        context.set_remote_atomic(
                            attr.qp_access_flags.contains(
                                ibv_access_flags::IBV_ACCESS_REMOTE_ATOMIC
                            )
                        );
                        context.set_remote_read(
                            attr.qp_access_flags.contains(
                                ibv_access_flags::IBV_ACCESS_REMOTE_READ
                            )
                        );
                    }
                }
                Opcode::Init2InitQp
            },

            // resetting is always possible
            (_, true, ibv_qp_state::IBV_QPS_RESET) => Opcode::Any2RstQp,
            // nothing else is possible
            _ => return Err("invalid state transition"),
        };
        // actually execute the command
        let mut input = StateTransitionCommandParameter::new_zeroed();
        input.opt_param_mask.set(param_mask.bits());
        input.qpc_data = context.into_bytes();
        cmd.execute_command(
            opcode, (), input.as_bytes(), self.number,
        )?;
        if attr_mask.contains(ibv_qp_attr_mask::IBV_QP_STATE) {
            self.state = attr.qp_state;
            trace!("QP {} is now in {:?}", self.number, self.state);
        }
        // TODO: perhaps check if this worked
        Ok(())
    }

    /// Destroy this queue pair.
    pub(super) fn destroy(
        mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        trace!("destroying QP {}..", self.number);
        if self.state != ibv_qp_state::IBV_QPS_RESET {
            self.modify(cmd, &ibv_qp_attr {
                qp_state: ibv_qp_state::IBV_QPS_RESET,
                ..Default::default()
            }, ibv_qp_attr_mask::IBV_QP_STATE)?;
        }
        // actually free the memory
        self.memory.take().unwrap();
        Ok(())
    }
    
    /// Get the number of this queue pair.
    pub(super) fn number(&self) -> u32 {
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
    
    /// Get an element of this work queue.
    fn get_element<'e>(
        &self, memory: &'e mut (MappedPages, PhysicalAddress), index: u32,
    ) -> Result<&'e mut WqeControlSegment, &'static str> {
        let (pages, _addresss) = memory;
        pages.as_type_mut((self.offset + (index << self.wqe_shift)).try_into().unwrap())
    }
}

impl core::fmt::Debug for WorkQueue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WorkQueue").finish_non_exhaustive()
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

#[derive(FromBytes)]
#[repr(C)]
struct WqeControlSegment {
    owner_opcode: U32<BigEndian>,
    vlan_cv_f_ds: U32<BigEndian>,
    flags: U32<BigEndian>,
    flags2: U32<BigEndian>,
}

impl WqeControlSegment {
    /// Stamp this WQE so that it is invalid if prefetched by marking the
    /// first four bytes of every 64 byte chunk with 0xffffffff, except for
    /// the very first chunk of the WQE.
    fn stamp(&mut self) {
        let size = ((self.vlan_cv_f_ds.get() & 0x3f) << 4).try_into().unwrap();
        for i in (64..size).step_by(64) {
            // TODO: make this safe
            unsafe {
                let wqe: *mut u32 = (self as *mut _ as *mut u8)
                    .offset(i)
                    .cast();
                wqe.write(u32::MAX);
            }
        }
    }
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

#[bitfield]
struct QueuePairContext {
    state: B4,
    #[skip] __: B4,
    service_type: u8,
    #[skip] __: B3,
    path_migration_state: B2,
    #[skip] __: B19,
    protection_domain: B24,
    mtu: B3,
    msg_max: B5,
    #[skip] __: bool,
    log_rq_size: B4,
    log_rq_stride: B3,
    sq_no_prefetch: bool,
    log_sq_size: B4,
    log_sq_stride: B3,
    roce_mode: B2,
    #[skip] __: bool,
    reserved_lkey: bool,
    #[skip] __: B12,
    usr_page: B24,
    #[skip] __: u8,
    local_qpn: B24,
    #[skip] __: u8,
    remote_qpn: B24,
    // nested bitfields are only allowed to be 128 bits
    primary_path_one: QueuePairPathPartOne,
    primary_rgid: u128,
    primary_path_two: QueuePairPathPartTwo,
    alternative_path_one: QueuePairPathPartOne,
    alternative_rgid: u128,
    alternative_path_two: QueuePairPathPartTwo,
    #[skip] __: B72,
    next_send_psn: B24,
    #[skip] __: u8,
    cqn_send: B24,
    roce_entropy: u16,
    #[skip] __: B56,
    last_acked_psn: B24,
    #[skip] __: u8,
    ssn: B24,
    #[skip] __: u16,
    remote_read: bool,
    remote_write: bool,
    remote_atomic: bool,
    #[skip] __: B16,
    rnr_nak: B5,
    next_recv_psn: B24,
    #[skip] __: u16,
    xrcd: u16,
    #[skip] __: u8,
    cqn_receive: B24,
    /// The last three bits must be zero.
    db_record_addr: u64,
    qkey: u32,
    #[skip] __: u8,
    srqn: B24,
    #[skip] __: u8,
    msn: B24,
    rq_wqe_counter: u16,
    sq_wqe_counter: u16,
    // rate_limit_params
    #[skip] __: B56,
    qos_vport: u8,
    #[skip] __: u32,
    num_rmc_peers: u8,
    base_mkey: B24,
    #[skip] __: B2,
    log_page_size: B6,
    #[skip] __: u16,
    /// The last three bits must be zero.
    mtt_base_addr: B40,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u64,
}

// nested bitfields are only allowed to be 128 bits
#[bitfield]
#[derive(BitfieldSpecifier)]
struct QueuePairPathPartOne {
    #[skip] __: B17,
    disable_pkey_check: bool,
    #[skip] __: B7,
    pkey_index: B7,
    #[skip] __: u8,
    grh: bool,
    #[skip] __: B7,
    rlid: u16,
    ack_timeout: B5,
    #[skip] __: B4,
    mgid_index: B7,
    #[skip] __: u8,
    hop_limit: u8,
    #[skip] __: B4,
    tclass: u8,
    flow_label: B20,
}

#[bitfield]
#[derive(BitfieldSpecifier)]
struct QueuePairPathPartTwo {
    sched_queue: u8,
    #[skip] __: bool,
    vlan_index: B7,
    #[skip] __: u32,
    dmac: B48,
}

#[derive(AsBytes, FromBytes)]
#[repr(C, packed)]
struct StateTransitionCommandParameter {
    opt_param_mask: U32<BigEndian>,
    _reserved: u32,
    qpc_data: [u8; 248],
    _reserved2: [u8; 252],
}

bitflags! {
    struct OptionalParameterMask: u32 {
        const REMOTE_READ = 1 << 1;
        const REMOTE_ATOMIC = 1 << 2;
        const REMOTE_WRITE = 1 << 3;
        const PKEY_INDEX = 1 << 4;
        const QKEY = 1 << 5;
    }
}
