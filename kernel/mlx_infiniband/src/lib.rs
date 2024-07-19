#![no_std]
#![allow(non_camel_case_types)]

use bitflags::bitflags;
use strum_macros::FromRepr;

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

#[repr(u8)]
#[derive(Default, Debug, FromRepr)]
pub enum ibv_mtu {
    Mtu256 = 1,
    Mtu512 = 2,
    Mtu1024 = 3,
    Mtu2048 = 4,
    #[default]
    Mtu4096 = 5,
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
