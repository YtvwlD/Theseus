//! This crate is a replacement for rdma-core on Linux.
//! 
//! The struct definitions are partly taken from the rust-bindgen output.
#![no_std]
#![allow(non_camel_case_types)]

use bitflags::bitflags;

pub use mlx3::Mtu as ibv_mtu;

pub mod ibv_qp_type {
    #[derive(Clone, Copy, PartialEq)]
    pub enum Type {
        IBV_QPT_RC, IBV_QPT_UC, IBV_QPT_UD,
    }
    pub use Type::IBV_QPT_RC;
    pub use Type::IBV_QPT_UC;
    pub use Type::IBV_QPT_UD;
}

pub type __be64 = u64;

bitflags! {
    #[derive(Default, Clone, Copy)]
    pub struct ibv_access_flags: i32 {
        const IBV_ACCESS_LOCAL_WRITE = 1;
        const IBV_ACCESS_REMOTE_WRITE = 2;
        const IBV_ACCESS_REMOTE_READ = 4;
        const IBV_ACCESS_REMOTE_ATOMIC = 8;
        const IBV_ACCESS_MW_BIND = 16;
        const IBV_ACCESS_ZERO_BASED = 32;
        const IBV_ACCESS_ON_DEMAND = 64;
        const IBV_ACCESS_HUGETLB = 128;
        const IBV_ACCESS_RELAXED_ORDERING = 1048576;
    }
}

pub struct ibv_context_ops {
    pub poll_cq: Option<unsafe fn(
        *mut ibv_cq, i32, *mut ibv_wc,
    ) -> i32>,
    pub post_send: Option<unsafe fn(
        *mut ibv_qp, *mut ibv_send_wr, *mut *mut ibv_send_wr,
    ) -> i32>,
    pub post_recv: Option<unsafe fn(
        *mut ibv_qp, *mut ibv_recv_wr, *mut *mut ibv_recv_wr,
    ) -> i32>,
}

bitflags! {
    #[derive(Default, PartialEq, Eq)]
    pub struct ibv_port_state: i32 {
        const IBV_PORT_NOP = 0;
        const IBV_PORT_DOWN = 1;
        const IBV_PORT_INIT = 2;
        const IBV_PORT_ARMED = 3;
        const IBV_PORT_ACTIVE = 4;
        const IBV_PORT_ACTIVE_DEFER = 5;
    }
}

pub struct ibv_device {}
pub struct ibv_context {
    pub ops: ibv_context_ops,
}
pub struct ibv_cq {
    pub context: *mut ibv_context,
}

#[derive(Default, Clone, Copy)]
pub struct ibv_gid {
    pub raw: [u8; 16],
}

pub struct ibv_mr {
    pub lkey: u32,
    pub rkey: u32,
}
pub struct ibv_pd {}

#[derive(Default)]
pub struct ibv_port_attr {
    pub state: ibv_port_state,
    pub active_mtu: u32,
    pub lid: u16,
}

pub struct ibv_sge {
    pub addr: u64,
    pub length: u32,
    pub lkey: u32,
}
pub struct ibv_srq {}

pub struct ibv_qp {
    pub context: *mut ibv_context,
    pub qp_num: u32,
}

#[derive(Default)]
pub struct ibv_qp_attr {
    pub qp_state: ibv_qp_state,
    pub path_mtu: u32,
    pub rq_psn: u32,
    pub sq_psn: u32,
    pub dest_qp_num: u32,
    pub qp_access_flags: ibv_access_flags,
    pub ah_attr: ibv_ah_attr,
    pub pkey_index: u16,
    pub max_rd_atomic: u8,
    pub max_dest_rd_atomic: u8,
    pub min_rnr_timer: u8,
    pub port_num: u8,
    pub timeout: u8,
    pub retry_cnt: u8,
    pub rnr_retry: u8,
}

pub struct ibv_qp_cap {
    pub max_send_wr: u32,
    pub max_recv_wr: u32,
    pub max_send_sge: u32,
    pub max_recv_sge: u32,
    pub max_inline_data: u32,
}

pub struct ibv_qp_init_attr {
    pub qp_context: *mut (),
    pub send_cq: *mut ibv_cq,
    pub recv_cq: *mut ibv_cq,
    pub srq: *mut (),
    pub cap: ibv_qp_cap,
    pub qp_type: ibv_qp_type::Type,
    pub sq_sig_all: i32,
}

bitflags! {
    pub struct ibv_qp_attr_mask: u32 {
        const IBV_QP_STATE = 1;
        const IBV_QP_ACCESS_FLAGS = 8;
        const IBV_QP_PKEY_INDEX = 16;
        const IBV_QP_PORT = 32;
        const IBV_QP_AV = 128;
        const IBV_QP_PATH_MTU = 256;
        const IBV_QP_TIMEOUT = 512;
        const IBV_QP_RETRY_CNT = 1024;
        const IBV_QP_RNR_RETRY = 2048;
        const IBV_QP_MAX_QP_RD_ATOMIC = 8192;
        const IBV_QP_RQ_PSN = 4096;
        const IBV_QP_MIN_RNR_TIMER = 32768;
        const IBV_QP_SQ_PSN = 65536;
        const IBV_QP_MAX_DEST_RD_ATOMIC = 131072;
        const IBV_QP_DEST_QPN = 1048576;
    }
}

#[derive(Default)]
pub enum ibv_qp_state {
    #[default]
    IBV_QPS_RESET,
    IBV_QPS_INIT,
    IBV_QPS_RTR,
    IBV_QPS_RTS,
}

#[derive(Default)]
pub struct ibv_global_route {
    pub dgid: ibv_gid,
    pub hop_limit: u8,
}

#[derive(Default)]
pub struct ibv_ah_attr {
    pub grh: ibv_global_route,
    pub dlid: u16,
    pub sl: u8,
    pub src_path_bits: u8,
    pub is_global: u8,
    pub port_num: u8,
}

pub struct ibv_wc {}
pub struct ibv_wc_status {}
pub enum ibv_wc_opcode {}

pub struct ibv_send_wr {
    pub wr_id: u64,
    pub next: *mut ibv_send_wr,
    pub sg_list: *mut ibv_sge,
    pub num_sge: i32,
    pub opcode: ibv_wr_opcode,
    pub send_flags: ibv_send_flags,
    pub __bindgen_anon_1: (),
    pub wr: (),
    pub qp_type: (),
    pub __bindgen_anon_2: (),
}

pub struct ibv_recv_wr {
    pub wr_id: u64,
    pub next: *mut ibv_recv_wr,
    pub sg_list: *mut ibv_sge,
    pub num_sge: i32,
}

pub enum ibv_wr_opcode {
    IBV_WR_SEND,
}

pub enum ibv_send_flags {
    IBV_SEND_SIGNALED,
}

/// Get list of IB devices currently available
/// 
/// @num_devices: optional.  if non-NULL, set to the number of devices
/// returned in the array.
/// 
/// Return a NULL-terminated array of IB devices.  The array can be
/// released with ibv_free_device_list().
pub unsafe fn ibv_get_device_list(num_devices: *mut i32) -> *mut *mut ibv_device {
    todo!()
}

/// Free list from ibv_get_device_list()
/// 
/// Free an array of devices returned from ibv_get_device_list().  Once
/// the array is freed, pointers to devices that were not opened with
/// ibv_open_device() are no longer valid.  Client code must open all
/// devices it intends to use before calling ibv_free_device_list().
pub unsafe fn ibv_free_device_list(list: *mut *mut ibv_device) {
    todo!()
}

/// Return kernel device name
pub unsafe fn ibv_get_device_name(device: *mut ibv_device) -> *const i8 {
    todo!()
}

/// Return kernel device index
/// 
/// Available for the kernel with support of IB device query
/// over netlink interface. For the unsupported kernels, the
/// relevant -1 will be returned.
pub unsafe fn ibv_get_device_index(device: *mut ibv_device) -> i32 {
    -1
}

/// Return device's node GUID
pub unsafe fn ibv_get_device_guid(device: *mut ibv_device) -> __be64 {
    todo!()
}


/// Initialize device for use
pub unsafe fn ibv_open_device(device: *mut ibv_device) -> *mut ibv_context {
    todo!()
}

/// Release device
pub unsafe fn ibv_close_device(context: *mut ibv_context) -> i32 {
    todo!()
}

/// Get port properties
pub unsafe fn ibv_query_port(
    context: *mut ibv_context, port_num: u8, port_attr: *mut ibv_port_attr,
) -> i32 {
    todo!()
}

/// Get a GID table entry
pub unsafe fn ibv_query_gid(
    context: *mut ibv_context, port_num: u8, index: i32, gid: *mut ibv_gid,
) -> i32 {
    todo!()
}

/// Allocate a protection domain
pub unsafe fn ibv_alloc_pd(context: *mut ibv_context) -> *mut ibv_pd {
    todo!()
}

/// Free a protection domain
pub unsafe fn ibv_dealloc_pd(pd: *mut ibv_pd) -> i32 {
    todo!()
}

/// Register a memory region
pub unsafe fn ibv_reg_mr(
    pd: *mut ibv_pd, addr: *mut (), length: usize, access: ibv_access_flags,
) -> *mut ibv_mr {
    todo!()
}

/// Deregister a memory region
pub unsafe fn ibv_dereg_mr(mr: *mut ibv_mr) -> i32 {
    todo!()
}

/// Create a completion queue
/// 
/// @context - Context CQ will be attached to
/// @cqe - Minimum number of entries required for CQ
/// @cq_context - Consumer-supplied context returned for completion events
/// @channel - Completion channel where completion events will be queued.
///     May be NULL if completion events will not be used.
/// @comp_vector - Completion vector used to signal completion events.
///     Must be >= 0 and < context->num_comp_vectors.
pub unsafe fn ibv_create_cq(
    context: *mut ibv_context, cqe: i32, cq_context: *mut (),
    channel: *mut (), comp_vector: i32,
) -> *mut ibv_cq {
    todo!()
}

/// Destroy a completion queue
pub unsafe fn ibv_destroy_cq(cq: *mut ibv_cq) -> i32 {
    todo!()
}

/// Create a queue pair.
pub unsafe fn ibv_create_qp(
    pd: *mut ibv_pd, qp_init_attr: *mut ibv_qp_init_attr,
) -> *mut ibv_qp {
    todo!()
}

/// Modify a queue pair.
pub unsafe fn ibv_modify_qp(
    qp: *mut ibv_qp, attr: *mut ibv_qp_attr, attr_mask: ibv_qp_attr_mask,
) -> i32 {
    todo!()
}

/// Destroy a queue pair.
pub unsafe fn ibv_destroy_qp(qp: *mut ibv_qp) -> i32 {
    todo!()
}

