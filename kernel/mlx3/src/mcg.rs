const DEFAULT_MGM_LOG_ENTRY_SIZE: usize = 10;

pub(super) fn get_mgm_entry_size() -> u64 {
    // TODO: how do we actually choose this correctly?
    1 << DEFAULT_MGM_LOG_ENTRY_SIZE
}
