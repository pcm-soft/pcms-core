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

//! PCMS plugin executor.
//!
//! The executor invokes trusted plugins through the PCMS SDK ABI.
//!
//! # Safety model
//!
//! Plugins executed by this executor are trusted kernel components.
//! They execute in the same privilege/address space as the kernel.
//!
//! This provides very low invocation overhead, but it is NOT a security
//! or fault-isolation boundary.
//!
//! A plugin can corrupt kernel memory or fail to return.

use core::ptr::NonNull;

use pcms_sdk::{
    validate_packet,
    validate_plugin_interface,
    MedicalPacket,
    PcmsExecutionContext,
    PcmsPluginInterface,
    PcmsStatus,
};

use crate::timer::read_tsc_cycles;

/// Maximum permitted execution time.
///
/// This value is a SOFTWARE accounting limit.
///
/// It does NOT provide hard preemption. A plugin which never returns from
/// `execute()` cannot be stopped by this executor alone.
///
/// At 3 GHz:
///
/// 9,000,000 cycles ~= 3 ms.
///
/// The actual relationship between cycles and wall-clock time depends on
/// hardware, frequency scaling and timer configuration.
pub const HARD_SLA_CYCLES_LIMIT: u64 = 9_000_000;

/// Result returned by the executor.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorStatus {
    /// Plugin execution completed successfully.
    Ok = 0,

    /// Plugin ABI is invalid.
    InvalidPlugin = 1,

    /// Packet is invalid.
    InvalidPacket = 2,

    /// Plugin itself returned an error.
    PluginError = 3,

    /// Software execution budget was exceeded.
    DeadlineExceeded = 4,

    /// Kernel execution context is invalid.
    InvalidContext = 5,
}

impl ExecutorStatus {
    /// Convert executor status into a PCMS SDK status.
    #[inline(always)]
    pub const fn as_sdk_status(self) -> PcmsStatus {
        match self {
            Self::Ok => PcmsStatus::Ok,
            Self::InvalidPlugin => PcmsStatus::AbiMismatch,
            Self::InvalidPacket => PcmsStatus::InvalidPacket,
            Self::PluginError => PcmsStatus::PluginError,
            Self::DeadlineExceeded => PcmsStatus::DeadlineExceeded,
            Self::InvalidContext => PcmsStatus::InvalidArgument,
        }
    }
}

/// Registered trusted plugin.
///
/// The plugin interface is borrowed rather than copied. This prevents
/// accidental ownership of ABI function tables.
pub struct KernelPlugin {
    interface: NonNull<PcmsPluginInterface>,
    context: NonNull<PcmsExecutionContext>,
}

impl KernelPlugin {
    /// Register a plugin interface.
    ///
    /// No plugin code is executed here.
    ///
    /// # Safety
    ///
    /// `interface` must point to a valid `PcmsPluginInterface` whose memory
    /// remains valid for the entire lifetime of the returned `KernelPlugin`.
    ///
    /// `context` must point to a valid kernel-owned execution context whose
    /// lifetime also exceeds the returned plugin registration.
    pub unsafe fn new(
        interface: *mut PcmsPluginInterface,
        context: *mut PcmsExecutionContext,
    ) -> Result<Self, ExecutorStatus> {
        if interface.is_null() || context.is_null() {
            return Err(ExecutorStatus::InvalidContext);
        }

        let interface_ref = unsafe { &*interface };

        validate_plugin_interface(interface_ref)
            .map_err(|_| ExecutorStatus::InvalidPlugin)?;

        Ok(Self {
            interface: unsafe { NonNull::new_unchecked(interface) },
            context: unsafe { NonNull::new_unchecked(context) },
        })
    }

    /// Return the underlying plugin interface.
    #[inline(always)]
    pub fn interface(&self) -> &PcmsPluginInterface {
        unsafe { self.interface.as_ref() }
    }

    /// Return the plugin execution context.
    #[inline(always)]
    pub fn context(&self) -> *mut PcmsExecutionContext {
        self.context.as_ptr()
    }

    /// Initialize the plugin.
    ///
    /// This should normally be called once during kernel initialization.
    ///
    /// # Safety
    ///
    /// The plugin must be trusted kernel code and the context must remain
    /// valid for the duration of the call.
    pub unsafe fn init(&self) -> PcmsStatus {
        let interface = unsafe { self.interface.as_ref() };

        unsafe {
            (interface.init)(self.context.as_ptr())
        }
    }

    /// Destroy the plugin.
    ///
    /// This should only be called when the kernel guarantees that no
    /// concurrent execution of the plugin is possible.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the plugin is no longer executing and that
    /// its context remains valid.
    pub unsafe fn destroy(&self) -> PcmsStatus {
        let interface = unsafe { self.interface.as_ref() };

        unsafe {
            (interface.destroy)(self.context.as_ptr())
        }
    }
}

/// Result of a plugin invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionResult {
    /// Status returned by the plugin.
    pub status: PcmsStatus,

    /// Number of timer cycles consumed by the call.
    pub cycles: u64,

    /// Whether the software SLA was exceeded.
    pub deadline_exceeded: bool,
}

/// Kernel executor.
///
/// There is intentionally no heap allocation, mutex or dynamic dispatch on
/// the execution path.
pub struct KernelExecutor {
    hard_limit_cycles: u64,
}

impl KernelExecutor {
    /// Construct an executor with the default hard software budget.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            hard_limit_cycles: HARD_SLA_CYCLES_LIMIT,
        }
    }

    /// Construct an executor with a custom cycle budget.
    ///
    /// The caller is responsible for selecting a value appropriate for the
    /// target CPU and workload.
    #[inline(always)]
    pub const fn with_limit(hard_limit_cycles: u64) -> Self {
        Self {
            hard_limit_cycles,
        }
    }

    /// Return configured software execution budget.
    #[inline(always)]
    pub const fn hard_limit_cycles(&self) -> u64 {
        self.hard_limit_cycles
    }

    /// Execute a trusted plugin.
    ///
    /// The hot path contains:
    ///
    /// 1. packet validation;
    /// 2. timestamp read;
    /// 3. direct C ABI function call;
    /// 4. timestamp read;
    /// 5. integer comparison.
    ///
    /// There is no heap allocation and no locking.
    ///
    /// # Safety
    ///
    /// The supplied plugin must have been registered from a valid
    /// `PcmsPluginInterface`. The packet must remain valid for the duration
    /// of the plugin call.
    ///
    /// Plugins execute with kernel privileges and therefore must be treated
    /// as trusted code.
    #[inline(never)]
    pub unsafe fn launch_in_kernel(
        &self,
        plugin: &KernelPlugin,
        packet: &mut MedicalPacket,
    ) -> ExecutionResult {
        if validate_packet(packet).is_err() {
            return ExecutionResult {
                status: PcmsStatus::InvalidPacket,
                cycles: 0,
                deadline_exceeded: false,
            };
        }

        let start = read_tsc_cycles();

        let status = unsafe {
            let interface = plugin.interface();

            (interface.execute)(
                plugin.context(),
                packet as *mut MedicalPacket,
            )
        };

        let end = read_tsc_cycles();

        let elapsed = end.wrapping_sub(start);

        let deadline_exceeded = elapsed > self.hard_limit_cycles;

        if deadline_exceeded {
            return ExecutionResult {
                status: PcmsStatus::DeadlineExceeded,
                cycles: elapsed,
                deadline_exceeded: true,
            };
        }

        ExecutionResult {
            status,
            cycles: elapsed,
            deadline_exceeded: false,
        }
    }
}

impl Default for KernelExecutor {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}