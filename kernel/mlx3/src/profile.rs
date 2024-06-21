use memory::PAGE_SIZE;
use strum::EnumCount;
use strum_macros::{Display, EnumCount, FromRepr};

use crate::fw::InitHcaParameters;

use super::fw::Capabilities;

#[repr(usize)]
#[derive(Default, Display, EnumCount, FromRepr, Clone, Copy)]
enum ResourceType {
    #[default] QP, RDMARC, ALTC, AUXC, SRQ, CQ, EQ, DMPT, CMPT, MTT, MCG,
}


#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Resource {
    size: u64,
    start: u64,
    typ: ResourceType,
    num: u64,
}

impl Resource {
    fn lognum(&self) -> u32 {
        self.num.ilog2()
    }
}

const DEFAULT_NUM_QP: u64 = 1 << 17;
const DEFAULT_NUM_SRQ: u64 = 1 << 16;
const DEFAULT_RDMARC_PER_QP: u64 = 1 << 4;
const DEFAULT_NUM_CQ: u64 = 1 << 16;
const DEFAULT_NUM_MCG: u64 = 1 << 13;
const DEFAULT_NUM_MPT: u64 = 1 << 19;
const DEFAULT_NUM_MTT: u64 = 1 << 20; // based ON 1024 Ram Mem 1024 >> log_mtt_per_seg-1
const MAX_NUM_EQS: u64 = 1 << 9;

#[repr(usize)]
#[derive(EnumCount)]
enum CmptType {
    QP, SRQ, CQ, EQ,
}

/// Construct a profile.
/// 
/// It doesn't return the profile, though. You'll get [`InitHcaParameters`]
/// and `total_size` (the ICM size in bytes) instead.
pub(super) fn make_profile(caps: &Capabilities) -> Result<(InitHcaParameters, u64), &'static str> {
    let mut init_hca = InitHcaParameters::default();
    let mut total_size = 0;
    let log_mtt_per_seg = 3;

    // TODO: this temporarily produces invalid values,
    // but that's how the C driver does it
    let mut profiles: [Resource; ResourceType::COUNT] = Default::default();

    profiles[ResourceType::QP as usize].size = caps.qpc_entry_sz().into();
    profiles[ResourceType::RDMARC as usize].size = caps.rdmarc_entry_sz().into();
    profiles[ResourceType::ALTC as usize].size = caps.altc_entry_sz().into();
    profiles[ResourceType::AUXC as usize].size = caps.aux_entry_sz().into();
    profiles[ResourceType::SRQ as usize].size = caps.srq_entry_sz().into();
    profiles[ResourceType::CQ as usize].size = caps.cqc_entry_sz().into();
    profiles[ResourceType::EQ as usize].size = caps.eqc_entry_sz().into();
    profiles[ResourceType::DMPT as usize].size = caps.d_mpt_entry_sz().into();
    profiles[ResourceType::CMPT as usize].size = caps.c_mpt_entry_sz().into();
    profiles[ResourceType::MTT as usize].size = caps.mtt_entry_sz().into();
    profiles[ResourceType::MCG as usize].size = super::mcg::get_mgm_entry_size();

    profiles[ResourceType::QP as usize].num = DEFAULT_NUM_QP;
    profiles[ResourceType::RDMARC as usize].num = DEFAULT_NUM_QP * DEFAULT_RDMARC_PER_QP;
    profiles[ResourceType::ALTC as usize].num = DEFAULT_NUM_QP;
    profiles[ResourceType::AUXC as usize].num = DEFAULT_NUM_QP;
    profiles[ResourceType::SRQ as usize].num = DEFAULT_NUM_SRQ;
    profiles[ResourceType::CQ as usize].num = DEFAULT_NUM_CQ;
    profiles[ResourceType::EQ as usize].num = MAX_NUM_EQS;
    profiles[ResourceType::DMPT as usize].num = DEFAULT_NUM_MPT;
    profiles[ResourceType::CMPT as usize].num = (CmptType::COUNT << 24).try_into().unwrap();
    profiles[ResourceType::MTT as usize].num = DEFAULT_NUM_MTT * (1 << log_mtt_per_seg);
    profiles[ResourceType::MCG as usize].num = DEFAULT_NUM_MCG;

    for (idx, profile) in profiles.iter_mut().enumerate() {
        profile.typ = ResourceType::from_repr(idx).unwrap();
        profile.num = profile.num.checked_next_power_of_two().unwrap();
        profile.size *= profile.num;
        if profile.size < PAGE_SIZE.try_into().unwrap() {
            profile.size = PAGE_SIZE.try_into().unwrap();
        }
    }

    // Sort the resources in decreasing order of size. Since they all have sizes
    // that are powers of 2, we'll be able to keep resources aligned to their
    // size and pack them without gaps using the sorted order.
    profiles.sort_unstable_by_key(|p| p.size);
    profiles.reverse();

    for (idx, profile) in profiles.iter_mut().enumerate() {
        profile.start = total_size;
        total_size += profile.size;
        if total_size > caps.max_icm_sz() {
            return Err("total size > maximum ICM size");
        }
        if profile.size > 0 {
            trace!(
                " resource[{:02}] ({:>6}): 2^{:02} entries @ {:#010x} size {} KB",
                idx, profile.typ, profile.lognum(), profile.start, profile.size >> 10,
            );
        }
    }
    init_hca.rdmarc_shift = 0;
    for profile in profiles.iter() {
        match profile.typ {
            ResourceType::CMPT => init_hca.cmpt_base = profile.start,
            ResourceType::CQ => {
                init_hca.num_cqs = profile.num.try_into().unwrap();
                init_hca.cqc_base = profile.start;
                init_hca.log_num_cqs = profile.lognum().try_into().unwrap();
            },
            ResourceType::SRQ => {
                init_hca.num_srqs = profile.num.try_into().unwrap();
                init_hca.srqc_base = profile.start;
                init_hca.log_num_srqs = profile.lognum().try_into().unwrap();
            },
            ResourceType::QP => {
                init_hca.num_qps = profile.num.try_into().unwrap();
                init_hca.qpc_base = profile.start;
                init_hca.log_num_qps = profile.lognum().try_into().unwrap();
            },
            ResourceType::ALTC => init_hca.altc_base = profile.start,
            ResourceType::AUXC => init_hca.auxc_base = profile.start,
            ResourceType::MTT => {
                init_hca.num_mtts = profile.num.try_into().unwrap();
                init_hca.mtt_base = profile.start;
            },
            ResourceType::EQ => {
                init_hca.num_eqs = MAX_NUM_EQS.try_into().unwrap();
                init_hca.eqc_base = profile.start;
                init_hca.log_num_eqs = init_hca.num_eqs.ilog2().try_into().unwrap();
            },
            ResourceType::RDMARC => {
                // TODO: this should be possible without a loop
                while DEFAULT_NUM_QP << init_hca.rdmarc_shift < profile.num {
                    init_hca.max_qp_dest_rdma = 1 << init_hca.rdmarc_shift;
                    init_hca.rdmarc_base = profile.start;
                    init_hca.log_rd_per_qp = init_hca.rdmarc_shift;
                    init_hca.rdmarc_shift += 1;
                }
            },
            ResourceType::DMPT => {
                init_hca.num_mpts = profile.num.try_into().unwrap();
                init_hca.dmpt_base = profile.start;
                init_hca.log_mpt_sz = profile.lognum().try_into().unwrap();
            },
            ResourceType::MCG => {
                init_hca.mc_base = profile.start;
                init_hca.log_mc_entry_sz = super::mcg::get_mgm_entry_size().ilog2().try_into().unwrap();
                init_hca.log_mc_table_sz = profile.lognum().try_into().unwrap();
                init_hca.log_mc_hash_sz = (profile.lognum() - 1).try_into().unwrap();
                init_hca.num_mgms = (profile.num >> 1).try_into().unwrap();
                init_hca.num_amgms = (profile.num >> 1).try_into().unwrap();
            },
        }
    }
    trace!("Max ICM size: {} GB", caps.max_icm_sz() >> 30);
    trace!("ICM memory reserving {} GB", total_size >> 30);
    trace!("HCA Pages Required: {}", total_size >> 12);
    Ok((init_hca, total_size))
}
