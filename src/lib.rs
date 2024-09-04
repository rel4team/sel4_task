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
#![feature(alloc_error_handler)]
#![feature(asm_const)]

mod deps;
pub mod heap;
mod scheduler;
mod structures;
pub mod sync;
pub mod tcb;
mod tcb_queue;
mod thread_state;

pub use scheduler::*;
pub use structures::*;
pub use sync::UPSafeCell;
pub use tcb::*;
pub use tcb_queue::*;
pub use thread_state::*;

extern crate alloc;

#[cfg(test)]
mod tests {
    use alloc::{sync::Arc, vec, vec::Vec};
    use core::{arch::global_asm, intrinsics::size_of};
    use lazy_static::lazy_static;
    use riscv::register::{stvec, utvec::TrapMode};
    use sel4_common::{
        arch::{shutdown, vm_rights_t, ArchReg, ArchTCB},
        fault::{lookup_fault_t, seL4_Fault_t},
        println,
        sel4_config::{
            seL4_MsgMaxExtraCaps, seL4_MsgMaxLength, seL4_PageBits, tcbBuffer, tcbCTable,
            tcbCaller, tcbReply, wordBits, wordRadix, CONFIG_NUM_PRIORITIES,
            CONFIG_TIME_SLICE,
        },
        structures::{exception_t, seL4_IPCBuffer},
        utils::{convert_to_mut_type_ref, convert_to_type_ref},
        BIT, MASK,
    };
    use sel4_cspace::{
        arch::{cap_t, CapTag},
        interface::cte_t,
    };
    use sel4_vspace::pptr_t;
    global_asm!(include_str!("entry.asm"));

    use super::*;

    lazy_static! {
        static ref COMMON_MOCK_TCB: Arc<UPSafeCell<tcb_t>> = Arc::new(UPSafeCell::new(
            new_mock_tcb_with_state(ThreadState::ThreadStateRunning)
        ));
    }

    fn new_mock_tcb_with_state(state: ThreadState) -> tcb_t {
        let mut tcb = tcb_t {
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
        };

        tcb.init();
        tcb
    }

    #[test_case]
    pub fn tcb_create_test() {
        println!(">>>>>>>>>>>> Entering tcb_create_test...");

        let tcb = new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        assert_eq!(tcb.get_state(), ThreadState::ThreadStateRunning);
        assert_ne!(tcb.get_ptr(), 0);

        println!("Test tcb_create_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_and_get_cspace_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_and_get_cspace_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);

        // 确保至少两个slot可用
        assert_eq!(
            tcb.get_ptr() - tcb.get_cspace(0).get_ptr() >= 2 * size_of::<cte_t>(),
            true
        );

        (0..=1).for_each(|idx| {
            let slot = tcb.get_cspace_mut_ref(idx);
            slot.cap.set_asid_base(0x20);
            slot.cteMDBNode.set_first_badged(1);
        });

        (0..=1).for_each(|idx| {
            let slot = tcb.get_cspace(idx);
            assert_eq!(slot.cap.get_asid_base(), 0x20);
            assert_eq!(slot.cteMDBNode.get_first_badged(), 1);
        });

        println!("Test tcb_set_and_get_cspace_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_is_stopped_happy_case1_test() {
        println!(">>>>>>>>>>>> Entering tcb_is_stopped_happy_case1_test...");

        let stopped_states = vec![
            ThreadState::ThreadStateInactive,
            ThreadState::ThreadStateBlockedOnReceive,
            ThreadState::ThreadStateBlockedOnSend,
            ThreadState::ThreadStateBlockedOnReply,
            ThreadState::ThreadStateBlockedOnNotification,
        ];

        stopped_states.into_iter().for_each(|state| {
            let tcb = new_mock_tcb_with_state(state);
            assert_eq!(tcb.is_stopped(), true);
        });

        println!("Test tcb_is_stopped_happy_case1_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_is_stopped_happy_case2_test() {
        println!(">>>>>>>>>>>> Entering tcb_is_stopped_happy_case2_test...");

        let not_stopped_states = vec![
            ThreadState::ThreadStateRunning,
            ThreadState::ThreadStateRestart,
            ThreadState::ThreadStateIdleThreadState,
            ThreadState::ThreadStateExited,
        ];

        not_stopped_states.into_iter().for_each(|state| {
            let tcb = new_mock_tcb_with_state(state);
            assert_eq!(tcb.is_stopped(), false);
        });

        println!("Test tcb_is_stopped_happy_case2_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_is_runnable_happy_case1_test() {
        println!(">>>>>>>>>>>> Entering tcb_is_runnable_happy_case1_test...");

        let states = vec![
            ThreadState::ThreadStateRunning,
            ThreadState::ThreadStateRestart,
        ];

        states.into_iter().for_each(|state| {
            let tcb = new_mock_tcb_with_state(state);
            assert_eq!(tcb.is_runnable(), true);
        });

        println!("Test tcb_is_runnable_happy_case1_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_is_runnable_happy_case2_test() {
        println!(">>>>>>>>>>>> Entering tcb_is_runnable_happy_case2_test...");

        let states = vec![
            ThreadState::ThreadStateIdleThreadState,
            ThreadState::ThreadStateExited,
            ThreadState::ThreadStateInactive,
            ThreadState::ThreadStateBlockedOnReceive,
            ThreadState::ThreadStateBlockedOnSend,
            ThreadState::ThreadStateBlockedOnReply,
            ThreadState::ThreadStateBlockedOnNotification,
        ];

        states.into_iter().for_each(|state| {
            let tcb = new_mock_tcb_with_state(state);
            assert_eq!(tcb.is_runnable(), false);
        });

        println!("Test tcb_is_runnable_happy_case2_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_is_current_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_is_current_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        set_current_thread(tcb);
        assert_eq!(tcb.is_current(), true);

        println!("Test tcb_is_current_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_mcp_priorty_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_mcp_priorty_test...");

        let mut tcb = new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_mcps = vec![20, 40, 60];

        target_mcps.into_iter().for_each(|target_mcp| {
            tcb.set_mcp_priority(target_mcp);

            assert_eq!(tcb.tcbMCP, target_mcp);
        });

        println!("Test tcb_set_mcp_priorty_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_bind_notification_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_bind_notification_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_ntf: Vec<pptr_t> = vec![0x40000, 0x400000, 0x4000000];

        target_ntf.into_iter().for_each(|ntf_ptr| {
            tcb.bind_notification(ntf_ptr);

            assert_eq!(tcb.tcbBoundNotification, ntf_ptr);
        });

        println!("Test tcb_bind_notification_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_bind_and_unbind_notification_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_bind_and_unbind_notification_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.bind_notification(0x20);
        assert_ne!(tcb.tcbBoundNotification, 0);

        tcb.unbind_notification();
        assert_eq!(tcb.tcbBoundNotification, 0);

        println!("Test tcb_bind_and_unbind_notification_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_get_cpu_should_return_zero_when_is_not_smp() {
        println!(">>>>>>>>>>>> Entering tcb_get_cpu_should_return_zero_when_is_not_smp...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        assert_eq!(tcb.get_cpu(), 0);

        println!("Test tcb_get_cpu_should_return_zero_when_is_not_smp passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_thread_state_happy_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_thread_state_happy_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_states = vec![
            ThreadState::ThreadStateInactive,
            ThreadState::ThreadStateRunning,
            ThreadState::ThreadStateRestart,
            ThreadState::ThreadStateBlockedOnReceive,
            ThreadState::ThreadStateBlockedOnSend,
            ThreadState::ThreadStateBlockedOnReply,
            ThreadState::ThreadStateBlockedOnNotification,
            ThreadState::ThreadStateIdleThreadState,
            ThreadState::ThreadStateExited,
        ];

        target_states
            .into_iter()
            .enumerate()
            .for_each(|(idx, state)| {
                set_thread_state(tcb, state);

                assert_eq!(tcb.get_state() as usize, idx);
            });

        println!("Test tcb_set_thread_state_happy_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_sched_enqueue_happy_case1_test() {
        println!(">>>>>>>>>>>> Entering tcb_sched_enqueue_happy_case1_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        let target_priority = 200;
        tcb.domain = target_domain;
        tcb.set_priority(target_priority);

        tcb.sched_enqueue();

        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.tcbPriority, target_priority);
        assert_eq!(tcb.tcbSchedPrev, 0);
        assert_eq!(tcb.tcbSchedNext, 0);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        let idx = ready_queues_index(target_domain, target_priority);
        let queue = tcb.get_sched_queue(idx);
        assert_eq!(queue.head, tcb.get_ptr());
        assert_eq!(queue.tail, tcb.get_ptr());

        tcb.sched_dequeue();

        println!("Test tcb_sched_enqueue_happy_case1_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_sched_enqueue_happy_case2_test() {
        println!(">>>>>>>>>>>> Entering tcb_sched_enqueue_happy_case2_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        let target_priority = 200;
        tcb.domain = target_domain;
        tcb.set_priority(target_priority);

        tcb.sched_enqueue();

        let idx = ready_queues_index(target_domain, target_priority);
        let queue = tcb.get_sched_queue(idx);
        assert_eq!(queue.head, tcb.get_ptr());
        assert_eq!(queue.tail, tcb.get_ptr());

        let new_tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        new_tcb.domain = target_domain;
        new_tcb.set_priority(target_priority);

        new_tcb.sched_enqueue();

        assert_eq!(new_tcb.tcbSchedPrev, tcb.get_ptr());
        assert_eq!(new_tcb.tcbSchedNext, 0);
        assert_eq!(tcb.tcbSchedNext, new_tcb.get_ptr());

        tcb.sched_dequeue();
        new_tcb.sched_dequeue();

        println!("Test tcb_sched_enqueue_happy_case2_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_sched_dequeue_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_sched_dequeue_happy_case_test...");

        let mut tcbs = vec![
            new_mock_tcb_with_state(ThreadState::ThreadStateRunning),
            new_mock_tcb_with_state(ThreadState::ThreadStateRunning),
            new_mock_tcb_with_state(ThreadState::ThreadStateRunning),
        ];

        let target_domain = 0;
        let target_priority = 200;

        let queue = tcbs[0].get_sched_queue(ready_queues_index(target_domain, target_priority));
        assert!(queue.empty());

        tcbs.iter_mut().for_each(|tcb| {
            tcb.domain = target_domain;
            tcb.set_priority(target_priority);
            tcb.sched_enqueue();
        });
        assert!(queue.empty() == false);

        tcbs.iter_mut().for_each(|tcb| {
            tcb.sched_dequeue();
        });
        assert!(queue.empty());

        println!("Test tcb_sched_dequeue_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_domain_is_runnable_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_domain_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        assert_eq!(tcb.tcbState.get_tcb_queued(), 0);
        tcb.set_domain(target_domain);
        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.is_runnable(), true);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        println!("Test tcb_set_domain_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_domain_is_current_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_domain_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        set_current_scheduler_action(SchedulerAction_ResumeCurrentThread);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 0);
        set_current_thread(tcb);
        assert_eq!(tcb.is_current(), true);

        tcb.set_domain(target_domain);
        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);
        assert_eq!(get_ks_scheduler_action(), SchedulerAction_ChooseNewThread);

        println!("Test tcb_set_domain_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_set_vm_root_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_set_vm_root_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let ret = tcb.set_vm_root();
        assert_eq!(ret.is_ok(), true);

        println!("Test tcb_set_vm_root_happy_case_test passed!<<<<<<<<<<<<\n");
    }
    #[test_case]
    pub fn tcb_switch_to_this_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_switch_to_this_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.sched_enqueue();
        assert_ne!(get_currenct_thread().get_ptr(), tcb.get_ptr());

        tcb.switch_to_this();
        assert_eq!(get_currenct_thread().get_ptr(), tcb.get_ptr());

        println!("Test tcb_switch_to_this_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_slot_should_return_error_when_cap_type_is_not_cnode_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_slot_should_return_error_when_cap_type_is_not_cnode_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
        ctable_slot.cap = cap_t::new_frame_cap(0, 0, 0, 0, 0, 0);
        let slot = tcb.lookup_slot(0);
        assert_eq!(slot.status, exception_t::EXCEPTION_LOOKUP_FAULT);
        assert_eq!(slot.slot as *const cte_t as usize, 0);

        println!("Test tcb_lookup_slot_should_return_error_when_cap_type_is_not_cnode_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_slot_should_return_valid_when_guard_size_gt_wordBits_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_slot_should_return_valid_when_guard_size_gt_wordBits_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let slot: &mut cte_t = &mut cte_t::default();
        let slot_ptr = slot.get_ptr();
        let guard_bits = wordBits + 1; // overflow to 1
        let radix_bits = 1;
        let level_bits = radix_bits + guard_bits;
        let cap_ptr = 0;

        assert_ne!(level_bits, wordBits);

        let capCnodeGuard = 0;
        assert_eq!(capCnodeGuard, 0);

        let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
        ctable_slot.cap = cap_t::new_cnode_cap(1, guard_bits, capCnodeGuard, slot_ptr);

        let slot = tcb.lookup_slot(cap_ptr);
        assert_eq!(slot.status, exception_t::EXCEPTION_NONE);

        println!("Test tcb_lookup_slot_should_return_valid_when_guard_size_gt_wordBits_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_slot_should_return_error_when_guard_not_eqs_test() {
        println!(
            ">>>>>>>>>>>> Entering tcb_lookup_slot_should_return_error_when_guard_not_eqs_test..."
        );

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let slot: &mut cte_t = &mut cte_t::default();
        let slot_ptr = slot.get_ptr();
        let guard_bits = wordBits - 1;
        let radix_bits = 1;
        let level_bits = radix_bits + guard_bits;
        let cap_ptr = 0;

        assert_eq!(level_bits, wordBits);
        assert!(guard_bits <= wordBits);

        let mut capCnodeGuard =
            (cap_ptr >> ((wordBits - guard_bits) & MASK!(wordRadix))) & MASK!(guard_bits);
        assert_eq!(capCnodeGuard, 0);
        capCnodeGuard += 1;

        let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
        ctable_slot.cap = cap_t::new_cnode_cap(1, guard_bits, capCnodeGuard, slot_ptr);
        let slot = tcb.lookup_slot(0);

        assert_eq!(slot.status, exception_t::EXCEPTION_LOOKUP_FAULT);

        println!("Test tcb_lookup_slot_should_return_error_when_guard_not_eqs_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_slot_should_return_error_when_level_bits_gt_wordBits_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_slot_should_return_error_when_level_bits_gt_wordBits_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
        let guard_bits = wordBits - 1;
        let cap_ptr = 0;
        let capCnodeGuard = (cap_ptr >> ((wordBits - guard_bits) & MASK!(0))) & MASK!(guard_bits);
        ctable_slot.cap = cap_t::new_cnode_cap(2, guard_bits, capCnodeGuard, 0);
        let slot = tcb.lookup_slot(0);
        assert_eq!(slot.status, exception_t::EXCEPTION_LOOKUP_FAULT);
        assert_eq!(slot.slot as *const cte_t as usize, 0);

        println!("Test tcb_lookup_slot_should_return_error_when_level_bits_gt_wordBits_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_slot_should_return_valid_when_level_bits_eqs_wordBits_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_slot_should_return_valid_when_level_bits_eqs_wordBits_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let slot: &mut cte_t = &mut cte_t::default();
        let slot_ptr = slot.get_ptr();
        let guard_bits = wordBits - 1;
        let radix_bits = 1;
        let level_bits = radix_bits + guard_bits;
        let cap_ptr = 0;

        assert_eq!(level_bits, wordBits);
        assert!(guard_bits <= wordBits);

        let capCnodeGuard =
            (cap_ptr >> ((wordBits - guard_bits) & MASK!(wordRadix))) & MASK!(guard_bits);
        assert_eq!(capCnodeGuard, 0);

        let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
        ctable_slot.cap = cap_t::new_cnode_cap(1, guard_bits, capCnodeGuard, slot_ptr);
        let slot = tcb.lookup_slot(0);

        assert_eq!(slot.status, exception_t::EXCEPTION_NONE);
        assert_eq!(slot.slot as *const cte_t as usize, slot_ptr);

        println!("Test tcb_lookup_slot_should_return_valid_when_level_bits_eqs_wordBits_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_setup_reply_master_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_setup_reply_master_happy_case_test...");

        let mut tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        // 改变 tcb指针
        if tcb.get_ptr() - tcb.get_cspace(tcbCTable).get_ptr() < 3 * size_of::<cte_t>() {
            let tcb_ptr = tcb.get_cspace(tcbCTable).get_ptr() + 3 * size_of::<cte_t>();
            tcb = unsafe { &mut *(tcb_ptr as *mut tcb_t) };
        }

        assert_eq!(
            tcb.get_ptr() - tcb.get_cspace(tcbCTable).get_ptr() >= 3 * size_of::<cte_t>(),
            true
        );
        tcb.get_cspace_mut_ref(tcbReply).cap = cap_t::new_null_cap();
        tcb.setup_reply_master();

        assert_eq!(
            tcb.get_cspace(tcbReply).cap.get_cap_type(),
            CapTag::CapReplyCap
        );
        assert_eq!(
            tcb.get_cspace(tcbReply).cap.get_type(),
            CapTag::CapReplyCap as usize
        );

        println!("Test tcb_setup_reply_master_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_suspend_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_suspend_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.sched_enqueue();
        assert_eq!(tcb.get_state(), ThreadState::ThreadStateRunning);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);
        tcb.suspend();
        assert_eq!(tcb.get_state(), ThreadState::ThreadStateInactive);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 0);

        println!("Test tcb_suspend_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_restart_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_restart_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateBlockedOnSend);
        assert_eq!(tcb.get_state(), ThreadState::ThreadStateBlockedOnSend);
        assert!(tcb.tcbState.get_tcb_queued() == 0);

        tcb.restart();

        assert_eq!(tcb.get_state(), ThreadState::ThreadStateRestart);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        println!("Test tcb_restart_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    // #[test_case]
    // pub fn tcb_setup_caller_cap_happy_case_test() {
    //     println!(">>>>>>>>>>>> Entering tcb_setup_caller_cap_happy_case_test...");

    //     let mut tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
    //     tcb.get_cspace_mut_ref(tcbCaller).cap = cap_t::new_null_cap();
    //     let _temps = [
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //     ];

    //     // 改变 tcb指针
    //     if tcb.get_ptr() - tcb.get_cspace(tcbCTable).get_ptr() < 4 * size_of::<cte_t>() {
    //         let tcb_ptr = tcb.get_cspace(tcbCTable).get_ptr() + 4 * size_of::<cte_t>();
    //         tcb = unsafe { &mut *(tcb_ptr as *mut tcb_t) };
    //     }

    //     tcb.get_cspace_mut_ref(tcbCaller).cap = cap_t::new_null_cap();

    //     let mut sender = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
    //     let _temps2 = [
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //         "hello, world",
    //     ];
    //     // 改变 tcb指针
    //     if sender.get_ptr() - sender.get_cspace(tcbCTable).get_ptr() < 4 * size_of::<cte_t>() {
    //         let tcb_ptr = sender.get_cspace(tcbCTable).get_ptr() + 4 * size_of::<cte_t>();
    //         sender = unsafe { &mut *(tcb_ptr as *mut tcb_t) };
    //     }

    //     sender.get_cspace_mut_ref(tcbReply).cap = cap_t::new_null_cap();
    //     assert_eq!(
    //         sender.get_cspace(tcbReply).cap.get_cap_type(),
    //         CapTag::CapNullCap
    //     );
    //     sender.setup_reply_master();
    //     assert_eq!(
    //         sender.get_cspace(tcbReply).cap.get_cap_type(),
    //         CapTag::CapReplyCap
    //     );
    //     tcb.setup_caller_cap(sender, true);

    //     assert_ne!(
    //         tcb.get_cspace(tcbCaller).cap.get_cap_type(),
    //         CapTag::CapNullCap
    //     );

    //     println!("Test tcb_setup_caller_cap_happy_case_test passed!<<<<<<<<<<<<\n");
    // }

    #[test_case]
    fn tcb_delete_caller_cap_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_delete_caller_cap_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.get_cspace_mut_ref(tcbCaller).cap = cap_t::new_reply_cap(0, 0, 0);
        tcb.delete_caller_cap();
        assert_eq!(
            tcb.get_cspace(tcbCaller).cap.get_cap_type(),
            CapTag::CapNullCap
        );

        println!("Test tcb_delete_caller_cap_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_ipc_buffer_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_ipc_buffer_happy_case_test...");

        let page_base: [u8; BIT!(seL4_PageBits)] = [0; BIT!(seL4_PageBits)]; // page
        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.tcbIPCBuffer = 0x20;
        let buffer_cap = cap_t::new_frame_cap(
            0,
            page_base.as_ptr() as usize,
            seL4_PageBits,
            vm_rights_t::VMReadWrite as usize,
            0,
            0,
        );

        let mock_buffer =
            convert_to_mut_type_ref::<seL4_IPCBuffer>(page_base.as_ptr() as usize + 0x20);
        mock_buffer.tag = 0x888;
        mock_buffer.msg = [0x200; seL4_MsgMaxLength];
        mock_buffer.userData = 0x400;
        mock_buffer.caps_or_badges = [0x600; seL4_MsgMaxExtraCaps];
        mock_buffer.receiveCNode = 0x800;
        mock_buffer.receiveIndex = 0x1000;
        mock_buffer.receiveDepth = 0x2000;

        tcb.get_cspace_mut_ref(tcbBuffer).cap = buffer_cap;

        let res = tcb.lookup_ipc_buffer(false);
        assert_eq!(res.is_some(), true);
        let res = res.unwrap();
        assert_eq!(res.tag, 0x888);
        assert_eq!(res.msg, [0x200; seL4_MsgMaxLength]);
        assert_eq!(res.userData, 0x400);
        assert_eq!(res.caps_or_badges, [0x600; seL4_MsgMaxExtraCaps]);
        assert_eq!(res.receiveCNode, 0x800);
        assert_eq!(res.receiveIndex, 0x1000);
        assert_eq!(res.receiveDepth, 0x2000);

        println!("Test tcb_lookup_ipc_buffer_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_extra_caps_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_extra_caps_happy_case_test...");

        let page_base: [u8; BIT!(seL4_PageBits)] = [0; BIT!(seL4_PageBits)]; // page
        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);

        // ipc buffer build
        {
            tcb.tcbIPCBuffer = 0x20;
            let buffer_cap = cap_t::new_frame_cap(
                0,
                page_base.as_ptr() as usize,
                seL4_PageBits,
                vm_rights_t::VMReadWrite as usize,
                0,
                0,
            );

            let mock_buffer =
                convert_to_mut_type_ref::<seL4_IPCBuffer>(page_base.as_ptr() as usize + 0x20);
            mock_buffer.tag = 0x888;
            mock_buffer.msg = [0x200; seL4_MsgMaxLength];
            mock_buffer.userData = 0x400;
            mock_buffer.caps_or_badges = [0; seL4_MsgMaxExtraCaps];
            mock_buffer.receiveCNode = 0x800;
            mock_buffer.receiveIndex = 0x1000;
            mock_buffer.receiveDepth = 0x2000;

            tcb.get_cspace_mut_ref(tcbBuffer).cap = buffer_cap;
        }

        // slot build
        let slot: &mut cte_t = &mut cte_t::default();
        {
            let slot_ptr = slot.get_ptr();
            let guard_bits = wordBits - 1;
            let radix_bits = 1;
            let level_bits = radix_bits + guard_bits;
            let cap_ptr = 0;

            assert_eq!(level_bits, wordBits);
            assert!(guard_bits <= wordBits);

            let capCnodeGuard =
                (cap_ptr >> ((wordBits - guard_bits) & MASK!(wordRadix))) & MASK!(guard_bits);
            assert_eq!(capCnodeGuard, 0);

            let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
            ctable_slot.cap = cap_t::new_cnode_cap(1, guard_bits, capCnodeGuard, slot_ptr);
        }

        // msg info build
        {
            tcb.tcbArch.set_register(ArchReg::MsgInfo, 1 << 7);
        }

        let res: &mut [pptr_t; 3] = &mut [0; seL4_MsgMaxExtraCaps];
        let result = tcb.lookup_extra_caps(res);
        assert_eq!(result.is_ok(), true);
        assert_eq!(res[0], slot.get_ptr());
        assert_eq!(res[1], 0);
        assert_eq!(res[2], 0);

        println!("Test tcb_lookup_extra_caps_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn tcb_lookup_extra_caps_with_buf_happy_case_test() {
        println!(">>>>>>>>>>>> Entering tcb_lookup_extra_caps_with_buf_happy_case_test...");

        let page_base: [u8; BIT!(seL4_PageBits)] = [0; BIT!(seL4_PageBits)]; // page
        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);

        // ipc buffer build

        tcb.tcbIPCBuffer = 0x20;
        let buffer_cap = cap_t::new_frame_cap(
            0,
            page_base.as_ptr() as usize,
            seL4_PageBits,
            vm_rights_t::VMReadWrite as usize,
            0,
            0,
        );

        let mock_buffer =
            convert_to_mut_type_ref::<seL4_IPCBuffer>(page_base.as_ptr() as usize + 0x20);
        mock_buffer.tag = 0x888;
        mock_buffer.msg = [0x200; seL4_MsgMaxLength];
        mock_buffer.userData = 0x400;
        mock_buffer.caps_or_badges = [0; seL4_MsgMaxExtraCaps];
        mock_buffer.receiveCNode = 0x800;
        mock_buffer.receiveIndex = 0x1000;
        mock_buffer.receiveDepth = 0x2000;

        tcb.get_cspace_mut_ref(tcbBuffer).cap = buffer_cap;

        // slot build
        let slot: &mut cte_t = &mut cte_t::default();
        {
            let slot_ptr = slot.get_ptr();
            let guard_bits = wordBits - 1;
            let radix_bits = 1;
            let level_bits = radix_bits + guard_bits;
            let cap_ptr = 0;

            assert_eq!(level_bits, wordBits);
            assert!(guard_bits <= wordBits);

            let capCnodeGuard =
                (cap_ptr >> ((wordBits - guard_bits) & MASK!(wordRadix))) & MASK!(guard_bits);
            assert_eq!(capCnodeGuard, 0);

            let ctable_slot = tcb.get_cspace_mut_ref(tcbCTable);
            ctable_slot.cap = cap_t::new_cnode_cap(1, guard_bits, capCnodeGuard, slot_ptr);
        }

        // msg info build
        {
            tcb.tcbArch.set_register(ArchReg::MsgInfo, 1 << 7);
        }

        let res: &mut [pptr_t; 3] = &mut [0; seL4_MsgMaxExtraCaps];
        let mock_buffer = convert_to_type_ref::<seL4_IPCBuffer>(page_base.as_ptr() as usize + 0x20);
        let result = tcb.lookup_extra_caps_with_buf(res, Some(mock_buffer));
        assert_eq!(result.is_ok(), true);

        assert_eq!(res[0], slot.get_ptr());
        assert_eq!(res[1], 0);
        assert_eq!(res[2], 0);

        println!("Test tcb_lookup_extra_caps_with_buf_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[no_mangle]
    pub fn kernel_stack_alloc() {}
    #[no_mangle]
    fn ksIdleThreadTCB() {}

    #[test_case]
    pub fn scheduler_get_idle_thread_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_get_idle_thread_happy_case_test...");

        let mock_idle_thread = new_mock_tcb_with_state(ThreadState::ThreadStateIdleThreadState);
        unsafe { ksIdleThread = mock_idle_thread.get_ptr() as usize }
        let idle_thread = get_idle_thread();
        assert_eq!(
            idle_thread.get_state(),
            ThreadState::ThreadStateIdleThreadState
        );
        assert_eq!(idle_thread.get_ptr() as usize, unsafe { ksIdleThread });

        println!("Test scheduler_get_idle_thread_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_set_and_get_ks_scheduler_action_happy_case_test() {
        println!(
            ">>>>>>>>>>>> Entering scheduler_set_and_get_ks_scheduler_action_happy_case_test..."
        );

        let action = SchedulerAction_ChooseNewThread;
        set_current_scheduler_action(action);
        assert_eq!(get_ks_scheduler_action(), action);

        println!(
            "Test scheduler_set_and_get_ks_scheduler_action_happy_case_test passed!<<<<<<<<<<<<\n"
        );
    }

    #[test_case]
    pub fn scheduler_set_and_get_current_thread_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_set_and_get_current_thread_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        set_current_thread(tcb);
        assert_eq!(get_currenct_thread().get_ptr(), tcb.get_ptr());

        println!("Test scheduler_set_and_get_current_thread_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_get_currenct_thread_unsafe_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_get_currenct_thread_unsafe_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        set_current_thread(tcb);
        assert_eq!(get_currenct_thread_unsafe().get_ptr(), tcb.get_ptr());

        println!("Test scheduler_get_currenct_thread_unsafe_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_set_and_get_current_domain_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_set_and_get_current_domain_happy_case_test...");

        let domain = 0x20;
        unsafe { ksCurDomain = domain }
        assert_eq!(get_current_domain(), domain);

        println!("Test scheduler_set_and_get_current_domain_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_ready_queues_index_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_ready_queues_index_happy_case_test...");

        let domain = 0x20;
        let priority = 0x40;
        let idx = ready_queues_index(domain, priority);
        assert_eq!(idx, domain * CONFIG_NUM_PRIORITIES + priority);

        println!("Test scheduler_ready_queues_index_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_possible_switch_to_domain_not_equals_test() {
        println!(">>>>>>>>>>>> Entering scheduler_possible_switch_to_domain_equals_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        unsafe { ksCurDomain = target_domain + 1 };
        let target_priority = 201;
        tcb.domain = target_domain;
        tcb.set_priority(target_priority);

        possible_switch_to(tcb);
        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.tcbPriority, target_priority);
        assert_eq!(tcb.tcbSchedPrev, 0);
        assert_eq!(tcb.tcbSchedNext, 0);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);
        println!("Test scheduler_possible_switch_to_domain_equals_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_possible_switch_to_action_is_choose_new_test() {
        println!(">>>>>>>>>>>> Entering scheduler_possible_switch_to_action_is_choose_new_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        unsafe { ksCurDomain = target_domain };
        let target_priority = 202;
        tcb.domain = target_domain;
        tcb.set_priority(target_priority);

        set_current_scheduler_action(SchedulerAction_ChooseNewThread);
        possible_switch_to(tcb);
        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.tcbPriority, target_priority);
        assert_eq!(tcb.tcbSchedPrev, 0);
        assert_eq!(tcb.tcbSchedNext, 0);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);
        assert_eq!(get_ks_scheduler_action(), SchedulerAction_ChooseNewThread);

        println!(
            "Test scheduler_possible_switch_to_action_is_choose_new_test passed!<<<<<<<<<<<<\n"
        );
    }

    #[test_case]
    pub fn scheduler_possible_switch_to_action_is_resume_test() {
        println!(">>>>>>>>>>>> Entering scheduler_possible_switch_to_action_is_resume_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        let target_domain = 0;
        unsafe { ksCurDomain = target_domain };
        let target_priority = 203;
        tcb.domain = target_domain;
        tcb.set_priority(target_priority);

        set_current_scheduler_action(SchedulerAction_ResumeCurrentThread);
        possible_switch_to(tcb);
        assert_eq!(tcb.domain, target_domain);
        assert_eq!(tcb.tcbPriority, target_priority);
        assert_eq!(tcb.tcbSchedPrev, 0);
        assert_eq!(tcb.tcbSchedNext, 0);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);
        assert_eq!(get_ks_scheduler_action(), tcb.get_ptr());

        println!("Test scheduler_possible_switch_to_action_is_resume_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_timerTick_happy_case_test1() {
        println!(">>>>>>>>>>>> Entering scheduler_timerTick_happy_case_test1...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.tcbTimeSlice = CONFIG_TIME_SLICE;
        set_current_thread(tcb);
        timerTick();
        assert_eq!(tcb.tcbTimeSlice, CONFIG_TIME_SLICE - 1);

        println!("Test scheduler_timerTick_happy_case_test1 passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_timerTick_happy_case_test2() {
        println!(">>>>>>>>>>>> Entering scheduler_timerTick_happy_case_test2...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.tcbTimeSlice = 1;
        set_current_thread(tcb);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 0);

        timerTick();
        assert_eq!(tcb.tcbTimeSlice, CONFIG_TIME_SLICE);
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        println!("Test scheduler_timerTick_happy_case_test2 passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_activateThread_happy_case_test() {
        println!(">>>>>>>>>>>> Entering scheduler_activateThread_happy_case_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRestart);
        tcb.tcbArch.set_register(ArchReg::FaultIP, 10);
        set_current_thread(tcb);
        activateThread();
        assert_eq!(tcb.get_state(), ThreadState::ThreadStateRunning);
        assert_eq!(tcb.tcbArch.get_register(ArchReg::NextIP), 10);

        println!("Test scheduler_activateThread_happy_case_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_schedule_action_is_resume_test() {
        println!(">>>>>>>>>>>> Entering scheduler_schedule_action_is_resume_test...");

        set_current_scheduler_action(SchedulerAction_ResumeCurrentThread);
        schedule();
        assert_eq!(
            get_ks_scheduler_action(),
            SchedulerAction_ResumeCurrentThread
        );

        println!("Test scheduler_schedule_action_is_resume_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_schedule_action_is_not_resume_test() {
        println!(">>>>>>>>>>>> Entering scheduler_schedule_action_is_not_resume_test...");

        set_current_scheduler_action(SchedulerAction_ChooseNewThread);
        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        set_current_thread(tcb);
        unsafe { ksDomainTime = 1 }
        schedule();
        assert_eq!(
            get_ks_scheduler_action(),
            SchedulerAction_ResumeCurrentThread
        );

        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        println!("Test scheduler_schedule_action_is_not_resume_test passed!<<<<<<<<<<<<\n");
    }

    #[test_case]
    pub fn scheduler_schedule_action_is_tcb_ptr_test() {
        println!(">>>>>>>>>>>> Entering scheduler_schedule_action_is_tcb_ptr_test...");

        let tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        tcb.tcbPriority = 3;
        let c_tcb = &mut new_mock_tcb_with_state(ThreadState::ThreadStateRunning);
        c_tcb.tcbPriority = 3;
        set_current_thread(c_tcb); // current tcb
        set_current_scheduler_action(tcb.get_ptr()); // sched_tcb

        unsafe { ksDomainTime = 1 }
        schedule();
        assert_eq!(tcb.tcbState.get_tcb_queued(), 1);

        println!("Test scheduler_schedule_action_is_tcb_ptr_test passed!<<<<<<<<<<<<\n");
    }

    pub fn test_runner(tests: &[&dyn Fn()]) {
        println!("Running {} tests\n", tests.len());
        for test in tests {
            test();
        }
        println!("All Test Cases(count: {}) passed!", tests.len());
        shutdown();
    }

    #[panic_handler]
    fn panic(info: &core::panic::PanicInfo) -> ! {
        println!("{}", info);
        shutdown()
    }

    #[no_mangle]
    pub fn finaliseCap() {}
    #[no_mangle]
    pub fn post_cap_deletion() {}

    #[alloc_error_handler]
    pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
        panic!("Heap allocation error, layout = {:?}", layout);
    }

    #[no_mangle]
    pub fn call_test_main() {
        extern "C" {
            fn trap_entry();
        }
        unsafe {
            stvec::write(trap_entry as usize, TrapMode::Direct);
        }
        heap::init_heap();
        crate::test_main();
    }
    #[no_mangle]
    pub fn c_handle_syscall() {
        unsafe {
            core::arch::asm!("sret");
        }
    }
}
