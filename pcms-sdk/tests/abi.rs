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

use core::mem::{align_of, size_of};

use pcms_sdk::*;

#[test]
fn medical_packet_layout_is_stable() {
    assert_eq!(size_of::<BufferHandle>(), 8);

    assert_eq!(size_of::<MedicalPacket>(), 32);
    assert_eq!(align_of::<MedicalPacket>(), 8);

    assert_eq!(size_of::<PcmsPluginInterface>(), 48);
}

#[test]
fn command_values_are_stable() {
    assert_eq!(
        ModuleCommand::ProcessDicomFrame.as_raw(),
        1
    );

    assert_eq!(
        ModuleCommand::EncryptStreamPacket.as_raw(),
        2
    );

    assert_eq!(
        ModuleCommand::AnalyzeFheBuffer.as_raw(),
        3
    );

    assert_eq!(
        ModuleCommand::NetworkTransmit.as_raw(),
        4
    );

    assert_eq!(
        ModuleCommand::from_raw(99),
        None
    );
}

#[test]
fn packet_validation_is_bounded() {
    let packet = MedicalPacket {
        buffer: BufferHandle {
            slot: 1,
            generation: 7,
        },

        len: 4096,

        timestamp: 1,

        command:
            ModuleCommand::ProcessDicomFrame
                .as_raw(),

        flags: 0,
    };

    assert_eq!(
        validate_packet(&packet),
        Ok(ModuleCommand::ProcessDicomFrame)
    );
}