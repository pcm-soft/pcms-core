// Copyright 2026 Gleb Obitotsky
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![no_std]
#![no_main]

mod bus;
mod executor;
mod timer;

use core::ffi::c_void;
use core::panic::PanicInfo;
use core::ptr;

use bus::PcmsBus;
use pcms_sdk::{sdk_version, PCMS_ABI_MAGIC, PCMS_SDK_ABI_VERSION};

const BUS_SIZE: usize = 256;

type EfiStatus = usize;

type EfiHandle = *mut c_void;

#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
struct EfiSimpleTextOutputProtocol {
    reset: unsafe extern "efiapi" fn(
        this: *mut EfiSimpleTextOutputProtocol,
        extended_verification: u8,
    ) -> EfiStatus,
    output_string: unsafe extern "efiapi" fn(
        this: *mut EfiSimpleTextOutputProtocol,
        string: *const u16,
    ) -> EfiStatus,
}

#[repr(C)]
pub struct EfiSystemTable {
    hdr: EfiTableHeader,
    firmware_vendor: *const u16,
    firmware_revision: u32,
    console_in_handle: EfiHandle,
    con_in: *mut c_void,
    console_out_handle: EfiHandle,
    con_out: *mut EfiSimpleTextOutputProtocol,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DemoPacket {
    command: u32,
    payload: u64,
}

static mut BUS: Option<PcmsBus<DemoPacket, BUS_SIZE>> = None;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn efi_print(con_out: *mut EfiSimpleTextOutputProtocol, s: &[u16]) {
    if con_out.is_null() {
        return;
    }
    let _ = ((*con_out).output_string)(con_out, s.as_ptr());
}

unsafe fn efi_print_ascii(con_out: *mut EfiSimpleTextOutputProtocol, ascii: &str) {
    let mut buf = [0u16; 129];
    let bytes = ascii.as_bytes();
    let len = core::cmp::min(bytes.len(), 128);
    for i in 0..len {
        buf[i] = bytes[i] as u16;
    }
    buf[len] = 0;
    efi_print(con_out, &buf[..=len]);
}

unsafe fn efi_print_u64(con_out: *mut EfiSimpleTextOutputProtocol, mut n: u64) {
    if n == 0 {
        efi_print_ascii(con_out, "0");
        return;
    }

    let mut digits = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }

    let mut buf = [0u16; 21];
    for j in 0..i {
        buf[j] = digits[i - 1 - j] as u16;
    }
    buf[i] = 0;
    efi_print(con_out, &buf[..=i]);
}

unsafe fn efi_print_hex32(con_out: *mut EfiSimpleTextOutputProtocol, mut n: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u16; 11]; // "0x" + 8 digits + NUL
    buf[0] = b'0' as u16;
    buf[1] = b'x' as u16;
    for i in 0..8 {
        let shift = (7 - i) * 4;
        let nibble = ((n >> shift) & 0xF) as usize;
        buf[2 + i] = HEX[nibble] as u16;
    }
    buf[10] = 0;
    efi_print(con_out, &buf);
}

#[no_mangle]
pub extern "C" fn efi_main(
    _image_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
) -> EfiStatus {
    let con_out = if !system_table.is_null() {
        unsafe { (*system_table).con_out }
    } else {
        ptr::null_mut()
    };

    unsafe {
        efi_print_ascii(con_out, "\r\n");
        efi_print_ascii(con_out, "================================================\r\n");
        efi_print_ascii(con_out, "  PCMS Core \r\n");
        efi_print_ascii(con_out, "================================================\r\n");
        efi_print_ascii(con_out, "\r\n");
    }

    unsafe {
        efi_print_ascii(con_out, "[boot] loading PCMS SDK...\r\n");
        efi_print_ascii(con_out, "[boot]   ABI magic     = ");
        efi_print_hex32(con_out, PCMS_ABI_MAGIC);
        efi_print_ascii(con_out, " (\"PCMS\")\r\n");

        efi_print_ascii(con_out, "[boot]   ABI version   = ");
        efi_print_u64(con_out, PCMS_SDK_ABI_VERSION as u64);
        efi_print_ascii(con_out, "\r\n");

        efi_print_ascii(con_out, "[boot]   sdk_version() = ");
        efi_print_u64(con_out, sdk_version() as u64);
        efi_print_ascii(con_out, "\r\n");

        efi_print_ascii(con_out, "[boot] SDK ABI OK\r\n");
    }

    unsafe {
        efi_print_ascii(con_out, "[boot] initializing memory subsystem...\r\n");
    }

    let bus_capacity = BUS_SIZE;
    unsafe {
        BUS = Some(PcmsBus::new());
    }

    unsafe {
        efi_print_ascii(con_out, "[boot]   bus capacity   = ");
        efi_print_u64(con_out, bus_capacity as u64);
        efi_print_ascii(con_out, " slots\r\n");

        let approx_bytes = bus_capacity * 64;
        efi_print_ascii(con_out, "[boot]   approx footprint = ");
        efi_print_u64(con_out, approx_bytes as u64);
        efi_print_ascii(con_out, " bytes (slots)\r\n");

        efi_print_ascii(con_out, "[boot] memory subsystem ready\r\n");
    }

    unsafe {
        efi_print_ascii(con_out, "[boot] timer            = TSC / architectural counter\r\n");
        efi_print_ascii(con_out, "[boot] executor         = ready (trusted-plugin path)\r\n");
    }

    unsafe {
        efi_print_ascii(con_out, "\r\n");
        efi_print_ascii(con_out, "[boot] entering idle consumer loop\r\n");
        efi_print_ascii(con_out, "\r\n");
    }

    let mut heartbeat: u64 = 0;
    let mut last_print_cycles = timer::read_tsc_cycles();

    const HEARTBEAT_CYCLES: u64 = 2_500_000_000;

    loop {
        let packet = unsafe { BUS.as_ref().and_then(|bus| bus.pop().ok()) };

        match packet {
            Some(packet) => {
                process_packet(packet);
                unsafe {
                    efi_print_ascii(con_out, "[run ] processed packet  cmd=");
                    efi_print_u64(con_out, packet.command as u64);
                    efi_print_ascii(con_out, "  payload=");
                    efi_print_u64(con_out, packet.payload);
                    efi_print_ascii(con_out, "\r\n");
                }
            }
            None => {
                timer::spin_for_cycles(1_000);
            }
        }

        let now = timer::read_tsc_cycles();
        if now.wrapping_sub(last_print_cycles) >= HEARTBEAT_CYCLES {
            heartbeat = heartbeat.wrapping_add(1);
            last_print_cycles = now;

            unsafe {
                efi_print_ascii(con_out, "[run ] alive  heartbeat=");
                efi_print_u64(con_out, heartbeat);
                efi_print_ascii(con_out, "  tsc=");
                efi_print_u64(con_out, now);
                efi_print_ascii(con_out, "\r\n");
            }
        }
    }
}

#[inline(always)]
fn process_packet(packet: DemoPacket) {
    match packet.command {
        0 => {
            let _ = packet.payload;
        }
        _ => {
            // Unknown command.
        }
    }
}