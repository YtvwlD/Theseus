//! This module contains functionality to interact with the firmware.

use core::mem::size_of;

use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use zerocopy::{FromBytes, U16, U32, U64};

use crate::cmd::{CommandMailBox, Opcode};

#[derive(Clone, FromBytes)]
#[repr(C, packed)]
pub(super) struct Firmware {
    pages: U16<BigEndian>,
    major: U16<BigEndian>,
    sub_minor: U16<BigEndian>,
    minor: U16<BigEndian>,
    _padding1: u16,
    ix_rev: U16<BigEndian>,
    _padding2: [u8; 22], // contains the build timestamp
    clr_int_base: U64<BigEndian>,
    clr_int_bar: u8,
    // many fields follow
}

impl Firmware {
    pub(super) fn query(config_regs: &mut MappedPages) -> Result<Self, &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        // TODO: this should not be in high memory (?)
        let (pages, physical) = create_contiguous_mapping(PAGE_SIZE, DMA_FLAGS)?;
        trace!("asking the card to put information about its firmware into {pages:?}@{physical}...");
        cmd.execute_command(Opcode::QueryFw, None, 0, Some(physical))?;
        let mut fw = pages.as_type::<Firmware>(0)?.clone();
        fw.clr_int_bar = (fw.clr_int_bar >> 6) * 2;
        trace!("got firmware info: {fw:?}");
        Ok(fw)
    }
    
    pub(super) fn map_area(self, config_regs: &mut MappedPages) -> Result<MappedFirmwareArea, &'static str> {
        const MAX_CHUNK_LOG2: u32 = 18;
        trace!("mapping firmware area...");

        let mut cmd = CommandMailBox::new(config_regs)?;
        let size = PAGE_SIZE * usize::from(self.pages);
        let (pages, physical) = create_contiguous_mapping(size, DMA_FLAGS)?;
        let mut align = physical.value().trailing_zeros();
        if align > MAX_CHUNK_LOG2 {
            trace!("alignment greater than max chunk size, defaulting to 256KB");
            align = MAX_CHUNK_LOG2;
        }

        let mut count = size / (1 << align);
        if size % (1 << align) != 0 {
            count += 1;
        }
        // TODO: we can batch as many vpm entries as fit in a mailbox (1 page)
        // rather than 1 chunk per mailbox, this will make bootup faster
        let (mut vpm_pages, vpm_physical) = create_contiguous_mapping(size_of::<VirtualPhysicalMapping>(), DMA_FLAGS)?;
        let vpm: &mut VirtualPhysicalMapping = vpm_pages.as_type_mut(0)?;
        let mut pointer = physical;
        for _ in 0..count {
            vpm.physical_address_high.set((pointer.value() >> 32).try_into().unwrap());
            vpm.physical_address_low.set((pointer.value() & 0xffffffff).try_into().unwrap());
            cmd.execute_command(Opcode::MapFa, Some(vpm_physical), 1, None)?;
            pointer += 1 << align;
        }
        trace!("mapped {} pages for firmware area", self.pages);

        Ok(MappedFirmwareArea { pages, physical, })
    }
}

impl core::fmt::Debug for Firmware {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("Firmware")
            .field("clr_int_bar", &self.clr_int_bar)
            .field("clr_int_base", &format_args!("{:#x}", self.clr_int_base))
            .field("version", &format_args!("{}.{}.{}", self.major, self.minor, self.sub_minor))
            .field("ix_rev", &self.ix_rev.get())
            .field("size", &format_args!(
                "{}.{} KB",
                (self.pages.get() as usize * PAGE_SIZE) / 1024,
                (self.pages.get() as usize * PAGE_SIZE) % 1024,
            ))
            .finish()
    }
}


#[derive(Clone, FromBytes, Default)]
#[repr(C, packed)]
struct VirtualPhysicalMapping {
    virtual_address_high: U32<BigEndian>,
    // actually just 20 bits
    virtual_address_low: U32<BigEndian>,
    physical_address_high: U32<BigEndian>,
    // actually just 20 bits and then log2size
    physical_address_low: U32<BigEndian>,
}

/// A mapped firmware area.
/// 
/// Instead of dropping, please unmap the area from the card.
pub(super) struct MappedFirmwareArea {
    pages: MappedPages,
    physical: PhysicalAddress,
}

impl MappedFirmwareArea {
    pub(super) fn run(&self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        cmd.execute_command(Opcode::RunFw, None, 0, None)?;
        trace!("successfully run firmware");
        Ok(())
    }

     pub(super) fn query_capabilities(&self, config_regs: &mut MappedPages) -> Result<Capabilities, &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        let (pages, physical) = create_contiguous_mapping(size_of::<Capabilities>(), DMA_FLAGS)?;
        cmd.execute_command(Opcode::QueryDevCap, None, 0, Some(physical))?;
        let mut caps = pages.as_type::<Capabilities>(0)?.clone();
        // truncate fields to their actual size
        caps.log2_rsvd_qps &= 0xf;
        caps.log_max_qp &= 0x1f;
        caps.log2_rsvd_srqs >>= 4;
        caps.log_max_srqs &= 0x1f;
        caps.num_rsvd_eec &= 0x3f;
        caps.log_max_eec &= 0xf;
        caps.log2_rsvd_cqs &= 0xf;
        caps.log_max_cq &= 0x1f;
        caps.log_max_d_mpts &= 0x3f;
        caps.log2_rsvd_eqs &= 0xf;
        caps.log_max_eq &= 0xf;
        caps.num_rsvd_eqs &= 0xf;
        caps.log2_rsvd_mtts >>= 4;
        caps.log_max_mrw_sz &= 0x7f;
        caps.log2_rsvd_mrws &= 0xf;
        caps.log_max_mtts &= 0x3f;
        // TODO: caps.num_sys_eq
        caps.log_max_ra_req_qp &= 0x3f;
        caps.log_max_ra_res_qp &= 0x3f;
        caps.log2_max_gso_sz &= 0x1f;
        caps.rdma &= 0x3f;
        caps.rsz_srq &= 0x1;
        caps.port_beacon >>= 7;
        caps.num_ports &= 0xf;
        caps.log_max_msg &= 0x1f;
        caps.cq_timestamp >>= 7;
        caps.num_rsvd_uars >>= 4;
        caps.uar_sz &= 0x3f;
        caps.bf >>= 7;
        caps.log_bf_reg_sz &= 0x1f;
        caps.log_max_bf_regs_per_page &= 0x3f;
        caps.log_max_bf_pages &= 0x3f;
        caps.num_rsvd_pds >>= 4;
        caps.log_max_pd &= 0x1f;
        caps.num_rsvd_xrcds >>= 4;
        caps.log_max_xrcd &= 0x1f;
        caps.ecn_qcn_ver &= 1;
        caps.mad_demux &= 1;
        // each UAR has 4 EQ doorbells; so if a UAR is reserved,
        // then we can't use any EQs whose doorbell falls on that page,
        // even if the EQ itself isn't reserved
        if caps.num_rsvd_uars * 4 > caps.num_rsvd_eqs {
            caps.num_rsvd_eqs = caps.num_rsvd_uars * 4;
        }
        // TODO: caps.reserved_qpt_cnt[MLX3_QP_REGION_FW] = 1 << caps.log2_rsvd_qps
        // no merge of flags and ext_flags here
        
        trace!("max BF pages: {}", 1 << caps.log_max_bf_pages);
        // TODO: caps.reserved_qpt_cnt[MLX3_QP_REGION_ETH_ADDR] = (1 << caps.log_num_macs) * (1 << caps.log_num_vlans) * caps.num_ports
        trace!("got caps: {:?}", caps);
        Ok(caps)
    }
    
    /// Unmaps the area from the card. Further usage requires a software reset.
    pub(super) fn unmap(self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        trace!("unmapping firmware area...");
        let mut cmd = CommandMailBox::new(config_regs)?;
        cmd.execute_command(Opcode::UnmapFa, None, 0, None)?;
        Ok(())
    }
}

impl Drop for MappedFirmwareArea {
    fn drop(&mut self) {
        panic!("please unmap instead of dropping")
    }
}

#[derive(Clone, FromBytes)]
#[repr(C, packed)]
pub(super) struct Capabilities {
    _padding1: u128,
    log_max_srq_sz: u8,
    log_max_qp_sz: u8,
    // actually just u4
    log2_rsvd_qps: u8,
    // actually just u5
    log_max_qp: u8,
    // actually just the upper u4
    log2_rsvd_srqs: u8,
    // actually just u5
    log_max_srqs: u8,
    // actually just u6
    num_rsvd_eec: u8,
    // actually just u4
    log_max_eec: u8,
    // deprecated
    num_rsvd_eqs: u8,
    log_max_cq_sz: u8,
    // actually just u4
    log2_rsvd_cqs: u8,
    // actually just u5
    log_max_cq: u8,
    log_max_eq_sz: u8,
    // actually just u6
    log_max_d_mpts: u8,
    // actually just u4, deprecated
    log2_rsvd_eqs: u8,
    // actually just u4
    log_max_eq: u8,
    // actually just the upper u4
    log2_rsvd_mtts: u8,
    // actually just u7
    log_max_mrw_sz: u8,
    // actually just u4
    log2_rsvd_mrws: u8,
    // actually just u6
    log_max_mtts: u8,
    _padding2: u16,
    // actually just u12, not present in mlx3
    _num_sys_eq: u16,
    // max_av?
    _padding3: u8,
    // actually just u6
    log_max_ra_req_qp: u8,
    _padding4: u8,
    // actually just u6
    log_max_ra_res_qp: u8,
    _padding5: u8,
    // actually just u5
    log2_max_gso_sz: u8,
    rss: u8,
    // actually just u6
    rdma: u8,
    _padding6: u16,
    _padding7: u8,
    // actually just u1
    rsz_srq: u8,
    // actually just the upper u1
    port_beacon: u8,
    ack_delay: u8,
    // pci_pf_num
    mtu_with: u8,
    // actually just u4
    num_ports: u8,
    // actually just u5
    log_max_msg: u8,
    _padding8: u16,
    // max_funix
    max_gid: u8,
    rate_support: U16<BigEndian>,
    // actually just the upper u1
    cq_timestamp: u8,
    _padding9: u8,
    // max_pkey?
    ext_flags: U32<BigEndian>,
    cap_flags: U32<BigEndian>,
    // actually just the upper u4
    num_rsvd_uars: u8,
    // actually just u6
    uar_sz: u8,
    _padding10: u8,
    log_page_sz: u8,
    // actually just the upper u1
    bf: u8,
    // actually just u5
    log_bf_reg_sz: u8,
    // actually just u6
    log_max_bf_regs_per_page: u8,
    // actually just u6
    log_max_bf_pages: u8,
    _padding11: u8,
    max_sg_sq: u8,
    max_desc_sz_sq: U16<BigEndian>,
    _padding12: u8,
    max_sg_rq: u8,
    max_desc_sz_rq: U16<BigEndian>,
    // user_mac_en?
    // svlan_by_qp?
    _padding13: u64,
    _padding14: u8,
    log_max_qp_mcg: u8,
    num_rsvd_mcgs: u8,
    log_max_mcg: u8,
    // actually just the upper u4
    num_rsvd_pds: u8,
    // actually just u5
    log_max_pd: u8,
    // actually just the upper u4
    num_rsvd_xrcds: u8,
    // actually just u5
    log_max_xrcd: u8,
    max_if_cnt_basic: U32<BigEndian>,
    max_if_cnt_extended: U32<BigEndian>,
    ext2_flags: U16<BigEndian>,
    _padding15: u16,
    flow_steering_flags: U16<BigEndian>,
    flow_steering_range: u8,
    flow_steering_max_qp_per_entry: u8,
    sl2vl_event: u8,
    _padding16: u8,
    cq_eq_cache_line_stride: u8,
    // actually just u1
    ecn_qcn_ver: u8,
    _padding17: u32,
    pub(super) rdmarc_entry_sz: U16<BigEndian>,
    pub(super) qpc_entry_sz: U16<BigEndian>,
    pub(super) aux_entry_sz: U16<BigEndian>,
    pub(super) altc_entry_sz: U16<BigEndian>,
    pub(super) eqc_entry_sz: U16<BigEndian>,
    pub(super) cqc_entry_sz: U16<BigEndian>,
    pub(super) srq_entry_sz: U16<BigEndian>,
    pub(super) c_mpt_entry_sz: U16<BigEndian>,
    pub(super) mtt_entry_sz: U16<BigEndian>,
    pub(super) d_mpt_entry_sz: U16<BigEndian>,
    bmme_flags: U16<BigEndian>,
    phv_en: U16<BigEndian>,
    rsvd_lkey: U32<BigEndian>,
    diag_flags: U32<BigEndian>,
    pub(super) max_icm_sz: U64<BigEndian>,
    // actually just u24
    dmfs_high_rate_qpn_base: U32<BigEndian>,
    // actually just u24
    dmfs_high_rate_qpn_range: U32<BigEndian>,
    _padding18: u16,
    _padding19: u8,
    // actually just u1
    mad_demux: u8,
    _padding20: u32,
    _padding21: u128,
    _padding22: u128,
    // actually just u12
    qp_rate_limit_max: U16<BigEndian>,
    // actually just u12
    qp_rate_limit_min: U16<BigEndian>,
    // reserved space follows
}

impl Capabilities {
    fn bf_regs_per_page(&self) -> usize {
        if self.bf == 1 {
            if 1 << self.log_max_bf_regs_per_page > PAGE_SIZE / self.bf_reg_size() {
                3
            } else {
                1 << self.log_max_bf_regs_per_page
            }
        } else {
            0
        }
    }

    fn bf_reg_size(&self) -> usize {
        if self.bf == 1 {
            1 << self.log_bf_reg_sz
        } else {
            0
        }
    }

    fn num_uars(&self) -> usize {
        usize::try_from(self.uar_size()).unwrap() / PAGE_SIZE
    }

    fn uar_size(&self) -> u64 {
        1 << (self.uar_sz + 20)
    }
}

impl core::fmt::Debug for Capabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("Capabilities")
            .field("BlueFlame available", &self.bf)
            .field("BlueFlame reg size", &self.bf_reg_size())
            .field("BlueFlame regs/page", &self.bf_regs_per_page())
            .field("Max ICM size (PB)", &(self.max_icm_sz.get() >> 50))
            .field("Max QPs", &(1 << self.log_max_qp))
            .field("reserved QPs", &(1 << self.log2_rsvd_qps))
            .field("QPC entry size", &self.qpc_entry_sz)
            .field("Max SRQs", &(1 << self.log_max_srqs))
            .field("reserved SRQs", &(1 << self.log2_rsvd_srqs))
            .field("SRQ entry size", &self.srq_entry_sz)
            .field("Max CQs", &(1 << self.log_max_cq))
            .field("reserved CQs", &(1 << self.log2_rsvd_cqs))
            .field("CQC entry size", &self.cqc_entry_sz)
            .field("Max EQs", &(1 << self.log_max_eq))
            .field("reserved EQs", &(1 << self.log2_rsvd_eqs))
            .field("EQC entry size", &self.eqc_entry_sz)
            .field("reserved MPTs", &(1 << self.log2_rsvd_mrws))
            .field("reserved MTTs", &(1 << self.log2_rsvd_mtts))
            .field("Max CQE count", &(1 << self.log_max_cq_sz))
            .field("max QPE count", &(1 << self.log_max_qp_sz))
            .field("max SRQe count", &(1 << self.log_max_eq_sz))
            .field("MTT Entry Size", &self.mtt_entry_sz)
            .field("Reserved MTTs", &(1 << self.log2_rsvd_mtts))
            .field("cMPT Entry Size", &self.c_mpt_entry_sz)
            .field("dMPT Entry Size", &self.d_mpt_entry_sz)
            .field("Reserved UAR", &self.num_rsvd_uars)
            .field("UAR Size", &self.uar_size())
            .field("Num UAR", &self.num_uars())
            .field("Network Port count", &self.num_ports)
            .field("Min Page Size", &(1 << self.log_page_sz))
            .field("Max SQ desc size WQE Entry Size", &self.max_desc_sz_sq)
            .field("max SQ S/G WQE Entries", &self.max_sg_sq)
            .field("Max RQ desc size", &self.max_desc_sz_rq)
            .field("max RQ S/G", &self.max_sg_rq)
            .field("Max Message Size", &(1 << self.log_max_msg))
            // TODO: dump flags
            .finish()
    }
}

// TODO: this is just a placeholder for now
#[derive(Default)]
pub(super) struct InitHcaParameters {
    pub(super) qpc_base: u64,
    pub(super) rdmarc_base: u64,
    pub(super) auxc_base: u64,
    pub(super) altc_base: u64,
    pub(super) srqc_base: u64,
    pub(super) cqc_base: u64,
    pub(super) eqc_base: u64,
    pub(super) mc_base: u64,
    pub(super) dmpt_base: u64,
    pub(super) cmpt_base: u64,
    pub(super) mtt_base: u64,
    pub(super) num_cqs: usize,
    pub(super) num_qps: usize,
    pub(super) num_eqs: usize,
    pub(super) num_mpts: usize,
    pub(super) num_mgms: usize,
    pub(super) num_amgms: usize,
    pub(super) num_srqs: usize,
    pub(super) num_mtts: usize,
    pub(super) max_qp_dest_rdma: usize,
    pub(super) log_mc_entry_sz: u16,
    pub(super) log_mc_hash_sz: u16,
    pub(super) log_num_qps: u8,
    pub(super) log_num_srqs: u8,
    pub(super) log_num_cqs: u8,
    pub(super) log_num_eqs: u8,
    pub(super) log_rd_per_qp: u8,
    pub(super) log_mc_table_sz: u8,
    pub(super) log_mpt_sz: u8,
}
