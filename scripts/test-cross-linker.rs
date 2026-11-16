use std::{ffi::c_void, thread};

#[link(name = "synchronization")]
unsafe extern "system" {
    fn WaitOnAddress(
        address: *const c_void,
        compare: *const c_void,
        size: usize,
        timeout: u32,
    ) -> i32;
    fn WakeByAddressSingle(address: *const c_void);
    fn WakeByAddressAll(address: *const c_void);
}

fn main() {
    let value = 1_u32;
    let expected = 0_u32;
    let address = (&value as *const u32).cast();
    // SAFETY: Both buffers are valid u32 values for the call's duration. The
    // values differ, so WaitOnAddress returns without waiting or changing them.
    unsafe {
        assert_ne!(
            WaitOnAddress(
                address,
                (&expected as *const u32).cast(),
                size_of::<u32>(),
                1
            ),
            0
        );
        WakeByAddressSingle(address);
        WakeByAddressAll(address);
    }
    assert_eq!(thread::spawn(|| 42).join().unwrap(), 42);
}
