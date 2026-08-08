#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi_services::{init, println};

#[entry]
fn efi_main() -> Status {
    println!("PCMS Core Loader v0.1");

    loop {}
}