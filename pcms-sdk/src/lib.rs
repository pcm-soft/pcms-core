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
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

//! PCMS SDK.
//!
//! The SDK defines a small, versioned, `no_std`, zero-allocation ABI for
//! trusted realtime plugins.
//!
//! ## Trusted-kernel execution
//!
//! Plugins using this ABI execute in the same privilege/address space as the
//! kernel. On x86 this can be ring 0; on AArch64 typically EL1; on RISC-V
//! typically S-mode or M-mode, depending on the kernel architecture.
//!
//! This minimizes transition and copy overhead, but it is NOT an isolation
//! boundary. A faulty or malicious plugin can corrupt kernel memory.

use core::{ffi::c_void, mem::size_of};

/// Current PCMS SDK ABI version.
pub const PCMS_SDK_ABI_VERSION: u32 = 1;

/// ABI magic: ASCII "PCMS".
pub const PCMS_ABI_MAGIC: u32 = u32::from_le_bytes(*b"PCMS");

/// Maximum packet payload accepted by the SDK contract.
pub const PCMS_MAX_PACKET_LEN: u32 = 64 * 1024 * 1024;

/// Plugin may read mapped buffers.
pub const CAP_BUFFER_READ: u64 = 1 << 0;

/// Plugin may write mapped buffers.
pub const CAP_BUFFER_WRITE: u64 = 1 << 1;

/// Plugin may emit bounded kernel events.
pub const CAP_EMIT_EVENT: u64 = 1 << 2;

/// Plugin may request ownership transfer.
pub const CAP_TRANSFER_OWNERSHIP: u64 = 1 << 3;

/// Module command.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleCommand {
    /// Process one DICOM/CT/MR frame.
    ProcessDicomFrame = 1,

    /// Encrypt one bounded stream packet.
    EncryptStreamPacket = 2,

    /// Analyze a homomorphically encrypted buffer.
    AnalyzeFheBuffer = 3,

    /// Prepare data for network transmission.
    NetworkTransmit = 4,
}

impl ModuleCommand {
    /// Convert a raw ABI value into a known command.
    #[inline]
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::ProcessDicomFrame),
            2 => Some(Self::EncryptStreamPacket),
            3 => Some(Self::AnalyzeFheBuffer),
            4 => Some(Self::NetworkTransmit),
            _ => None,
        }
    }

    /// Return the stable numeric ABI representation.
    #[inline]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }
}

/// Opaque reference to a kernel-owned memory allocation.
///
/// `slot` identifies an allocator slot.
/// `generation` prevents stale-handle reuse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferHandle {
    /// Kernel-assigned slot index.
    pub slot: u32,

    /// Generation associated with this allocation.
    pub generation: u32,
}

impl BufferHandle {
    /// Invalid handle.
    pub const INVALID: Self = Self {
        slot: u32::MAX,
        generation: 0,
    };

    /// Check whether the handle is invalid.
    #[inline]
    pub const fn is_invalid(self) -> bool {
        self.slot == u32::MAX
    }
}

/// Fixed-size packet descriptor crossing the ABI.
///
/// The descriptor itself is exactly 32 bytes.
///
/// It deliberately does NOT contain an arbitrary data pointer.
/// Data is accessed through the kernel-validated [`BufferHandle`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MedicalPacket {
    /// Kernel-owned buffer identity.
    pub buffer: BufferHandle,

    /// Number of valid bytes in the buffer.
    pub len: u32,

    /// Kernel timestamp/cycle counter value.
    pub timestamp: u64,

    /// Command discriminant.
    pub command: u32,

    /// Application-defined flags controlled by the kernel.
    pub flags: u32,
}

const _: () = assert!(size_of::<MedicalPacket>() == 32);

/// Result returned through the ABI.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcmsStatus {
    /// Operation completed successfully.
    Ok = 0,

    /// Generic plugin failure.
    PluginError = 1,

    /// Invalid packet.
    InvalidPacket = 2,

    /// Invalid or stale buffer handle.
    InvalidHandle = 3,

    /// Requested range is outside the buffer.
    BoundsError = 4,

    /// Required capability was not granted.
    CapabilityDenied = 5,

    /// ABI version mismatch.
    AbiMismatch = 6,

    /// Plugin execution exceeded its budget.
    DeadlineExceeded = 7,

    /// Kernel context is unavailable.
    KernelUnavailable = 8,

    /// Invalid argument.
    InvalidArgument = 9,

    /// Unsupported command.
    UnsupportedCommand = 10,
}

impl PcmsStatus {
    /// Return the stable ABI representation.
    #[inline]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }
}

/// ABI-visible mapped buffer.
///
/// The pointer is valid only until the corresponding `buffer_unmap` call.
///
/// Plugins MUST NOT retain the pointer after unmapping or after returning
/// from the `execute` callback.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PcmsMappedBuffer {
    /// Pointer to the validated memory range.
    pub ptr: *mut u8,

    /// Number of accessible bytes.
    pub len: usize,

    /// Opaque kernel mapping token.
    pub token: u64,
}

impl PcmsMappedBuffer {
    /// Empty mapping.
    pub const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        len: 0,
        token: 0,
    };

    /// Convert mapping into an immutable slice.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the kernel returned a valid mapping and
    /// that the mapping remains alive for the duration of the returned borrow.
    #[inline]
    pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Convert mapping into a mutable slice.
    ///
    /// # Safety
    ///
    /// The caller must have write capability and exclusive access to the
    /// mapped memory.
    #[inline]
    pub unsafe fn as_mut_slice<'a>(&mut self) -> &'a mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

/// Execution context supplied by the kernel.
#[repr(C)]
pub struct PcmsExecutionContext {
    /// Pointer to the immutable host API table.
    pub host: *const PcmsHostApi,

    /// Kernel-owned opaque state.
    pub opaque: *mut c_void,
}

/// Host services exposed to trusted plugins.
///
/// All calls are synchronous and must be bounded by the kernel.
#[repr(C)]
pub struct PcmsHostApi {
    /// ABI magic.
    pub magic: u32,

    /// ABI version.
    pub abi_version: u32,

    /// Structure size.
    pub struct_size: u32,

    /// Reserved field. Must be zero.
    pub reserved: u32,

    /// Map a kernel buffer.
    pub buffer_map: unsafe extern "C" fn(
        ctx: *mut PcmsExecutionContext,
        handle: BufferHandle,
        offset: u32,
        len: u32,
        write: u32,
        out: *mut PcmsMappedBuffer,
    ) -> PcmsStatus,

    /// Unmap a previously mapped buffer.
    pub buffer_unmap: unsafe extern "C" fn(
        ctx: *mut PcmsExecutionContext,
        mapping: *const PcmsMappedBuffer,
    ) -> PcmsStatus,

    /// Emit a bounded kernel event.
    pub emit_event: unsafe extern "C" fn(
        ctx: *mut PcmsExecutionContext,
        event_id: u32,
        value: u64,
    ) -> PcmsStatus,
}

/// Plugin ABI interface.
///
/// The kernel loads one instance of this structure and invokes the callbacks
/// synchronously.
#[repr(C)]
pub struct PcmsPluginInterface {
    /// ABI magic.
    pub magic: u32,

    /// ABI version.
    pub abi_version: u32,

    /// Structure size.
    pub struct_size: u32,

    /// Reserved field. Must be zero.
    pub reserved: u32,

    /// Capability bitmap.
    pub capabilities: u64,

    /// Plugin initialization callback.
    pub init:
        unsafe extern "C" fn(ctx: *mut PcmsExecutionContext) -> PcmsStatus,

    /// Plugin execution callback.
    pub execute: unsafe extern "C" fn(
        ctx: *mut PcmsExecutionContext,
        packet: *mut MedicalPacket,
    ) -> PcmsStatus,

    /// Plugin destruction callback.
    pub destroy:
        unsafe extern "C" fn(ctx: *mut PcmsExecutionContext) -> PcmsStatus,
}

const _: () = assert!(size_of::<PcmsPluginInterface>() == 48);

/// Validate plugin metadata before registration.
///
/// This function does not call plugin code.
#[inline]
pub fn validate_plugin_interface(
    plugin: &PcmsPluginInterface,
) -> Result<(), PcmsStatus> {
    if plugin.magic != PCMS_ABI_MAGIC {
        return Err(PcmsStatus::AbiMismatch);
    }

    if plugin.abi_version != PCMS_SDK_ABI_VERSION {
        return Err(PcmsStatus::AbiMismatch);
    }

    if plugin.struct_size < size_of::<PcmsPluginInterface>() as u32 {
        return Err(PcmsStatus::AbiMismatch);
    }

    if plugin.reserved != 0 {
        return Err(PcmsStatus::InvalidArgument);
    }

    Ok(())
}

/// Validate a packet without dereferencing its buffer.
#[inline]
pub fn validate_packet(
    packet: &MedicalPacket,
) -> Result<ModuleCommand, PcmsStatus> {
    if packet.buffer.is_invalid() {
        return Err(PcmsStatus::InvalidHandle);
    }

    if packet.len > PCMS_MAX_PACKET_LEN {
        return Err(PcmsStatus::BoundsError);
    }

    ModuleCommand::from_raw(packet.command)
        .ok_or(PcmsStatus::UnsupportedCommand)
}

/// Map the complete packet payload.
///
/// No allocation and no copying are performed.
///
/// # Safety
///
/// `ctx` must be the execution context supplied by the kernel for the current
/// callback. The kernel remains responsible for validating the handle,
/// generation, range and plugin capability.
#[inline]
pub unsafe fn map_packet(
    ctx: *mut PcmsExecutionContext,
    packet: &MedicalPacket,
    write: bool,
) -> Result<PcmsMappedBuffer, PcmsStatus> {
    if ctx.is_null() {
        return Err(PcmsStatus::InvalidArgument);
    }

    if packet.buffer.is_invalid() {
        return Err(PcmsStatus::InvalidHandle);
    }

    let host = unsafe { (*ctx).host };

    if host.is_null() {
        return Err(PcmsStatus::KernelUnavailable);
    }

    let api = unsafe { &*host };

    if api.magic != PCMS_ABI_MAGIC
        || api.abi_version != PCMS_SDK_ABI_VERSION
    {
        return Err(PcmsStatus::AbiMismatch);
    }

    let mut mapping = PcmsMappedBuffer::EMPTY;

    let status = unsafe {
        (api.buffer_map)(
            ctx,
            packet.buffer,
            0,
            packet.len,
            if write { 1 } else { 0 },
            &mut mapping,
        )
    };

    if status != PcmsStatus::Ok {
        return Err(status);
    }

    if mapping.ptr.is_null() && mapping.len != 0 {
        return Err(PcmsStatus::KernelUnavailable);
    }

    if mapping.len < packet.len as usize {
        return Err(PcmsStatus::BoundsError);
    }

    Ok(mapping)
}

/// RAII mapping guard.
///
/// The guard does not allocate. When dropped it calls the kernel's
/// `buffer_unmap` callback.
pub struct MappedBuffer {
    ctx: *mut PcmsExecutionContext,
    mapping: PcmsMappedBuffer,
}

impl MappedBuffer {
    /// Map a packet payload.
    ///
    /// # Safety
    ///
    /// `ctx` and `packet` must have been supplied by the kernel for the
    /// current callback and remain valid until this object is dropped.
    #[inline]
    pub unsafe fn map(
        ctx: *mut PcmsExecutionContext,
        packet: &MedicalPacket,
        write: bool,
    ) -> Result<Self, PcmsStatus> {
        let mapping = unsafe { map_packet(ctx, packet, write)? };

        Ok(Self {
            ctx,
            mapping,
        })
    }

    /// Access mapped data as immutable bytes.
    ///
    /// # Safety
    ///
    /// The kernel must have granted read capability.
    #[inline]
    pub unsafe fn as_slice(&self) -> &[u8] {
        unsafe { self.mapping.as_slice() }
    }

    /// Access mapped data as mutable bytes.
    ///
    /// # Safety
    ///
    /// The kernel must have granted write capability.
    #[inline]
    pub unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { self.mapping.as_mut_slice() }
    }

    /// Return mapped length.
    #[inline]
    pub const fn len(&self) -> usize {
        self.mapping.len
    }

    /// Check whether mapping is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.mapping.len == 0
    }
}

impl Drop for MappedBuffer {
    fn drop(&mut self) {
        unsafe {
            if self.ctx.is_null() {
                return;
            }

            let host = (*self.ctx).host;

            if host.is_null() {
                return;
            }

            let _ = ((*host).buffer_unmap)(
                self.ctx,
                &self.mapping,
            );
        }
    }
}

/// Emit a bounded kernel event.
///
/// # Safety
///
/// `ctx` must be the context supplied by the kernel for the current callback.
#[inline]
pub unsafe fn emit_event(
    ctx: *mut PcmsExecutionContext,
    event_id: u32,
    value: u64,
) -> PcmsStatus {
    if ctx.is_null() {
        return PcmsStatus::InvalidArgument;
    }

    let host = unsafe { (*ctx).host };

    if host.is_null() {
        return PcmsStatus::KernelUnavailable;
    }

    let api = unsafe { &*host };

    unsafe {
        (api.emit_event)(
            ctx,
            event_id,
            value,
        )
    }
}

/// Return current SDK ABI version.
#[inline]
pub const fn sdk_version() -> u32 {
    PCMS_SDK_ABI_VERSION
}