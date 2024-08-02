//! This module contains some structs for InfiniBand.

#![no_std]
#![allow(non_camel_case_types)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use bitflags::bitflags;
use strum_macros::FromRepr;

pub mod ibv_qp_type {
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub enum Type {
        IBV_QPT_RC, IBV_QPT_UC, IBV_QPT_UD,
    }
    pub use Type::IBV_QPT_RC;
    pub use Type::IBV_QPT_UC;
    pub use Type::IBV_QPT_UD;
}

pub struct ibv_qp_cap {
    pub max_send_wr: u32,
    pub max_recv_wr: u32,
    pub max_send_sge: u32,
    pub max_recv_sge: u32,
    pub max_inline_data: u32,
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

pub struct ibv_device_attr {
    pub fw_ver: String,
    pub phys_port_cnt: u8,
}

#[repr(u8)]
#[derive(Default, Debug, Clone, Copy, FromRepr)]
pub enum ibv_mtu {
    Mtu256 = 1,
    Mtu512 = 2,
    Mtu1024 = 3,
    Mtu2048 = 4,
    #[default]
    Mtu4096 = 5,
}

#[derive(Debug, Default, PartialEq, Eq, FromRepr)]
#[repr(i32)]
pub enum ibv_port_state {
    #[default]
    IBV_PORT_NOP = 0,
    IBV_PORT_DOWN = 1,
    IBV_PORT_INIT = 2,
    IBV_PORT_ARMED = 3,
    IBV_PORT_ACTIVE = 4,
    IBV_PORT_ACTIVE_DEFER = 5,
}

#[derive(Debug, Default, FromRepr)]
#[repr(u8)]
pub enum PhysicalPortState {
    #[default]
    Nop = 0,
    Sleep = 1,
    Polling = 2,
    Disabled = 3,
    PortConfigurationTraining = 4,
    LinkUp = 5,
    LinkErrorRecovery = 6,
    PhyTest = 7,
}

#[derive(Default, Clone, Copy)]
pub struct ibv_gid {
    pub raw: [u8; 16],
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

#[derive(Default)]
pub struct ibv_qp_attr {
    pub qp_state: ibv_qp_state,
    pub path_mtu: ibv_mtu,
    pub qkey: u32,
    pub rq_psn: u32,
    pub sq_psn: u32,
    pub dest_qp_num: u32,
    pub qp_access_flags: ibv_access_flags,
    pub ah_attr: ibv_ah_attr,
    pub alt_ah_attr: ibv_ah_attr,
    pub pkey_index: u16,
    pub alt_pkey_index: u16,
    pub max_rd_atomic: u8,
    pub max_dest_rd_atomic: u8,
    pub min_rnr_timer: u8,
    pub port_num: u8,
    pub timeout: u8,
    pub retry_cnt: u8,
    pub rnr_retry: u8,
    pub alt_port_num: u8,
    pub alt_timeout: u8,
}


bitflags! {
    pub struct ibv_qp_attr_mask: u32 {
        const IBV_QP_STATE = 1;
        const IBV_QP_ACCESS_FLAGS = 8;
        const IBV_QP_PKEY_INDEX = 16;
        const IBV_QP_PORT = 32;
        const IBV_QP_QKEY = 64;
        const IBV_QP_AV = 128;
        const IBV_QP_PATH_MTU = 256;
        const IBV_QP_TIMEOUT = 512;
        const IBV_QP_RETRY_CNT = 1024;
        const IBV_QP_RNR_RETRY = 2048;
        const IBV_QP_MAX_QP_RD_ATOMIC = 8192;
        const IBV_QP_RQ_PSN = 4096;
        const IBV_QP_ALT_PATH = 16384;
        const IBV_QP_MIN_RNR_TIMER = 32768;
        const IBV_QP_SQ_PSN = 65536;
        const IBV_QP_MAX_DEST_RD_ATOMIC = 131072;
        const IBV_QP_DEST_QPN = 1048576;
    }
}

#[derive(Default)]
pub struct ibv_port_attr {
    pub state: ibv_port_state,
    pub max_mtu: ibv_mtu,
    pub active_mtu: ibv_mtu,
    pub port_cap_flags: u32,
    pub lid: u16,
    pub sm_lid: u16,
    pub lmc: u8,
    pub link_layer: u8,
    pub phys_state: PhysicalPortState,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum ibv_qp_state {
    #[default]
    IBV_QPS_RESET,
    IBV_QPS_INIT,
    IBV_QPS_RTR,
    IBV_QPS_RTS,
    IBV_QPS_SQD,
}

pub struct ibv_send_wr {
    pub wr_id: u64,
    pub next: Option<()>,
    pub sg_list: Vec<ibv_sge>,
    pub num_sge: i32,
    pub opcode: ibv_wr_opcode,
    pub send_flags: ibv_send_flags,
    pub __bindgen_anon_1: (),
    pub wr: ibv_send_wr_wr,
    pub qp_type: (),
    pub __bindgen_anon_2: (),
}

pub enum ibv_send_wr_wr {
    rdma {
        /// Start address of remote memory buffer
        remote_addr: u64,
        /// Key of the remote Memory Region
        rkey: u32,
    },
    atomic {
        /// Start address of remote memory buffer
        remote_addr: u64,
        /// Compare operand
        compare_add: u64,
        /// Swap operand
        swap: u64,
        /// Key of the remote Memory Region
        rkey: u32,
    },
    ud {
        /// Address handle for the remote node address
        ah: ibv_send_wr_wr_ah,
        remote_qpn: u32,
        remote_qkey: u32,
    },
}

impl Default for ibv_send_wr_wr {
    fn default() -> Self {
        Self::rdma { remote_addr: 0, rkey: 0, }
    }
}


pub struct ibv_send_wr_wr_ah {
    pub port: u32,
    pub dlid: u16,
    pub slid: u8,
}

pub struct ibv_recv_wr {
    pub wr_id: u64,
    pub next: Option<()>,
    pub sg_list: Vec<ibv_sge>,
    pub num_sge: i32,
}

#[derive(PartialEq)]
pub enum ibv_wr_opcode {
    IBV_WR_RDMA_WRITE,
    IBV_WR_SEND,
    IBV_WR_RDMA_READ,
}

pub enum ibv_send_flags {
    IBV_SEND_SIGNALED,
}

pub struct ibv_sge {
    pub addr: u64,
    pub length: u32,
    pub lkey: u32,
}
