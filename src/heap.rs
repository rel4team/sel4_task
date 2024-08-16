use buddy_system_allocator::LockedHeap;

const KERNEL_SPACE_SIZE: usize = 8192;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut HEAP_SPACE: [u8; KERNEL_SPACE_SIZE] = [0; KERNEL_SPACE_SIZE];

pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR
            .lock()
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_SPACE_SIZE);
    }
}
