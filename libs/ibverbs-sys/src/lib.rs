//! This crate is a replacement for rdma-core on Linux.
#![no_std]
#![allow(non_camel_case_types)]

pub mod ibv_qp_type {
    pub enum Type {
        IBV_QPT_RC, IBV_QPT_UC, IBV_QPT_UD,
    }
    pub use Type::IBV_QPT_RC;
    pub use Type::IBV_QPT_UC;
    pub use Type::IBV_QPT_UD;
}

type __be64 = u64; // TODO

pub struct ibv_access_flags {}

pub struct ibv_port_state {}

pub struct ibv_device {}
pub struct ibv_context {}
pub struct ibv_cq {}
pub struct ibv_gid {}
pub struct ibv_mr {}
pub struct ibv_pd {}
pub struct ibv_port_attr {}
pub struct ibv_sge {}
pub struct ibv_srq {}

pub struct ibv_qp {}
pub struct ibv_qp_attr {}
pub struct ibv_qp_attr_mask {}
pub struct ibv_qp_cap {}
pub struct ibv_qp_init_attr {}
pub enum ibv_qp_state {}

pub struct ibv_ah_attr {}

pub struct ibv_wc {}
pub struct ibv_wc_status {}
pub enum ibv_wc_opcode {}

pub struct ibv_send_wr {}
pub struct ibv_recv_wr {}
pub enum ibv_wr_opcode {}

pub enum ibv_send_flags {}


pub fn ibv_get_device_list() {
    todo!()
}

pub fn ibv_free_device_list() {
    todo!()
}

pub fn ibv_open_device() {
    todo!()
}

pub fn ibv_get_device_guid() {
    todo!()
}

pub fn ibv_get_device_index() {
    todo!()
}

pub fn ibv_get_device_name() {
    todo!()
}

pub fn ibv_close_device() {
    todo!()
}

pub fn ibv_query_port() {
    todo!()
}

pub fn ibv_query_gid() {
    todo!()
}

pub fn ibv_create_cq() {
    todo!()
}

pub fn ibv_destroy_cq() {
    todo!()
}

pub fn ibv_alloc_pd() {
    todo!()
}

pub fn ibv_dealloc_pd() {
    todo!()
}

pub fn ibv_reg_mr() {
    todo!()
}

pub fn ibv_dereg_mr() {
    todo!()
}

pub fn ibv_create_qp() {
    todo!()
}

pub fn ibv_modify_qp() {
    todo!()
}

pub fn ibv_destroy_qp() {
    todo!()
}

