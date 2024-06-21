use core::mem::size_of;

use alloc::vec::Vec;
use memory::{create_contiguous_mapping, MappedPages, PhysicalAddress, DMA_FLAGS, PAGE_SIZE};

use crate::{cmd::{CommandMailBox, Opcode}, fw::{Capabilities, InitHcaParameters, VirtualPhysicalMapping}, mcg::get_mgm_entry_size};

#[repr(u64)]
#[derive(Default, Clone, Copy)]
enum CmptType {
    #[default] QP, SRQ, CQ, EQ,
}

/// A mapped ICM auxiliary area.
/// 
/// Instead of dropping, please unmap the area from the card.
pub(super) struct MappedIcmAuxiliaryArea {
    pages: MappedPages,
    physical: PhysicalAddress,
}

impl MappedIcmAuxiliaryArea {
    pub(super) fn new(pages: MappedPages, physical: PhysicalAddress) -> Self {
        Self { pages, physical, }
    }

    /// Unmaps the area from the card.
    pub(super) fn unmap(self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        trace!("unmapping ICM auxiliary area...");
        let mut cmd = CommandMailBox::new(config_regs)?;
        cmd.execute_command(Opcode::UnmapIcmAux, 0, 0, 0)?;
        trace!("successfully unmapped ICM auxiliary area");
        core::mem::forget(self); // don't run the drop handler in this case
        Ok(())
    }
    
    pub(super) fn map_icm_tables(
        &self, config_regs: &mut MappedPages,
        init_hca_params: &InitHcaParameters, caps: &Capabilities,
    ) -> Result<MappedIcmTables, &'static str> {
        // first, map the cmpt tables
        const CMPT_SHIFT: u8 = 24;
        // TODO: do we really need to calculate the bases here?
        let qp_cmpt_table = self.init_icm_table(
            config_regs, caps.c_mpt_entry_sz(), init_hca_params.num_qps,
            1 << caps.log2_rsvd_qps(),
            init_hca_params.cmpt_base + (CmptType::QP as u64 * caps.c_mpt_entry_sz() as u64) << CMPT_SHIFT,
        )?;
        trace!("mapped QP cMPT table");
        let srq_cmpt_table = self.init_icm_table(
            config_regs, caps.c_mpt_entry_sz(), init_hca_params.num_srqs,
            1 << caps.log2_rsvd_srqs(),
            init_hca_params.cmpt_base + (CmptType::SRQ as u64 * caps.c_mpt_entry_sz() as u64) << CMPT_SHIFT,
        )?;
        trace!("mapped SRQ cMPT table");
        let cq_cmpt_table = self.init_icm_table(
            config_regs, caps.c_mpt_entry_sz(), init_hca_params.num_cqs,
            1 << caps.log2_rsvd_cqs(),
            init_hca_params.cmpt_base + (CmptType::CQ as u64 * caps.c_mpt_entry_sz() as u64) << CMPT_SHIFT,
        )?;
        trace!("mapped CQ cMPT table");
        let eq_cmpt_table = self.init_icm_table(
            config_regs, caps.c_mpt_entry_sz(), init_hca_params.num_eqs,
            init_hca_params.num_eqs,
            init_hca_params.cmpt_base + (CmptType::EQ as u64 * caps.c_mpt_entry_sz() as u64) << CMPT_SHIFT,
        )?;
        trace!("mapped EQ cMPT table");

        // then, the rest
        let eq_table = EqTable {
            table: self.init_icm_table(
                config_regs, caps.eqc_entry_sz(), init_hca_params.num_eqs,
                init_hca_params.num_eqs, init_hca_params.eqc_base,
            )?,
            cmpt_table: eq_cmpt_table,
        };
        // Assuming Cache Line is 64 Bytes. Reserved MTT entries must be
        // aligned up to a cacheline boundary, since the FW will write to them,
        // while the driver writes to all other MTT entries. (The variable
        // caps.mtt_entry_sz below is really the MTT segment size, not the
        // raw entry size.)
        let reserved_mtts = (
            (1 << caps.log2_rsvd_mtts() as usize) * caps.mtt_entry_sz() as usize
        ).next_multiple_of(64) / caps.mtt_entry_sz() as usize;
        let mr_table = MrTable {
            mtt_table: self.init_icm_table(
                config_regs, caps.mtt_entry_sz(), init_hca_params.num_mtts,
                reserved_mtts, init_hca_params.mtt_base,
            )?,
            dmpt_table: self.init_icm_table(
                config_regs, caps.d_mpt_entry_sz(), init_hca_params.num_mpts,
                1 << caps.log2_rsvd_mrws(), init_hca_params.dmpt_base,
            )?,
        };
        let qp_table = QpTable {
            table: self.init_icm_table(
                config_regs, caps.qpc_entry_sz(), init_hca_params.num_qps,
                1 << caps.log2_rsvd_qps(), init_hca_params.qpc_base,
            )?,
            cmpt_table: qp_cmpt_table,
            auxc_table: self.init_icm_table(
                config_regs, caps.aux_entry_sz(), init_hca_params.num_qps,
                1 << caps.log2_rsvd_qps(), init_hca_params.auxc_base,
            )?,
            altc_table: self.init_icm_table(
                config_regs, caps.altc_entry_sz(), init_hca_params.num_qps,
                1 << caps.log2_rsvd_qps(), init_hca_params.altc_base,
            )?,
            rdmarc_table: self.init_icm_table(
                config_regs, caps.rdmarc_entry_sz() << init_hca_params.rdmarc_shift,
                init_hca_params.num_qps, 1 << caps.log2_rsvd_qps(),
                init_hca_params.rdmarc_base,
            )?,
            rdmarc_base: init_hca_params.rdmarc_base,
            rdmarc_shift: init_hca_params.rdmarc_shift,
        };
        let cq_table = CqTable {
            table: self.init_icm_table(
                config_regs, caps.cqc_entry_sz(), init_hca_params.num_cqs,
                1 << caps.log2_rsvd_cqs(), init_hca_params.cqc_base,
            )?,
            cmpt_table: cq_cmpt_table,
        };
        let srq_table = SrqTable {
            table: self.init_icm_table(
                config_regs, caps.srq_entry_sz(), init_hca_params.num_srqs,
                1 << caps.log2_rsvd_srqs(), init_hca_params.srqc_base,
            )?,
            cmpt_table: srq_cmpt_table,
        };
        let mcg_table = self.init_icm_table(
            config_regs, get_mgm_entry_size().try_into().unwrap(),
            init_hca_params.num_mgms + init_hca_params.num_amgms,
            init_hca_params.num_mgms + init_hca_params.num_amgms, init_hca_params.mc_base,
        )?;
        trace!("ICM tables mapped successfully");
        Ok(MappedIcmTables {
            cq_table: Some(cq_table),
            qp_table: Some(qp_table),
            eq_table: Some(eq_table),
            srq_table: Some(srq_table),
            mr_table: Some(mr_table),
            mcg_table: Some(mcg_table),
        })
    }
    
    fn init_icm_table(
        &self, config_regs: &mut MappedPages, obj_size: u16, obj_num: usize,
        reserved: usize, virt: u64,
    ) -> Result<IcmTable, &'static str> {
        // We allocate in as big chunks as we can,
        // up to a maximum of 256 KB per chunk.
        const TABLE_CHUNK_SIZE: usize = 1 << 18;

        let table_size = obj_size as usize * obj_num;
        let obj_per_chunk = TABLE_CHUNK_SIZE / obj_size as usize;
        let icm_num = (obj_num + obj_per_chunk - 1) / obj_per_chunk;
        let mut icm = Vec::new();
        // map the reserved entries
        let mut idx = 0;
        while idx * TABLE_CHUNK_SIZE < reserved * obj_size as usize {
            let mut chunk_size = TABLE_CHUNK_SIZE;
            // TODO: does this make sense?
            if (idx + 1) * chunk_size > table_size {
                chunk_size = (table_size - idx * TABLE_CHUNK_SIZE).next_multiple_of(PAGE_SIZE);
            }
            let mut num_pages: u32 = (chunk_size / PAGE_SIZE).try_into().unwrap();
            if num_pages == 0 {
                num_pages = 1;
                chunk_size = num_pages as usize * PAGE_SIZE;
            }
            icm.push(MappedIcm::new(
                config_regs, chunk_size, num_pages,
                virt + (idx * TABLE_CHUNK_SIZE) as u64,
            )?);

            idx += 1;
        }
        Ok(IcmTable {
            virt, obj_num, obj_size, icm_num, icm,
        })
    }
    
}

impl Drop for MappedIcmAuxiliaryArea {
    fn drop(&mut self) {
        panic!("please unmap instead of dropping")
    }
}

struct IcmTable {
    virt: u64,
    obj_num: usize,
    obj_size: u16,
    /// the available number of Icms
    icm_num: usize,
    /// must contain less than icm_num entries
    icm: Vec<MappedIcm>,
}

impl IcmTable {
    fn unmap(mut self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        while let Some(icm) = self.icm.pop() {
            icm.unmap(config_regs)?;
        }
        Ok(())
    }
}

struct CqTable {
    table: IcmTable,
    cmpt_table: IcmTable,
}

struct QpTable {
    table: IcmTable,
    cmpt_table: IcmTable,
    auxc_table: IcmTable,
    altc_table: IcmTable,
    rdmarc_table: IcmTable,
    rdmarc_base: u64,
    rdmarc_shift: u8,
}

struct EqTable {
    table: IcmTable,
    cmpt_table: IcmTable,
}

struct SrqTable {
    table: IcmTable,
    cmpt_table: IcmTable,
}

struct MrTable {
    mtt_table: IcmTable,
    dmpt_table: IcmTable,
    // TODO
}

/// An ICM mapping.
struct MappedIcm {
    pages: MappedPages,
    physical: PhysicalAddress,
    card_virtual: u64,
    num_pages: u32,
}

impl MappedIcm {
    /// Allocate and map an ICM.
    // TODO: merge this with Firmware::map_area and MappedFirmwareArea::map_icm_aux?
    fn new(
        config_regs: &mut MappedPages, chunk_size: usize, num_pages: u32,
        card_virtual: u64,
    ) -> Result<Self, &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        let (pages, physical) = create_contiguous_mapping(chunk_size, DMA_FLAGS)?;
        let mut align = physical.value().trailing_zeros();
        if align > PAGE_SIZE.ilog2() {
            // TODO: fw.rs says it's 256KB?
            trace!("alignment greater than max size, defaulting to 4KB");
            align = PAGE_SIZE.ilog2();
        }
        let size = num_pages as usize * PAGE_SIZE;
        let mut count = size / (1 << align);
        if size % (1 << align) != 0 {
            count += 1;
        }
        // TODO: we can batch as many vpm entries as fit in a mailbox (1 page)
        // rather than 1 chunk per mailbox, this will make bootup faster
        let (mut vpm_pages, vpm_physical) = create_contiguous_mapping(size_of::<VirtualPhysicalMapping>(), DMA_FLAGS)?;
        let vpm: &mut VirtualPhysicalMapping = vpm_pages.as_type_mut(0)?;
        let mut phys_pointer = physical;
        let mut virt_pointer = card_virtual;
        for _ in 0..count {
            vpm.physical_address.set(phys_pointer.value().try_into().unwrap());
            vpm.virtual_address.set(virt_pointer);
            cmd.execute_command(
                Opcode::MapIcm, vpm_physical.value() as u64, 1, 0,
            )?;
            phys_pointer += 1 << align;
            virt_pointer += 1 << align;
        }
        Ok(Self { pages, physical, card_virtual, num_pages, })
    }

    /// Unmaps the area from the card.
    pub(super) fn unmap(self, config_regs: &mut MappedPages) -> Result<(), &'static str> {
        let mut cmd = CommandMailBox::new(config_regs)?;
        cmd.execute_command(
            Opcode::UnmapIcm, self.card_virtual, self.num_pages, 0,
        )?;
        core::mem::forget(self); // don't run the drop handler in this case
        Ok(())
    }
}

impl Drop for MappedIcm {
    fn drop(&mut self) {
        panic!("please unmap instead of dropping")
    }
}

pub(super) struct MappedIcmTables {
    cq_table: Option<CqTable>,
    qp_table: Option<QpTable>,
    eq_table: Option<EqTable>,
    srq_table: Option<SrqTable>,
    mr_table: Option<MrTable>,
    mcg_table: Option<IcmTable>,
}

impl MappedIcmTables {
    /// Unmaps the area from the card.
    pub(super) fn unmap(
        mut self, config_regs: &mut MappedPages,
    ) -> Result<(), &'static str> {
        trace!("unmapping ICM tables...");
        if let Some(eq_table) = self.eq_table.take() {
            eq_table.table.unmap(config_regs)?;
            eq_table.cmpt_table.unmap(config_regs)?;
        }
        if let Some(cq_table) = self.cq_table.take() {
            cq_table.table.unmap(config_regs)?;
            cq_table.cmpt_table.unmap(config_regs)?;
        }
        if let Some(qp_table) = self.qp_table.take() {
            qp_table.table.unmap(config_regs)?;
            qp_table.rdmarc_table.unmap(config_regs)?;
            qp_table.altc_table.unmap(config_regs)?;
            qp_table.auxc_table.unmap(config_regs)?;
            qp_table.cmpt_table.unmap(config_regs)?;
        }
        if let Some(mr_table) = self.mr_table.take() {
            mr_table.dmpt_table.unmap(config_regs)?;
            mr_table.mtt_table.unmap(config_regs)?;
        }
        if let Some(mcg_table) = self.mcg_table.take() {
            mcg_table.unmap(config_regs)?;
        }
        if let Some(srq_table) = self.srq_table.take() {
            srq_table.table.unmap(config_regs)?;
            srq_table.cmpt_table.unmap(config_regs)?;
        }
        trace!("successfully unmapped ICM tables");
        Ok(())
    }
}
