//! This module contains functionality to interact with the firmware.

use core::mem::size_of;

use alloc::{format, string::String, vec::Vec};
use byteorder::BigEndian;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};
use mlx_infiniband::ibv_mtu;
use modular_bitfield_msb::{bitfield, specifiers::{B1, B10, B104, B11, B12, B15, B2, B20, B22, B24, B25, B27, B3, B31, B36, B4, B42, B45, B5, B6, B63, B7, B72, B88, B91}};
use volatile::WriteOnly;
use zerocopy::{AsBytes, FromBytes, U16, U32, U64};

use super::{
    cmd::{CommandInterface, MadDemuxOpcodeModifier, Opcode},
    device::{DEFAULT_UAR_PAGE_SHIFT, PAGE_SHIFT},
    icm::{MappedIcmAuxiliaryArea, ICM_PAGE_SHIFT},
    port::Port,
};

#[derive(Clone, FromBytes)]
#[repr(C, packed)]
pub(super) struct Firmware {
    pages: U16<BigEndian>,
    pub(super) major: U16<BigEndian>,
    pub(super) sub_minor: U16<BigEndian>,
    pub(super) minor: U16<BigEndian>,
    _padding1: u16,
    ix_rev: U16<BigEndian>,
    _padding2: [u8; 22], // contains the build timestamp
    clr_int_base: U64<BigEndian>,
    clr_int_bar: u8,
    // many fields follow
}

impl Firmware {
    pub(super) fn query(cmd: &mut CommandInterface) -> Result<Self, &'static str> {
        trace!("asking the card to provide information about its firmware...");
        let page: MappedPages = cmd.execute_command(
            Opcode::QueryFw, (), (), 0,
        )?;
        let mut fw = page.as_type::<Firmware>(0)?.clone();
        fw.clr_int_bar = (fw.clr_int_bar >> 6) * 2;
        trace!("got firmware info: {fw:?}");
        Ok(fw)
    }
    
    pub(super) fn map_area(&self, cmd: &mut CommandInterface) -> Result<MappedFirmwareArea, &'static str> {
        const MAX_CHUNK_LOG2: u32 = 18;
        trace!("mapping firmware area...");

        let size = PAGE_SIZE * usize::from(self.pages);
        let (pages, physical) = create_contiguous_mapping(size, DMA_FLAGS)?;
        let mut align = physical.value().trailing_zeros();
        if align > MAX_CHUNK_LOG2 {
            trace!("alignment greater than max chunk size, defaulting to 256KB");
            align = MAX_CHUNK_LOG2;
        }

        let mut num_entries = size / (1 << align);
        if size % (1 << align) != 0 {
            num_entries += 1;
        }
        // batch as many vpm entries as fit in a mailbox to make bootup faster
        let mut vpms = [VirtualPhysicalMapping::default(); 256];
        let mut pointer = physical;
        while num_entries > 0 {
            let mut chunk = PAGE_SIZE / size_of::<VirtualPhysicalMapping>();
            if num_entries < chunk {
                chunk = num_entries;
            }
            for i in 0..chunk {
                vpms[i].physical_address.set(pointer.value() as u64 | (align as u64 - ICM_PAGE_SHIFT as u64));
                pointer += 1 << align;
            }
            cmd.execute_command(
                Opcode::MapFa, (), vpms.as_bytes(), chunk.try_into().unwrap(),
            )?;
            num_entries -= chunk;
        }
        trace!("mapped {} pages for firmware area", self.pages);

        Ok(MappedFirmwareArea {
            memory: Some((pages, physical)),
            icm_aux_area: None,
        })
    }
    
    /// Format the version as a string.
    pub(super) fn version(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.sub_minor)
    }
}

impl core::fmt::Debug for Firmware {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("Firmware")
            .field("clr_int_bar", &self.clr_int_bar)
            .field("clr_int_base", &format_args!("{:#x}", self.clr_int_base))
            .field("version", &self.version())
            .field("ix_rev", &self.ix_rev.get())
            .field("size", &format_args!(
                "{}.{} KB",
                (self.pages.get() as usize * PAGE_SIZE) / 1024,
                (self.pages.get() as usize * PAGE_SIZE) % 1024,
            ))
            .finish()
    }
}


#[derive(Clone, AsBytes, Default, Copy)]
#[repr(C, packed)]
pub(super) struct VirtualPhysicalMapping {
    // actually just 52 bits
    pub(super) virtual_address: U64<BigEndian>,
    // actually just 52 bits and then log2size
    pub(super) physical_address: U64<BigEndian>,
}

/// A mapped firmware area.
/// 
/// Instead of dropping, please unmap the area from the card.
pub(super) struct MappedFirmwareArea {
    memory: Option<(MappedPages, PhysicalAddress)>,
    icm_aux_area: Option<MappedIcmAuxiliaryArea>,
}

impl MappedFirmwareArea {
    pub(super) fn run(&self, cmd: &mut CommandInterface) -> Result<(), &'static str> {
        cmd.execute_command(Opcode::RunFw, (), (), 0)?;
        trace!("successfully run firmware");
        Ok(())
    }

     pub(super) fn query_capabilities(&self, cmd: &mut CommandInterface) -> Result<Capabilities, &'static str> {
        let page: MappedPages = cmd.execute_command(
            Opcode::QueryDevCap, (), (), 0,
        )?;
        let mut caps = Capabilities::from_bytes(page.as_slice(
            0, size_of::<Capabilities>()
        )?.try_into().unwrap());
        // each UAR has 4 EQ doorbells; so if a UAR is reserved,
        // then we can't use any EQs whose doorbell falls on that page,
        // even if the EQ itself isn't reserved
        if caps.num_rsvd_uars() * 4 > caps.num_rsvd_eqs() {
            caps.set_num_rsvd_eqs(caps.num_rsvd_uars() * 4);
        }
        // TODO: caps.reserved_qpt_cnt[MLX3_QP_REGION_FW] = 1 << caps.log2_rsvd_qps
        // no merge of flags and ext_flags here
        
        trace!("max BF pages: {}", 1 << caps.log_max_bf_pages());
        // TODO: caps.reserved_qpt_cnt[MLX3_QP_REGION_ETH_ADDR] = (1 << caps.log_num_macs) * (1 << caps.log_num_vlans) * caps.num_ports
        trace!("got caps: {:?}", caps);
        Ok(caps)
    }
    
    /// Unmaps the area from the card. Further usage requires a software reset.
    pub(super) fn unmap(mut self, cmd: &mut CommandInterface) -> Result<(), &'static str> {
        if let Some(icm_aux_area) = self.icm_aux_area.take() {
            icm_aux_area
                .unmap(cmd)
                .unwrap()
        }
        trace!("unmapping firmware area...");
        cmd.execute_command(Opcode::UnmapFa, (), (), 0)?;
        trace!("successfully unmapped firmware area");
        // actually free the memory
        self.memory.take().unwrap();
        Ok(())
    }
    
    /// Set the ICM size.
    /// 
    /// Returns `aux_pages`, the auxiliary ICM size in pages.
    pub(crate) fn set_icm(&self, cmd: &mut CommandInterface, icm_size: u64) -> Result<u64, &'static str> {
        let aux_pages = cmd.execute_command(
            Opcode::SetIcmSize, (), icm_size, 0,
        )?;
        // TODO: round up number of system pages needed if ICM_PAGE_SIZE < PAGE_SIZE
        trace!("ICM auxilliary area requires {aux_pages} 4K pages");
        Ok(aux_pages)
    }

    /// Map the ICM auxiliary area.
    pub(super) fn map_icm_aux(
        &mut self, cmd: &mut CommandInterface, aux_pages: u64,
    ) -> Result<&MappedIcmAuxiliaryArea, &'static str> {
        if self.icm_aux_area.is_some() {
            return Err("ICM auxiliary area has already been mapped");
        }
        // TODO: merge this with Firmware::map_area?
        trace!("mapping ICM auxiliary area...");
        let (pages, physical) = create_contiguous_mapping(
            aux_pages as usize * PAGE_SIZE, DMA_FLAGS,
        )?;
        let mut align = physical.value().trailing_zeros();
        if align > PAGE_SIZE.ilog2() {
            trace!("alignment greater than max chunk size, defaulting to 256KB");
            align = PAGE_SIZE.ilog2();
        }
        let size = aux_pages * PAGE_SIZE as u64;
        let mut count = size / (1 << align);
        if size % (1 << align) != 0 {
            count += 1;
        }
        // TODO: we can batch as many vpm entries as fit in a mailbox (1 page)
        // rather than 1 chunk per mailbox, this will make bootup faster
        let mut vpm = VirtualPhysicalMapping::default();
        let mut pointer = physical;
        for _ in 0..count {
            vpm.physical_address.set(pointer.value() as u64 | (align as u64 - ICM_PAGE_SHIFT as u64));
            cmd.execute_command(Opcode::MapIcmAux, (), vpm.as_bytes(), 1)?;
            pointer += 1 << align;
        }
        trace!("mapped {} pages for ICM auxiliary area", aux_pages);

        self.icm_aux_area = Some(MappedIcmAuxiliaryArea::new(pages, physical));
        Ok(self.icm_aux_area.as_ref().unwrap())
    }
}

impl Drop for MappedFirmwareArea {
    fn drop(&mut self) {
        if self.icm_aux_area.is_some() || self.memory.is_some() {
            panic!("please unmap instead of dropping");
        }
    }
}

#[bitfield]
pub(super) struct Capabilities {
    #[skip] __: u128,
    log_max_srq_sz: u8,
    pub(super) log_max_qp_sz: u8,
    #[skip] __: B4,
    pub(super) log2_rsvd_qps: B4,
    #[skip] __: B3,
    log_max_qp: B5,
    pub(super) log2_rsvd_srqs: B4,
    #[skip] __: B7,
    log_max_srqs: B5,
    #[skip] __: B2,
    num_rsvd_eec: B6,
    #[skip] __: B4,
    log_max_eec: B4,
    // deprecated
    pub(super) num_rsvd_eqs: u8,
    log_max_cq_sz: u8,
    #[skip] __: B4,
    pub(super) log2_rsvd_cqs: B4,
    #[skip] __: B3,
    log_max_cq: B5,
    log_max_eq_sz: u8,
    #[skip] __: B2,
    log_max_d_mpts: B6,
    // deprecated
    #[skip] __: B4,
    log2_rsvd_eqs: B4,
    #[skip] __: B4,
    log_max_eq: B4,
    pub(super) log2_rsvd_mtts: B4,
    #[skip] __: B4,
    #[skip] __: B1,
    log_max_mrw_sz: B7,
    #[skip] __: B4,
    pub(super) log2_rsvd_mrws: B4,
    #[skip] __: B2,
    log_max_mtts: B6,
    #[skip] __: u16,
    #[skip] __: B4,
    // not present in mlx3
    num_sys_eq: B12,
    // max_av?
    #[skip] __: B10,
    log_max_ra_req_qp: B6,
    #[skip] __: B10,
    log_max_ra_res_qp: B6,
    #[skip] __: B11,
    log2_max_gso_sz: B5,
    rss: u8,
    #[skip] __: B2,
    rdma: B6,
    #[skip] __: B31,
    rsz_srq: B1,
    port_beacon: B1,
    #[skip] __: B7,
    ack_delay: u8,
    mtu_width: u8,
    #[skip] __: B4,
    num_ports: B4,
    #[skip] __: B3,
    pub(super) log_max_msg: B5,
    #[skip] __: u16,
    max_gid: u8,
    rate_support: u16,
    cq_timestamp: B1,
    #[skip] __: B15,
    // max_pkey?

    // flags: u64,
    #[skip] __: bool,
    cqe_64b: bool,
    eqe_64b: bool,
    #[skip] __: bool,
    port_mng_chg_ev: bool,
    #[skip] __: B3,
    sense_port: bool,
    #[skip] __: bool,
    set_eth_shed: bool,
    rss_ip_frag: bool,
    #[skip] __: B2,
    ethernet_user_prio: bool,
    counters: bool,
    ptp1588: bool,
    #[skip] __: B2,
    ethertype_steer: bool,
    vlan_steer: bool,
    vep_mc_steer: bool,
    vep_uc_steer: bool,
    udp_rss: bool,
    thermal_warning: bool,
    wol_port2: bool,
    wol_port1: bool,
    header_split: bool,
    #[skip] __: bool,
    fcs_keep: bool,
    mc_loopback: bool,
    uc_loopback: bool,
    fcoe_t11: bool,
    roce: bool,
    ipv6_checksum: bool,
    ud_sw: bool,
    #[skip] __: bool,
    l2_multicast: bool,
    router_mode: bool,
    paging: bool,
    #[skip] __: bool,
    ud_mcast_ipv4: bool,
    ud_mcast: bool,
    avp: bool,
    raw_mcast: bool,
    atomic: bool,
    apm: bool,
    mem_window: bool,
    blh: bool,
    raw_ipv6: bool,
    raw_ethertype: bool,
    dpdp: bool,
    fcoe: bool,
    vmm: bool,
    bad_qkey: bool,
    bad_pkey: bool,
    roce_checksum: bool,
    srq: bool,
    fcob: bool,
    reliable_mc: bool,
    xrc: bool,
    ud: bool,
    uc: bool,
    rc: bool,

    num_rsvd_uars: B4,
    #[skip] __: B6,
    uar_sz: B6,
    #[skip] __: u8,
    log_page_sz: u8,
    bf: bool,
    #[skip] __: B10,
    log_bf_reg_sz: B5,
    #[skip] __: B2,
    log_max_bf_regs_per_page: B6,
    #[skip] __: B2,
    log_max_bf_pages: B6,
    #[skip] __: u8,
    pub(super) max_sg_sq: u8,
    pub(super) max_desc_sz_sq: u16,
    #[skip] __: u8,
    pub(super) max_sg_rq: u8,
    max_desc_sz_rq: u16,
    // user_mac_en?
    // svlan_by_qp?
    #[skip] __: B72,
    log_max_qp_mcg: u8,
    num_rsvd_mcgs: u8,
    log_max_mcg: u8,
    num_rsvd_pds: B4,
    #[skip] __: B7,
    log_max_pd: B5,
    num_rsvd_xrcds: B4,
    #[skip] __: B7,
    log_max_xrcd: B5,
    max_if_cnt_basic: u32,
    max_if_cnt_extended: u32,
    ext2_flags: u16,
    #[skip] __: u16,
    flow_steering_flags: u16,
    flow_steering_range: u8,
    flow_steering_max_qp_per_entry: u8,
    sl2vl_event: u8,
    #[skip] __: u8,
    cq_eq_cache_line_stride: u8,
    #[skip] __: B7,
    ecn_qcn_ver: B1,
    #[skip ]__: u32,
    pub(super) rdmarc_entry_sz: u16,
    pub(super) qpc_entry_sz: u16,
    pub(super) aux_entry_sz: u16,
    pub(super) altc_entry_sz: u16,
    pub(super) eqc_entry_sz: u16,
    pub(super) cqc_entry_sz: u16,
    pub(super) srq_entry_sz: u16,
    pub(super) c_mpt_entry_sz: u16,
    pub(super) mtt_entry_sz: u16,
    pub(super) d_mpt_entry_sz: u16,
    bmme_flags: u16,
    phv_en: u16,
    pub(super) reserved_lkey: u32,
    diag_flags: u32,
    pub(super) max_icm_sz: u64,
    #[skip] __: u8,
    dmfs_high_rate_qpn_base: B24,
    #[skip] __: u8,
    dmfs_high_rate_qpn_range: B24,
    #[skip] __: B31,
    mad_demux: B1,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: B36,
    qp_rate_limit_max: B12,
    // actually just u12
    #[skip] __: B4,
    qp_rate_limit_min: B12,
    // reserved space follows
}

impl Capabilities {
    fn bf_regs_per_page(&self) -> usize {
        if self.bf() {
            if 1 << self.log_max_bf_regs_per_page() > PAGE_SIZE / self.bf_reg_size() {
                3
            } else {
                1 << self.log_max_bf_regs_per_page()
            }
        } else {
            0
        }
    }

    fn bf_reg_size(&self) -> usize {
        if self.bf() {
            1 << self.log_bf_reg_sz()
        } else {
            0
        }
    }

    fn num_uars(&self) -> usize {
        usize::try_from(self.uar_size()).unwrap() / PAGE_SIZE
    }

    fn uar_size(&self) -> u64 {
        1 << (self.uar_sz() + 20)
    }

    pub(super) fn get_doorbells_and_blueflame(
        &self, uar: MappedPages,
    ) -> Result<(Vec<MappedPages>, Vec<MappedPages>), &'static str> {
        let mut doorbells = Vec::new();
        let mut blueflame = Vec::new();
        let mut rest = uar;
        let mut idx = 0;
        while rest.size_in_pages() > 1 {
            let next_page = *rest.start() + 1;
            let split = rest
                .split(next_page)
                .map_err(|_| "failed to split UAR")?;
            if idx <= self.num_uars() {
                doorbells.push(split.0);
            } else {
                blueflame.push(split.0)
            }
            rest = split.1;
            idx += 1;
        }

        Ok((doorbells, blueflame))
    }
}

impl core::fmt::Debug for Capabilities {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f
            .debug_struct("Capabilities")
            .field("BlueFlame available", &self.bf())
            .field("BlueFlame reg size", &self.bf_reg_size())
            .field("BlueFlame regs/page", &self.bf_regs_per_page())
            .field("Max ICM size (PB)", &(self.max_icm_sz() >> 50))
            .field("Max QPs", &(1 << self.log_max_qp()))
            .field("reserved QPs", &(1 << self.log2_rsvd_qps()))
            .field("QPC entry size", &self.qpc_entry_sz())
            .field("Max SRQs", &(1 << self.log_max_srqs()))
            .field("reserved SRQs", &(1 << self.log2_rsvd_srqs()))
            .field("SRQ entry size", &self.srq_entry_sz())
            .field("Max CQs", &(1 << self.log_max_cq()))
            .field("reserved CQs", &(1 << self.log2_rsvd_cqs()))
            .field("CQC entry size", &self.cqc_entry_sz())
            .field("Max EQs", &(1 << self.log_max_eq()))
            .field("reserved EQs", &(1 << self.log2_rsvd_eqs()))
            .field("EQC entry size", &self.eqc_entry_sz())
            .field("reserved MPTs", &(1 << self.log2_rsvd_mrws()))
            .field("reserved MTTs", &(1 << self.log2_rsvd_mtts()))
            .field("Max CQE count", &(1 << self.log_max_cq_sz()))
            .field("max QPE count", &(1 << self.log_max_qp_sz()))
            .field("max SRQe count", &(1 << self.log_max_eq_sz()))
            .field("MTT Entry Size", &self.mtt_entry_sz())
            .field("Reserved MTTs", &(1 << self.log2_rsvd_mtts()))
            .field("cMPT Entry Size", &self.c_mpt_entry_sz())
            .field("dMPT Entry Size", &self.d_mpt_entry_sz())
            .field("Reserved UAR", &self.num_rsvd_uars())
            .field("UAR Size", &self.uar_size())
            .field("Num UAR", &self.num_uars())
            .field("Network Port count", &self.num_ports())
            .field("Min Page Size", &(1 << self.log_page_sz()))
            .field("Max SQ desc size WQE Entry Size", &self.max_desc_sz_sq())
            .field("max SQ S/G WQE Entries", &self.max_sg_sq())
            .field("Max RQ desc size", &self.max_desc_sz_rq())
            .field("max RQ S/G", &self.max_sg_rq())
            .field("Max Message Size", &(1 << self.log_max_msg()))
            .field("Unicast loopback support", &self.uc_loopback())
            .field("Multicast loopback support", &self.mc_loopback())
            .field("Header-data split support", &self.header_split())
            .field("Wake on LAN (port 1) support", &self.wol_port1())
            .field("Wake on LAN (port 2) support", &self.wol_port2())
            .field("Thermal warning event", &self.thermal_warning())
            .field("UDP RSS support", &self.udp_rss())
            .field("Unicast VEP steering support", &self.vep_uc_steer())
            .field("Multicast VEP steering support", &self.vep_mc_steer())
            .field("VLAN steering support", &self.vlan_steer())
            .field("EtherType steering support", &self.ethertype_steer())
            // WQE v1 support
            .field("PTP1588 support", &self.ptp1588())
            .field("QPC Ethernet user priority support", &self.ethernet_user_prio())
            .field("64B EQE support", &self.eqe_64b())
            .field("64B CQE support", &self.cqe_64b())
            .field("RC transport support", &self.rc())
            .field("UC transport support", &self.uc())
            .field("UD transport support", &self.ud())
            .field("XRC transport support", &self.xrc())
            .field("Reliable Multicast support", &self.reliable_mc())
            .field("FCoB support", &self.fcob())
            .field("SRQ support", &self.srq())
            .field("RoCE checksum support", &self.roce_checksum())
            .field("Pkey Violation Counter support", &self.bad_pkey())
            .field("Qkey Violation Counter support", &self.bad_qkey())
            .field("VMM support", &self.vmm())
            .field("FCoE support", &self.fcoe())
            .field("DPDP support", &self.dpdp())
            .field("Raw Ethertype support", &self.raw_ethertype())
            .field("Raw IPv6 support", &self.raw_ipv6())
            .field("LSO header support", &self.blh())
            .field("Memory window support", &self.mem_window())
            .field("Automatic Path Migration support", &self.apm())
            .field("Atomic op support", &self.atomic())
            .field("Raw multicast support", &self.raw_mcast())
            .field("AVP support", &self.avp())
            .field("UD Multicast support", &self.ud_mcast())
            .field("UD IPv4 Multicast support", &self.ud_mcast_ipv4())
            // DIF support
            .field("Paging on Demand support", &self.paging())
            .field("Router mode support", &self.router_mode())
            .field("L2 Multicast support", &self.l2_multicast())
            .field("UD transport SW parsing support", &self.ud_sw())
            .field("TCP checksum support for IPv6 support", &self.ipv6_checksum())
            .field("RoCE support", &self.roce())
            .field("FCoE T11 frame support", &self.fcoe_t11())
            .finish()
    }
}

#[derive(FromBytes)]
#[repr(C, packed)]
pub(super) struct DoorbellEq {
    pub(super) val: WriteOnly<U32<BigEndian>>,
    _padding: u32,
}

#[derive(FromBytes)]
#[repr(C, packed)]
pub(super) struct DoorbellPage {
    _padding1: u128,
    _padding2: u32,
    pub(super) send_queue_number: WriteOnly<U32<BigEndian>>,
    _padding3: u64,

    // CQ
    /// contains the sequence number, the command and the cq number
    pub(super) cq_sn_cmd_num: WriteOnly<U32<BigEndian>>,
    pub(super) cq_consumer_index: WriteOnly<U32<BigEndian>>,

    // skip 502 u32
    _padding4: [u32; 502],

    // EQ
    // for the EQ number n the relevant doorbell is in
    // DoorbellPage (n / 4) and eq (n % 4)
    pub(super) eqs: [DoorbellEq; 4],

    // skip 503 u32
    _padding9: [u32; 503],
}

#[bitfield]
pub(super) struct InitHcaParameters {
    version: u8,
    #[skip] __: B104,
    cacheline_sz: B3,
    // vxlan?
    #[skip] __: B45,
    flags: u32,
    recoverable_error_event: bool,
    #[skip] __: B63,

    // QPC parameters
    #[skip] __: u128,
    /// contains both the base (in the upper 59 bits) and log_num (in the lower 5 bits)
    qpc_base_num: u64,
    #[skip] __: u128,
    /// contains both the base (in the upper 59 bits) and log_num (in the lower 5 bits)
    qpc_srqc_base_num: u64,
    /// contains both the base (in the upper 59 bits) and log_num (in the lower 5 bits)
    qpc_cqc_base_num: u64,
    #[skip] __: bool,
    qpc_cqe: bool,
    qpc_eqe: bool,
    #[skip] __: B22,
    qpc_eqe_stride: B3,
    #[skip] __: bool,
    qpc_cqe_stride: B3,
    #[skip] __: u32,
    pub(super) qpc_altc_base: u64,
    #[skip] __: u64,
    pub(super) qpc_auxc_base: u64,
    #[skip] __: u64,
    /// contains both the base (in the upper 59 bits) and log_num (in the lower 5 bits)
    qpc_eqc_base_num: u64,
    #[skip] __: B20,
    qpc_num_sys_eqs: B12,
    #[skip] __: u32,
    /// contains both the base (in the upper 59 bits) and log_num (in the lower 3 bits)
    qpc_rdmarc_base_num: u64,
    #[skip] __: u64,

    // skip 8 u32
    #[skip] __: u128,
    #[skip] __: u128,

    // multicast parameters
    pub(super) mc_base: u64,
    #[skip] __: B91,
    pub(super) mc_log_entry_sz: B5,
    #[skip] __: B27,
    pub(super) mc_log_hash_sz: B5,
    #[skip] __: B4,
    mc_uc_steering: bool,
    #[skip] __: B22,
    pub(super) mc_log_table_sz: B5,
    #[skip] __: u32,

    #[skip] __: u128,
    
    // translation and protection table parameters
    pub(super) tpt_dmpt_base: u64,
    /// enable memory windows
    tpt_mw: bool,
    #[skip] __: B25,
    pub(super) tpt_log_dmpt_sz: B6,
    #[skip] __: u32,
    pub(super) tpt_mtt_base: u64,
    pub(super) tpt_cmpt_base: u64,
    #[skip] __: u64,

    #[skip] __: u64,

    // UAR parameters
    #[skip] __: B88,
    /// log page size in 4k chunks
    uar_log_sz: u8,
    #[skip] __: u128,

    // skip 36 u32
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,

    // flow steering parameters
    fs_base: u64,
    #[skip] __: B91,
    fs_log_entry_sz: B5,
    #[skip] __: u32,
    fs_a0: B2,
    #[skip] __: B25,
    fs_log_table_sz: B5,
    #[skip] __: B42,
    fs_eth_bits: B6,
    fs_eth_num_addrs: u16,
    #[skip] __: B12,
    fs_ib_bits: B3,
    #[skip] __: bool,
    fs_ib_num_addrs: u16,

    // skip 66 u32
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u64,
}

impl InitHcaParameters {
    pub(super) fn init_hca(
        &mut self, cmd: &mut CommandInterface,
    ) -> Result<Hca, &'static str> {

        // set the needed values
        self.set_version(2); // version must be 2
        // TODO: use a library for this
        let mut flags = 0;
        flags &= !(1 << 1); // little endian on the host
        flags |= 1 << 4; // enable counters / checksums
        flags |= 1; // check port for UD adress vector
        self.set_flags(flags);
        self.set_uar_log_sz(DEFAULT_UAR_PAGE_SHIFT - PAGE_SHIFT);
        
        // execute the command
        cmd.execute_command(Opcode::InitHca, (), &self.bytes[..], 0)?;
        trace!("HCA initialized");
        Ok(Hca { initialized: true, })
    }

    /// Get the number of queue pairs out of qpc_base_num.
    pub(super) fn num_qps(&self) -> usize {
        1 << (self.qpc_base_num() & 0x1f)
    }

    /// Set the (log) number of queue pairs in qpc_base_num.
    pub(super) fn set_qpc_log_qp(&mut self, new: u64) {
        assert_eq!(new & 0x1f, new);
        self.set_qpc_base_num(
            self.qpc_base_num() & 0xffffffffffffffe0 | new & 0x1f
        );
    }

    /// Get the QPC base out of qpc_base_num.
    pub(super) fn qpc_base(&self) -> u64 {
        self.qpc_base_num() & 0xffffffffffffffe0
    }

    /// Set the QPC base in qpc_base_num
    pub(super) fn set_qpc_base(&mut self, new: u64) {
        assert_eq!(new & 0xffffffffffffffe0, new);
        self.set_qpc_base_num(
            self.qpc_base_num() & 0x1f | new & 0xffffffffffffffe0
        );
    }

    /// Get the number of SRQs out of qpc_srqc_base_num.
    pub(super) fn num_srqs(&self) -> usize {
        1 << (self.qpc_srqc_base_num() & 0x1f)
    }

    /// Set the (log) number of SRQs in qpc_srqc_base_num.
    pub(super) fn set_qpc_log_srq(&mut self, new: u64) {
        assert_eq!(new & 0x1f, new);
        self.set_qpc_srqc_base_num(
            self.qpc_srqc_base_num() & 0xffffffffffffffe0 | new & 0x1f
        );
    }

    /// Get the SRQ base out of qpc_srqc_base_num
    pub(super) fn qpc_srqc_base(&self) -> u64 {
        self.qpc_srqc_base_num() & 0xffffffffffffffe0
    }

    /// Set the SRQ base in qpc_srqc_base_num
    pub(super) fn set_qpc_srqc_base(&mut self, new: u64) {
        assert_eq!(new & 0xffffffffffffffe0, new);
        self.set_qpc_srqc_base_num(
            self.qpc_srqc_base_num() & 0x1f | new & 0xffffffffffffffe0
        );
    }

    /// Get the number of completion queues out of qpc_cqc_base_num.
    pub(super) fn num_cqs(&self) -> usize {
        1 << (self.qpc_cqc_base_num() & 0x1f)
    }

    /// Set the (log) number of completions queues in qpc_cqc_base_num.
    pub(super) fn set_qpc_log_cq(&mut self, new: u64) {
        assert_eq!(new & 0x1f, new);
        self.set_qpc_cqc_base_num(
            self.qpc_cqc_base_num() & 0xffffffffffffffe0 | new & 0x1f
        );
    }

    /// Get the CQC base out of qpc_cqc_base_num
    pub(super) fn qpc_cqc_base(&self) -> u64 {
        self.qpc_cqc_base_num() & 0xffffffffffffffe0
    }

    /// Set the CQC base in qpc_cqc_base_num
    pub(super) fn set_qpc_cqc_base(&mut self, new: u64) {
        assert_eq!(new & 0xffffffffffffffe0, new);
        self.set_qpc_cqc_base_num(
            self.qpc_cqc_base_num() & 0x1f | new & 0xffffffffffffffe0
        );
    }

    /// Get the number of event queues out of qpc_eqc_base_num.
    pub(super) fn num_eqs(&self) -> usize {
        1 << (self.qpc_eqc_base_num() & 0x1f)
    }

    /// Set the (log) number of event queues in qpc_eqc_base_num.
    pub(super) fn set_qpc_log_eq(&mut self, new: u64) {
        assert_eq!(new & 0x1f, new);
        self.set_qpc_eqc_base_num(
            self.qpc_eqc_base_num() & 0xffffffffffffffe0 | new & 0x1f
        );
    }

    /// Get the EQC base out of qpc_eqc_base_num.
    pub(super) fn qpc_eqc_base(&self) -> u64 {
        self.qpc_eqc_base_num() & 0xffffffffffffffe0
    }

    /// Set the EQC base in qpc_eqc_base_num
    pub(super) fn set_qpc_eqc_base(&mut self, new: u64) {
        assert_eq!(new & 0xffffffffffffffe0, new);
        self.set_qpc_eqc_base_num(
            self.qpc_eqc_base_num() & 0x1f | new & 0xffffffffffffffe0
        );
    }

    /// Set the (log) number of RDs in qpc_rdmarc_base_num.
    pub(super) fn set_qpc_log_rd(&mut self, new: u8) {
        assert_eq!(new & 0x7, new);
        self.set_qpc_rdmarc_base_num(
            self.qpc_rdmarc_base_num() & 0xffffffffffffffe0 | new as u64 & 0x7
        );
    }

    /// Get the RDMARC base out of qpc_rdmarc_base_num.
    pub(super) fn qpc_rdmarc_base(&self) -> u64 {
        self.qpc_rdmarc_base_num() & 0xffffffffffffffe0
    }

    /// Set the RDMARC base in qpc_rdmarc_base_num
    pub(super) fn set_qpc_rdmarc_base(&mut self, new: u64) {
        assert_eq!(new & 0xffffffffffffffe0, new);
        self.set_qpc_rdmarc_base_num(
            self.qpc_rdmarc_base_num() & 0x7 | new & 0xffffffffffffffe0
        );
    }
}

// an initialized Host Channel Adapter
pub(super) struct Hca {
    initialized: bool,
}

impl Hca {
    pub(super) fn close(
        mut self, cmd: &mut CommandInterface,
    ) -> Result<(), &'static str> {
        trace!("Closing HCA...");
        cmd.execute_command(Opcode::CloseHca, (), (), 0)?;
        self.initialized = false;
        trace!("HCA closed successfully");
        Ok(())
    }

    pub(super) fn query_adapter(
        &self, cmd: &mut CommandInterface,
    ) -> Result<Adapter, &'static str> {
        let page: MappedPages = cmd.execute_command(
            Opcode::QueryAdapter, (), (), 0,
        )?;
        Ok(Adapter::from_bytes(page.as_slice(
            0, size_of::<Adapter>(),
        )?.try_into().unwrap()))
    }
    
    pub(super) fn config_mad_demux(
        &self, cmd: &mut CommandInterface, caps: &Capabilities,
    ) -> Result<(), &'static str> {
        // TODO: check if mad_demux is supported

        // Query mad_demux to find out which MADs are handled by internal sma
        const SUBNET_MANAGEMENT_CLASS: u32 = 0x1;
        let page: MappedPages = cmd.execute_command(
            Opcode::MadDemux, MadDemuxOpcodeModifier::QueryRestrictions,
            (), SUBNET_MANAGEMENT_CLASS,
        )?;
        // TODO: create a struct for this
        // Config mad_demux to handle all MADs returned by the query above
        cmd.execute_command(
            Opcode::MadDemux, MadDemuxOpcodeModifier::Configure,
            page.as_slice(0, PAGE_SIZE)?, SUBNET_MANAGEMENT_CLASS,
        )?;
        Ok(())
    }
    
    pub(crate) fn init_ports(
        &self, cmd: &mut CommandInterface, caps: &Capabilities,
    ) -> Result<Vec<Port>, &'static str> {
        let mut ports = Vec::with_capacity(caps.num_ports().into());
        for number in 1..=caps.num_ports() {
            let port = Port::new(cmd, number, ibv_mtu::Mtu4096, None)?;
            ports.push(port);
        }
        Ok(ports)
    }
}

impl Drop for Hca {
    fn drop(&mut self) {
        if self.initialized {
            panic!("please close instead of dropping")
        }
    }
}

#[bitfield]
pub(super) struct Adapter {
    #[skip] __: u128,
    /// When PCIe interrupt messages are being used, this value is used for
    /// clearing an interrupt. To clear an interrupt, the driver should write
    /// the value (1<<intapin) into the clr_int register. When using an MSI-X,
    /// this register is not used.
    inta_pin: u8,
    #[skip] __: B24,
    // skip 58 u32
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u128,
    #[skip] __: u64
}
