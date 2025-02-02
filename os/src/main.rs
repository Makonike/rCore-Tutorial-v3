#![no_std]
#![no_main]
#![feature(panic_info_message)]

use log::*;

#[macro_use]
mod console;
mod lang_items;
mod logging;
mod sbi;

use core::arch::global_asm;
global_asm!(include_str!("entry.asm"));

// fn main() {
//     println!("Hello, world!");
// }

#[no_mangle]
pub fn rust_main() -> ! {
    clear_bss();
    logging::init();
    println!("[RCORE] Hello, rCore!");
    trace!("Hello, rCore!");
    debug!("Hello, rCore!");
    info!("Hello, rCore!");
    warn!("Hello, rCore!");
    error!("Hello, rCore!");
    panic!("shutdown machine");
}

// clear bss segment data
fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| {
        unsafe { (a as *mut u8).write_volatile(0) };
    })
}
