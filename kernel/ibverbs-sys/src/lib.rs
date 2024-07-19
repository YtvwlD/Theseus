//! This crate is a replacement for rdma-core on Linux.
//! 
//! The struct definitions are partly taken from the rust-bindgen output.
#![no_std]
#![allow(non_camel_case_types)]

extern crate alloc;

use alloc::vec::Vec;
use bitflags::bitflags;
use core2::io::{Error, ErrorKind, Result as Result};

use mlx3::{get_mlx3_nic, ConnectX3Nic};
pub use mlx3::Mtu as ibv_mtu;
use sync_irq::{IrqSafeMutex, IrqSafeMutexGuard};

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
    pub poll_cq: Option<fn(
        &ibv_cq, &mut [ibv_wc],
    ) -> Result<i32>>,
    /// This is unsafe because the sges contain raw addresses.
    pub post_send: Option<unsafe fn(
        &mut ibv_qp, &mut ibv_send_wr,
    ) -> Result<Vec<ibv_send_wr>>>,
    /// This is unsafe because the sges contain raw addresses.
    pub post_recv: Option<unsafe fn(
        &mut ibv_qp, &mut ibv_recv_wr,
    ) -> Result<Vec<ibv_recv_wr>>>,
}

const IBV_CONTEXT_OPS: ibv_context_ops = ibv_context_ops {
    poll_cq: Some(ibv_poll_cq),
    post_send: Some(ibv_post_send),
    post_recv: Some(ibv_post_recv),
};

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

pub struct ibv_device {
    nic: &'static IrqSafeMutex<ConnectX3Nic>,
}

pub struct ibv_context {
    pub ops: ibv_context_ops,
    nic: &'static IrqSafeMutex<ConnectX3Nic>,
}

impl ibv_context {
    /// Get access to the underlying device.
    fn lock(&self) -> IrqSafeMutexGuard<ConnectX3Nic> {
        self.nic.lock()
    }
}

pub struct ibv_cq {
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

pub struct ibv_qp<'ctx> {
    pub ops: &'ctx ibv_context_ops,
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

pub struct ibv_qp_init_attr<'cq> {
    pub qp_context: isize,
    pub send_cq: &'cq ibv_cq,
    pub recv_cq: &'cq ibv_cq,
    pub srq: Option<()>,
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

pub mod ibv_wc_status {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum Type {
        IBV_WC_SUCCESS, IBV_WC_GENERAL_ERR,
    }
    pub use Type::IBV_WC_SUCCESS;
    pub use Type::IBV_WC_GENERAL_ERR;
}
pub mod ibv_wc_opcode {
    #[derive(Debug, Clone, Copy)]
    pub enum Type {
        IBV_WC_SEND,
        IBV_WC_RDMA_WRITE,
        IBV_WC_RDMA_READ,
        IBV_WC_LOCAL_INV,
        IBV_WC_RECV,
    }
    pub use Type::IBV_WC_SEND;
    pub use Type::IBV_WC_RDMA_WRITE;
    pub use Type::IBV_WC_RDMA_READ;
    pub use Type::IBV_WC_LOCAL_INV;
    pub use Type::IBV_WC_RECV;
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ibv_wc_flags: u32 {
        const IBV_WC_GRH = 1;
        const IBV_WC_WITH_IMM = 2;
        const IBV_WC_IP_CSUM_OK = 4;
        const IBV_WC_WITH_INV = 8;
        const IBV_WC_TM_SYNC_REQ = 16;
        const IBV_WC_TM_MATCH = 32;
        const IBV_WC_TM_DATA_VALID = 64;
    }
}

pub struct ibv_send_wr {
    pub wr_id: u64,
    pub next: Option<()>,
    pub sg_list: Vec<ibv_sge>,
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
    pub next: Option<()>,
    pub sg_list: Vec<ibv_sge>,
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
/// Return a array of IB devices.
pub fn ibv_get_device_list() -> Result<Vec<ibv_device>> {
    let mut devices = Vec::new();
    if let Some(mlx3) = get_mlx3_nic() {
        devices.push(ibv_device { nic: &mlx3, });
    }
    Ok(devices)
}

/// Return kernel device name
pub fn ibv_get_device_name(device: &ibv_device) -> Option<*const i8> {
    todo!()
}

/// Return kernel device index
/// 
/// Available for the kernel with support of IB device query
/// over netlink interface. For the unsupported kernels, the
/// relevant error will be returned.
pub fn ibv_get_device_index(device: &ibv_device) -> Result<i32> {
    Err(Error::from(ErrorKind::InvalidData))
}

/// Return device's node GUID
pub fn ibv_get_device_guid(device: &ibv_device) -> Result<__be64> {
    todo!()
}


/// Initialize device for use
pub fn ibv_open_device(device: &ibv_device) -> Result<ibv_context> {
    Ok(ibv_context { nic: device.nic, ops: IBV_CONTEXT_OPS, })
}

/// Get port properties
pub fn ibv_query_port(
    context: &ibv_context, port_num: u8,
) -> Result<ibv_port_attr> {
    todo!()
}

/// Get a GID table entry
pub fn ibv_query_gid(
    context: &ibv_context, port_num: u8, index: i32,
) -> Result<ibv_gid> {
    todo!()
}

/// Allocate a protection domain
pub fn ibv_alloc_pd(context: &ibv_context) -> Result<ibv_pd> {
    todo!()
}

/// Register a memory region
pub fn ibv_reg_mr<T>(
    pd: &ibv_pd, data: &mut [T], access: ibv_access_flags,
) -> Result<ibv_mr> {
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
pub fn ibv_create_cq(
    context: &ibv_context, cqe: i32, cq_context: isize,
    channel: Option<()>, comp_vector: i32,
) -> Result<ibv_cq> {
    todo!()
}

/// Create a queue pair.
pub fn ibv_create_qp<'ctx>(
    pd: &'ctx ibv_pd, qp_init_attr: &ibv_qp_init_attr<'ctx>,
) -> Result<ibv_qp<'ctx>> {
    todo!()
}

/// Modify a queue pair.
pub fn ibv_modify_qp(
    qp: &mut ibv_qp, attr: &ibv_qp_attr, attr_mask: ibv_qp_attr_mask,
) -> Result<()> {
    todo!()
}

/// poll a completion queue (CQ)
fn ibv_poll_cq(
    cq: &ibv_cq, wc: &mut [ibv_wc],
) -> Result<i32> {
    todo!()
}

/// post a list of work requests (WRs) to a send queue
unsafe fn ibv_post_send(
    qp: &mut ibv_qp, wr: &mut ibv_send_wr,
) -> Result<Vec<ibv_send_wr>> {
    todo!()
}

/// post a list of work requests (WRs) to a receive queue
unsafe fn ibv_post_recv(
    qp: &mut ibv_qp, wr: &mut ibv_recv_wr,
) -> Result<Vec<ibv_recv_wr>> {
    todo!()
}

// // // // // // // // // // // // // // // // // // // // // // // // // // // //
// This struct and implementation is taken from the upstream ibverbs-sys crate.  //
// // // // // // // // // // // // // // // // // // // // // // // // // // // //

/// An ibverb work completion.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ibv_wc {
    wr_id: u64,
    status: ibv_wc_status::Type,
    opcode: ibv_wc_opcode::Type,
    vendor_err: u32,
    byte_len: u32,

    /// Immediate data OR the local RKey that was invalidated depending on `wc_flags`.
    /// See `man ibv_poll_cq` for details.
    pub imm_data: u32,
    /// Local QP number of completed WR.
    ///
    /// Relevant for Receive Work Completions that are associated with an SRQ.
    pub qp_num: u32,
    /// Source QP number (remote QP number) of completed WR.
    ///
    /// Relevant for Receive Work Completions of a UD QP.
    pub src_qp: u32,
    /// Flags of the Work Completion. It is either 0 or the bitwise OR of one or more of the
    /// following flags:
    ///
    ///  - `IBV_WC_GRH`: Indicator that GRH is present for a Receive Work Completions of a UD QP.
    ///    If this bit is set, the first 40 bytes of the buffered that were referred to in the
    ///    Receive request will contain the GRH of the incoming message. If this bit is cleared,
    ///    the content of those first 40 bytes is undefined
    ///  - `IBV_WC_WITH_IMM`: Indicator that imm_data is valid. Relevant for Receive Work
    ///    Completions
    pub wc_flags: ibv_wc_flags,
    /// P_Key index (valid only for GSI QPs).
    pub pkey_index: u16,
    /// Source LID (the base LID that this message was sent from).
    ///
    /// Relevant for Receive Work Completions of a UD QP.
    pub slid: u16,
    /// Service Level (the SL LID that this message was sent with).
    ///
    /// Relevant for Receive Work Completions of a UD QP.
    pub sl: u8,
    /// Destination LID path bits.
    ///
    /// Relevant for Receive Work Completions of a UD QP (not applicable for multicast messages).
    pub dlid_path_bits: u8,
}

#[allow(clippy::len_without_is_empty)]
impl ibv_wc {
    /// Returns the 64 bit value that was associated with the corresponding Work Request.
    pub fn wr_id(&self) -> u64 {
        self.wr_id
    }

    /// Returns the number of bytes transferred.
    ///
    /// Relevant if the Receive Queue for incoming Send or RDMA Write with immediate operations.
    /// This value doesn't include the length of the immediate data, if such exists. Relevant in
    /// the Send Queue for RDMA Read and Atomic operations.
    ///
    /// For the Receive Queue of a UD QP that is not associated with an SRQ or for an SRQ that is
    /// associated with a UD QP this value equals to the payload of the message plus the 40 bytes
    /// reserved for the GRH. The number of bytes transferred is the payload of the message plus
    /// the 40 bytes reserved for the GRH, whether or not the GRH is present
    pub fn len(&self) -> usize {
        self.byte_len as usize
    }

    /// Check if this work requested completed successfully.
    ///
    /// A successful work completion (`IBV_WC_SUCCESS`) means that the corresponding Work Request
    /// (and all of the unsignaled Work Requests that were posted previous to it) ended, and the
    /// memory buffers that this Work Request refers to are ready to be (re)used.
    pub fn is_valid(&self) -> bool {
        self.status == ibv_wc_status::IBV_WC_SUCCESS
    }

    /// Returns the work completion status and vendor error syndrome (`vendor_err`) if the work
    /// request did not completed successfully.
    ///
    /// Possible statuses include:
    ///
    ///  - `IBV_WC_LOC_LEN_ERR`: Local Length Error: this happens if a Work Request that was posted
    ///    in a local Send Queue contains a message that is greater than the maximum message size
    ///    that is supported by the RDMA device port that should send the message or an Atomic
    ///    operation which its size is different than 8 bytes was sent. This also may happen if a
    ///    Work Request that was posted in a local Receive Queue isn't big enough for holding the
    ///    incoming message or if the incoming message size if greater the maximum message size
    ///    supported by the RDMA device port that received the message.
    ///  - `IBV_WC_LOC_QP_OP_ERR`: Local QP Operation Error: an internal QP consistency error was
    ///    detected while processing this Work Request: this happens if a Work Request that was
    ///    posted in a local Send Queue of a UD QP contains an Address Handle that is associated
    ///    with a Protection Domain to a QP which is associated with a different Protection Domain
    ///    or an opcode which isn't supported by the transport type of the QP isn't supported (for
    ///    example:
    ///    RDMA Write over a UD QP).
    ///  - `IBV_WC_LOC_EEC_OP_ERR`: Local EE Context Operation Error: an internal EE Context
    ///    consistency error was detected while processing this Work Request (unused, since its
    ///    relevant only to RD QPs or EE Context, which aren’t supported).
    ///  - `IBV_WC_LOC_PROT_ERR`: Local Protection Error: the locally posted Work Request’s buffers
    ///    in the scatter/gather list does not reference a Memory Region that is valid for the
    ///    requested operation.
    ///  - `IBV_WC_WR_FLUSH_ERR`: Work Request Flushed Error: A Work Request was in process or
    ///    outstanding when the QP transitioned into the Error State.
    ///  - `IBV_WC_MW_BIND_ERR`: Memory Window Binding Error: A failure happened when tried to bind
    ///    a MW to a MR.
    ///  - `IBV_WC_BAD_RESP_ERR`: Bad Response Error: an unexpected transport layer opcode was
    ///    returned by the responder. Relevant for RC QPs.
    ///  - `IBV_WC_LOC_ACCESS_ERR`: Local Access Error: a protection error occurred on a local data
    ///    buffer during the processing of a RDMA Write with Immediate operation sent from the
    ///    remote node. Relevant for RC QPs.
    ///  - `IBV_WC_REM_INV_REQ_ERR`: Remote Invalid Request Error: The responder detected an
    ///    invalid message on the channel. Possible causes include the operation is not supported
    ///    by this receive queue (qp_access_flags in remote QP wasn't configured to support this
    ///    operation), insufficient buffering to receive a new RDMA or Atomic Operation request, or
    ///    the length specified in a RDMA request is greater than 2^{31} bytes. Relevant for RC
    ///    QPs.
    ///  - `IBV_WC_REM_ACCESS_ERR`: Remote Access Error: a protection error occurred on a remote
    ///    data buffer to be read by an RDMA Read, written by an RDMA Write or accessed by an
    ///    atomic operation. This error is reported only on RDMA operations or atomic operations.
    ///    Relevant for RC QPs.
    ///  - `IBV_WC_REM_OP_ERR`: Remote Operation Error: the operation could not be completed
    ///    successfully by the responder. Possible causes include a responder QP related error that
    ///    prevented the responder from completing the request or a malformed WQE on the Receive
    ///    Queue. Relevant for RC QPs.
    ///  - `IBV_WC_RETRY_EXC_ERR`: Transport Retry Counter Exceeded: The local transport timeout
    ///    retry counter was exceeded while trying to send this message. This means that the remote
    ///    side didn't send any Ack or Nack. If this happens when sending the first message,
    ///    usually this mean that the connection attributes are wrong or the remote side isn't in a
    ///    state that it can respond to messages. If this happens after sending the first message,
    ///    usually it means that the remote QP isn't available anymore. Relevant for RC QPs.
    ///  - `IBV_WC_RNR_RETRY_EXC_ERR`: RNR Retry Counter Exceeded: The RNR NAK retry count was
    ///    exceeded. This usually means that the remote side didn't post any WR to its Receive
    ///    Queue. Relevant for RC QPs.
    ///  - `IBV_WC_LOC_RDD_VIOL_ERR`: Local RDD Violation Error: The RDD associated with the QP
    ///    does not match the RDD associated with the EE Context (unused, since its relevant only
    ///    to RD QPs or EE Context, which aren't supported).
    ///  - `IBV_WC_REM_INV_RD_REQ_ERR`: Remote Invalid RD Request: The responder detected an
    ///    invalid incoming RD message. Causes include a Q_Key or RDD violation (unused, since its
    ///    relevant only to RD QPs or EE Context, which aren't supported)
    ///  - `IBV_WC_REM_ABORT_ERR`: Remote Aborted Error: For UD or UC QPs associated with a SRQ,
    ///    the responder aborted the operation.
    ///  - `IBV_WC_INV_EECN_ERR`: Invalid EE Context Number: An invalid EE Context number was
    ///    detected (unused, since its relevant only to RD QPs or EE Context, which aren't
    ///    supported).
    ///  - `IBV_WC_INV_EEC_STATE_ERR`: Invalid EE Context State Error: Operation is not legal for
    ///    the specified EE Context state (unused, since its relevant only to RD QPs or EE Context,
    ///    which aren't supported).
    ///  - `IBV_WC_FATAL_ERR`: Fatal Error.
    ///  - `IBV_WC_RESP_TIMEOUT_ERR`: Response Timeout Error.
    ///  - `IBV_WC_GENERAL_ERR`: General Error: other error which isn't one of the above errors.
    pub fn error(&self) -> Option<(ibv_wc_status::Type, u32)> {
        match self.status {
            ibv_wc_status::IBV_WC_SUCCESS => None,
            status => Some((status, self.vendor_err)),
        }
    }

    /// Returns the operation that the corresponding Work Request performed.
    ///
    /// This value controls the way that data was sent, the direction of the data flow and the
    /// valid attributes in the Work Completion.
    pub fn opcode(&self) -> ibv_wc_opcode::Type {
        self.opcode
    }

    /// Returns a 32 bits number, in network order, in an SEND or RDMA WRITE opcodes that is being
    /// sent along with the payload to the remote side and placed in a Receive Work Completion and
    /// not in a remote memory buffer
    ///
    /// Note that IMM is only returned if `IBV_WC_WITH_IMM` is set in `wc_flags`. If this is not
    /// the case, no immediate value was provided, and `imm_data` should be interpreted
    /// differently. See `man ibv_poll_cq` for details.
    pub fn imm_data(&self) -> Option<u32> {
        if self.is_valid() && self.wc_flags.contains(ibv_wc_flags::IBV_WC_WITH_IMM) {
            Some(self.imm_data)
        } else {
            None
        }
    }
}

impl Default for ibv_wc {
    fn default() -> Self {
        ibv_wc {
            wr_id: 0,
            status: ibv_wc_status::IBV_WC_GENERAL_ERR,
            opcode: ibv_wc_opcode::IBV_WC_LOCAL_INV,
            vendor_err: 0,
            byte_len: 0,
            imm_data: 0,
            qp_num: 0,
            src_qp: 0,
            wc_flags: ibv_wc_flags::empty(),
            pkey_index: 0,
            slid: 0,
            sl: 0,
            dlid_path_bits: 0,
        }
    }
}

// // // // // // //
// End of copy.   //
// // // // // // //
