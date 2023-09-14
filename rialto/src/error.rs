// Copyright 2022, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This module contains the error thrown by Rialto.

use aarch64_paging::MapError;
use core::{fmt, result};
use fdtpci::PciError;
use hyp::Error as HypervisorError;
use libfdt::FdtError;
use vmbase::{memory::MemoryTrackerError, virtio::pci};

pub type Result<T> = result::Result<T, Error>;

type CiboriumSerError = ciborium::ser::Error<virtio_drivers::Error>;
type CiboriumDeError = ciborium::de::Error<virtio_drivers::Error>;

#[derive(Debug)]
pub enum Error {
    /// Hypervisor error.
    Hypervisor(HypervisorError),
    /// Failed when attempting to map some range in the page table.
    PageTableMapping(MapError),
    /// Invalid FDT.
    InvalidFdt(FdtError),
    /// Invalid PCI.
    InvalidPci(PciError),
    /// Failed memory operation.
    MemoryOperationFailed(MemoryTrackerError),
    /// Failed to initialize PCI.
    PciInitializationFailed(pci::PciError),
    /// Failed to create VirtIO Socket device.
    VirtIOSocketCreationFailed(virtio_drivers::Error),
    /// Missing socket device.
    MissingVirtIOSocketDevice,
    /// Failed VirtIO driver operation.
    VirtIODriverOperationFailed(virtio_drivers::Error),
    /// Failed to serialize.
    SerializationFailed(CiboriumSerError),
    /// Failed to deserialize.
    DeserializationFailed(CiboriumDeError),
    /// Failed to process request.
    RequestProcessingFailed(RequestProcessingError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Hypervisor(e) => write!(f, "Hypervisor error: {e}."),
            Self::PageTableMapping(e) => {
                write!(f, "Failed when attempting to map some range in the page table: {e}.")
            }
            Self::InvalidFdt(e) => write!(f, "Invalid FDT: {e}"),
            Self::InvalidPci(e) => write!(f, "Invalid PCI: {e}"),
            Self::MemoryOperationFailed(e) => write!(f, "Failed memory operation: {e}"),
            Self::PciInitializationFailed(e) => write!(f, "Failed to initialize PCI: {e}"),
            Self::VirtIOSocketCreationFailed(e) => {
                write!(f, "Failed to create VirtIO Socket device: {e}")
            }
            Self::MissingVirtIOSocketDevice => write!(f, "Missing VirtIO Socket device."),
            Self::VirtIODriverOperationFailed(e) => {
                write!(f, "Failed VirtIO driver operation: {e}")
            }
            Self::SerializationFailed(e) => write!(f, "Failed to serialize: {e}"),
            Self::DeserializationFailed(e) => write!(f, "Failed to deserialize: {e}"),
            Self::RequestProcessingFailed(e) => write!(f, "Failed to process request: {e}"),
        }
    }
}

impl From<HypervisorError> for Error {
    fn from(e: HypervisorError) -> Self {
        Self::Hypervisor(e)
    }
}

impl From<MapError> for Error {
    fn from(e: MapError) -> Self {
        Self::PageTableMapping(e)
    }
}

impl From<FdtError> for Error {
    fn from(e: FdtError) -> Self {
        Self::InvalidFdt(e)
    }
}

impl From<PciError> for Error {
    fn from(e: PciError) -> Self {
        Self::InvalidPci(e)
    }
}

impl From<MemoryTrackerError> for Error {
    fn from(e: MemoryTrackerError) -> Self {
        Self::MemoryOperationFailed(e)
    }
}

impl From<virtio_drivers::Error> for Error {
    fn from(e: virtio_drivers::Error) -> Self {
        Self::VirtIODriverOperationFailed(e)
    }
}

impl From<CiboriumSerError> for Error {
    fn from(e: CiboriumSerError) -> Self {
        Self::SerializationFailed(e)
    }
}

impl From<CiboriumDeError> for Error {
    fn from(e: CiboriumDeError) -> Self {
        Self::DeserializationFailed(e)
    }
}

impl From<RequestProcessingError> for Error {
    fn from(e: RequestProcessingError) -> Self {
        Self::RequestProcessingFailed(e)
    }
}

/// Errors related to request processing.
#[derive(Debug)]
pub enum RequestProcessingError {
    /// Failed to generate keys.
    KeyGeneration,
    /// Failed to get the private key.
    GettingPrivateKey,
    /// An internal error happened.
    InternalError(&'static str),
    /// An error happened during the interaction with coset.
    CosetError(coset::CoseError),
    /// This error type contains the errors defined in the IRPC spec.
    /// It should be forwarded to the HAL.
    RemoteProvisioningError(service_vm_comm::RemoteProvisioningError),
    /// No payload found in a key to sign.
    KeyToSignHasEmptyPayload,
    /// An error happened when serializing to/from a `Value`.
    CborValueError(ciborium::value::Error),
}

impl fmt::Display for RequestProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::KeyGeneration => write!(f, "Failed to generate keys"),
            Self::GettingPrivateKey => write!(f, "Failed to get the private key"),
            Self::InternalError(context) => write!(f, "Encountered an internal error: {context}"),
            Self::CosetError(e) => write!(f, "Encountered an error with coset: {e}"),
            Self::RemoteProvisioningError(e) => {
                write!(f, "Encountered an error defined in the IRPC spec: {e:?}")
            }
            Self::KeyToSignHasEmptyPayload => write!(f, "No payload found in a key to sign."),
            Self::CborValueError(e) => {
                write!(f, "An error happened when serializing to/from a CBOR Value: {e}")
            }
        }
    }
}

impl From<coset::CoseError> for RequestProcessingError {
    fn from(e: coset::CoseError) -> Self {
        Self::CosetError(e)
    }
}

impl From<ciborium::value::Error> for RequestProcessingError {
    fn from(e: ciborium::value::Error) -> Self {
        Self::CborValueError(e)
    }
}
