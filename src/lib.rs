//! This crate provides task management for seL4, including the TCB, scheduler, and thread relevant structures.
//!
//!  See more details in ../doc.md

#![feature(core_intrinsics)]
#![no_std]
#![allow(internal_features)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![no_main]
#![feature(asm_const)]

mod deps;
mod scheduler;
mod structures;
pub mod tcb;
mod tcb_queue;
mod thread_state;

pub use scheduler::*;
pub use structures::*;
pub use tcb::*;
pub use tcb_queue::*;
pub use thread_state::*;

#[cfg(test)]
mod tests {
    use core::arch::global_asm;
    use riscv::register::{stvec, utvec::TrapMode};
    use sel4_common::{
        arch::{shutdown, ArchReg, ArchTCB},
        fault::{lookup_fault_t, seL4_Fault_t},
        println,
    };
    global_asm!(include_str!("entry.asm"));

    use super::*;

    fn new_mock_tcb_with_state(state: ThreadState) -> tcb_t {
        tcb_t {
            tcbEPPrev: 0,
            tcbEPNext: 0,
            tcbSchedPrev: 0,
            tcbSchedNext: 0,
            tcbIPCBuffer: 0,
            tcbFaultHandler: 0,
            tcbTimeSlice: 0,
            tcbPriority: 0,
            tcbMCP: 0,
            domain: 0,
            tcbLookupFailure: lookup_fault_t::new_root_invalid(),
            tcbFault: seL4_Fault_t::new_null_fault(),
            tcbBoundNotification: 0,
            tcbState: thread_state_t::state_new(0, 0, 0, 0, 0, 0, state as usize),
            tcbArch: ArchTCB::default(),
        }
    }

    pub fn test_runner(_tests: &[&dyn Fn()]) {
        // println!("Running {} tests\n", tests.len());
        // for test in tests {
        //     test();
        // }
        // println!("All Test Cases(count: {}) passed!", tests.len());
        println!("There is no test case in this module now! todo!");
        shutdown();
    }

    #[panic_handler]
    fn panic(info: &core::panic::PanicInfo) -> ! {
        println!("{}", info);
        shutdown()
    }

    #[no_mangle]
    pub fn call_test_main() {
        extern "C" {
            fn trap_entry();
        }
        unsafe {
            stvec::write(trap_entry as usize, TrapMode::Direct);
        }
        crate::test_main();
    }
    #[no_mangle]
    pub fn c_handle_syscall() {
        unsafe {
            core::arch::asm!("sret");
        }
    }
}
