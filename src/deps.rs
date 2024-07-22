use sel4_common::{
    sel4_config::{seL4_TCBBits, CONFIG_MAX_NUM_NODES},
    BIT,
};
#[repr(align(2048))]
pub struct ksIdleThreadTCB_data {
    pub data: [[u8; CONFIG_MAX_NUM_NODES]; BIT!(seL4_TCBBits)],
}
#[no_mangle]
#[link_section = "._idle_thread"]
pub static mut ksIdleThreadTCB: ksIdleThreadTCB_data = ksIdleThreadTCB_data {
    data: [[0; CONFIG_MAX_NUM_NODES]; BIT!(seL4_TCBBits)],
};
extern "C" {
    #[cfg(feature = "ENABLE_SMP")]
    pub fn doMaskReschedule(mask: usize);
}
