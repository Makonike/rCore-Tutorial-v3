#![no_std]
#![no_main]

use core::arch::global_asm;
mod lang_items;
// fn main() {
// println!("Hello, world!");
// }
global_asm!(include_str!("entry.asm")); // 将汇编代码转为字符串，通过宏嵌入到代码中
