use core::panic::PanicInfo;

#[panic_handler]
fn panic(_into: &PanicInfo) -> ! {
    loop {}
}
