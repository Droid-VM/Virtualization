// Copyright 2024, The Android Open Source Project
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

//! Import necessary of UEFI crate for implementing EFI stub in pvmfw.

#![allow(dead_code)]
#![allow(private_interfaces)]
#![allow(improper_ctypes_definitions)]

use core::ffi::c_void;
use core::mem;
use core::ops::RangeInclusive;
use core::ptr;

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SystemTable {
    pub header: Header,

    pub firmware_vendor: *const Char16,
    pub firmware_revision: u32,

    pub stdin_handle: Handle,
    pub stdin: *mut SimpleTextInputProtocol,

    pub stdout_handle: Handle,
    pub stdout: *mut SimpleTextOutputProtocol,

    pub stderr_handle: Handle,
    pub stderr: *mut SimpleTextOutputProtocol,

    pub runtime_services: *mut RuntimeServices,
    pub boot_services: *mut BootServices,
    pub number_of_configuration_table_entries: usize,
    pub configuration_table: *mut ConfigurationTable,
}

impl SystemTable {
    pub const SIGNATURE: u64 = 0x5453_5953_2049_4249;
}

impl Default for SystemTable {
    /// Create a `SystemTable` with most fields set to zero.
    ///
    /// The only fields not set to zero are:
    /// * [`Header::signature`] is set to [`SystemTable::SIGNATURE`].
    /// * [`Header::size`] is set to the size in bytes of `SystemTable`.
    fn default() -> Self {
        Self {
            header: Header {
                signature: Self::SIGNATURE,
                size: u32::try_from(mem::size_of::<Self>()).unwrap(),
                ..Header::default()
            },

            firmware_vendor: ptr::null_mut(),
            firmware_revision: 0,

            stdin_handle: ptr::null_mut(),
            stdin: ptr::null_mut(),

            stdout_handle: ptr::null_mut(),
            stdout: ptr::null_mut(),

            stderr_handle: ptr::null_mut(),
            stderr: ptr::null_mut(),

            runtime_services: ptr::null_mut(),
            boot_services: ptr::null_mut(),
            number_of_configuration_table_entries: 0,
            configuration_table: ptr::null_mut(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Tpl(pub u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Event(u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Char16(u16);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct SimpleTextInputProtocol(u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct RuntimeServices(u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct EventType(u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct SimpleTextOutputMode {
    pub max_mode: i32,
    pub mode: i32,
    pub attribute: i32,
    pub cursor_column: i32,
    pub cursor_row: i32,
    pub cursor_visible: bool,
}

#[derive(Debug)]
#[repr(C)]
pub struct SimpleTextOutputProtocol {
    pub reset: unsafe extern "efiapi" fn(this: *mut Self, extended: bool) -> Status,
    pub output_string: unsafe extern "efiapi" fn(this: *mut Self, string: *const Char16) -> Status,
    pub test_string: unsafe extern "efiapi" fn(this: *mut Self, string: *const Char16) -> Status,
    pub query_mode: unsafe extern "efiapi" fn(
        this: *mut Self,
        mode: usize,
        columns: *mut usize,
        rows: *mut usize,
    ) -> Status,
    pub set_mode: unsafe extern "efiapi" fn(this: *mut Self, mode: usize) -> Status,
    pub set_attribute: unsafe extern "efiapi" fn(this: *mut Self, attribute: usize) -> Status,
    pub clear_screen: unsafe extern "efiapi" fn(this: *mut Self) -> Status,
    pub set_cursor_position:
        unsafe extern "efiapi" fn(this: *mut Self, column: usize, row: usize) -> Status,
    pub enable_cursor: unsafe extern "efiapi" fn(this: *mut Self, visible: bool) -> Status,
    pub mode: *mut SimpleTextOutputMode,
}

#[derive(Copy, Clone, Debug)]
pub struct ConfigurationTable(u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Guid(u32);

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct EventNotifyFn(u32);

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct InterfaceType(u32);

#[derive(Copy, Clone, Debug)]
pub struct DevicePathProtocol(u32);

#[derive(Copy, Clone, Debug)]
pub struct OpenProtocolInformationEntry(u32);

/// Table of pointers to all the boot services.
#[derive(Debug)]
#[repr(C)]
pub struct BootServices {
    pub header: Header,

    // Task Priority services
    pub raise_tpl: unsafe extern "efiapi" fn(new_tpl: Tpl) -> Tpl,
    pub restore_tpl: unsafe extern "efiapi" fn(old_tpl: Tpl),

    // Memory allocation functions
    pub allocate_pages: unsafe extern "efiapi" fn(
        alloc_ty: u32,
        mem_ty: MemoryType,
        count: usize,
        addr: *mut PhysicalAddress,
    ) -> Status,
    pub free_pages: unsafe extern "efiapi" fn(addr: PhysicalAddress, pages: usize) -> Status,
    pub get_memory_map: unsafe extern "efiapi" fn(
        size: *mut usize,
        map: *mut MemoryDescriptor,
        key: *mut usize,
        desc_size: *mut usize,
        desc_version: *mut u32,
    ) -> Status,
    pub allocate_pool: unsafe extern "efiapi" fn(
        pool_type: MemoryType,
        size: usize,
        buffer: *mut *mut u8,
    ) -> Status,
    pub free_pool: unsafe extern "efiapi" fn(buffer: *mut u8) -> Status,

    // Event & timer functions
    pub create_event: unsafe extern "efiapi" fn(
        ty: EventType,
        notify_tpl: Tpl,
        notify_func: Option<EventNotifyFn>,
        notify_ctx: *mut c_void,
        out_event: *mut Event,
    ) -> Status,
    pub set_timer: unsafe extern "efiapi" fn(event: Event, ty: u32, trigger_time: u64) -> Status,
    pub wait_for_event: unsafe extern "efiapi" fn(
        number_of_events: usize,
        events: *mut Event,
        out_index: *mut usize,
    ) -> Status,
    pub signal_event: unsafe extern "efiapi" fn(event: Event) -> Status,
    pub close_event: unsafe extern "efiapi" fn(event: Event) -> Status,
    pub check_event: unsafe extern "efiapi" fn(event: Event) -> Status,

    // Protocol handlers
    pub install_protocol_interface: unsafe extern "efiapi" fn(
        handle: *mut Handle,
        guid: *const Guid,
        interface_type: InterfaceType,
        interface: *const c_void,
    ) -> Status,
    pub reinstall_protocol_interface: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol: *const Guid,
        old_interface: *const c_void,
        new_interface: *const c_void,
    ) -> Status,
    pub uninstall_protocol_interface: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol: *const Guid,
        interface: *const c_void,
    ) -> Status,
    pub handle_protocol: unsafe extern "efiapi" fn(
        handle: Handle,
        proto: *const Guid,
        out_proto: *mut *mut c_void,
    ) -> Status,
    pub reserved: *mut c_void,
    pub register_protocol_notify: unsafe extern "efiapi" fn(
        protocol: *const Guid,
        event: Event,
        registration: *mut *const c_void,
    ) -> Status,
    pub locate_handle: unsafe extern "efiapi" fn(
        search_ty: i32,
        proto: *const Guid,
        key: *const c_void,
        buf_sz: *mut usize,
        buf: *mut Handle,
    ) -> Status,
    pub locate_device_path: unsafe extern "efiapi" fn(
        proto: *const Guid,
        device_path: *mut *const DevicePathProtocol,
        out_handle: *mut Handle,
    ) -> Status,
    pub install_configuration_table:
        unsafe extern "efiapi" fn(guid_entry: *const Guid, table_ptr: *const c_void) -> Status,

    // Image services
    pub load_image: unsafe extern "efiapi" fn(
        boot_policy: u8,
        parent_image_handle: Handle,
        device_path: *const DevicePathProtocol,
        source_buffer: *const u8,
        source_size: usize,
        image_handle: *mut Handle,
    ) -> Status,
    pub start_image: unsafe extern "efiapi" fn(
        image_handle: Handle,
        exit_data_size: *mut usize,
        exit_data: *mut *mut Char16,
    ) -> Status,
    pub exit: unsafe extern "efiapi" fn(
        image_handle: Handle,
        exit_status: Status,
        exit_data_size: usize,
        exit_data: *mut Char16,
    ) -> !,
    pub unload_image: unsafe extern "efiapi" fn(image_handle: Handle) -> Status,
    pub exit_boot_services:
        unsafe extern "efiapi" fn(image_handle: Handle, map_key: usize) -> Status,

    // Misc services
    pub get_next_monotonic_count: unsafe extern "efiapi" fn(count: *mut u64) -> Status,
    pub stall: unsafe extern "efiapi" fn(microseconds: usize) -> Status,
    pub set_watchdog_timer: unsafe extern "efiapi" fn(
        timeout: usize,
        watchdog_code: u64,
        data_size: usize,
        watchdog_data: *const u16,
    ) -> Status,

    // Driver support services
    pub connect_controller: unsafe extern "efiapi" fn(
        controller: Handle,
        driver_image: Handle,
        remaining_device_path: *const DevicePathProtocol,
        recursive: bool,
    ) -> Status,
    pub disconnect_controller: unsafe extern "efiapi" fn(
        controller: Handle,
        driver_image: Handle,
        child: Handle,
    ) -> Status,

    // Protocol open / close services
    pub open_protocol: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol: *const Guid,
        interface: *mut *mut c_void,
        agent_handle: Handle,
        controller_handle: Handle,
        attributes: u32,
    ) -> Status,
    pub close_protocol: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol: *const Guid,
        agent_handle: Handle,
        controller_handle: Handle,
    ) -> Status,
    pub open_protocol_information: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol: *const Guid,
        entry_buffer: *mut *const OpenProtocolInformationEntry,
        entry_count: *mut usize,
    ) -> Status,

    // Library services
    pub protocols_per_handle: unsafe extern "efiapi" fn(
        handle: Handle,
        protocol_buffer: *mut *mut *const Guid,
        protocol_buffer_count: *mut usize,
    ) -> Status,
    pub locate_handle_buffer: unsafe extern "efiapi" fn(
        search_ty: i32,
        proto: *const Guid,
        key: *const c_void,
        no_handles: *mut usize,
        buf: *mut *mut Handle,
    ) -> Status,
    pub locate_protocol: unsafe extern "efiapi" fn(
        proto: *const Guid,
        registration: *mut c_void,
        out_proto: *mut *mut c_void,
    ) -> Status,

    /// Warning: this function pointer is declared as `extern "C"` rather than
    /// `extern "efiapi". That means it will work correctly when called from a
    /// UEFI target (`*-unknown-uefi`), but will not work when called from a
    /// target with a different calling convention such as
    /// `x86_64-unknown-linux-gnu`.
    ///
    /// Support for C-variadics with `efiapi` requires the unstable
    /// [`extended_varargs_abi_support`](https://github.com/rust-lang/rust/issues/100189)
    /// feature.
    pub install_multiple_protocol_interfaces:
        unsafe extern "C" fn(handle: *mut Handle, ...) -> Status,

    /// Warning: this function pointer is declared as `extern "C"` rather than
    /// `extern "efiapi". That means it will work correctly when called from a
    /// UEFI target (`*-unknown-uefi`), but will not work when called from a
    /// target with a different calling convention such as
    /// `x86_64-unknown-linux-gnu`.
    ///
    /// Support for C-variadics with `efiapi` requires the unstable
    /// [`extended_varargs_abi_support`](https://github.com/rust-lang/rust/issues/100189)
    /// feature.
    pub uninstall_multiple_protocol_interfaces: unsafe extern "C" fn(handle: Handle, ...) -> Status,

    // CRC services
    pub calculate_crc32:
        unsafe extern "efiapi" fn(data: *const c_void, data_size: usize, crc32: *mut u32) -> Status,

    // Misc services
    pub copy_mem: unsafe extern "efiapi" fn(dest: *mut u8, src: *const u8, len: usize),
    pub set_mem: unsafe extern "efiapi" fn(buffer: *mut u8, len: usize, value: u8),

    // New event functions (UEFI 2.0 or newer)
    pub create_event_ex: unsafe extern "efiapi" fn(
        ty: EventType,
        notify_tpl: Tpl,
        notify_fn: Option<EventNotifyFn>,
        notify_ctx: *mut c_void,
        event_group: *mut Guid,
        out_event: *mut Event,
    ) -> Status,
}

/// Interface a C-style enum as an integer newtype.
///
/// This macro implements Debug for you, the way you would expect it to work on
/// Rust enums (printing the variant name instead of its integer value). It also
/// derives Clone, Copy, Eq, PartialEq, Ord, PartialOrd, and Hash, since that
/// always makes sense for C-style enums. If you want anything else
/// to be derived, you can ask for it by adding extra derives as shown in the
/// example below.
///
/// One minor annoyance is that since variants will be translated into
/// associated constants in a separate impl block, you need to discriminate
/// which attributes should go on the type and which should go on the impl
/// block. The latter should go on the right-hand side of the arrow operator.
///
/// Usage example:
/// ```
/// # use uefi_raw::newtype_enum;
/// newtype_enum! {
/// #[derive(Default)]
/// pub enum UnixBool: i32 => #[allow(missing_docs)] {
///     FALSE          =  0,
///     TRUE           =  1,
///     /// Nobody expects the Unix inquisition!
///     FILE_NOT_FOUND = -1,
/// }}
/// ```
#[macro_export]
macro_rules! newtype_enum {
    (
        $(#[$type_attrs:meta])*
        $visibility:vis enum $type:ident : $base_integer:ty => $(#[$impl_attrs:meta])* {
            $(
                $(#[$variant_attrs:meta])*
                $variant:ident = $value:expr,
            )*
        }
    ) => {
        $(#[$type_attrs])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
        $visibility struct $type(pub $base_integer);

        $(#[$impl_attrs])*
        #[allow(unused)]
        impl $type {
            $(
                $(#[$variant_attrs])*
                pub const $variant: $type = $type($value);
            )*
        }

        impl core::fmt::Debug for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                match *self {
                    // Display variants by their name, like Rust enums do
                    $(
                        $type::$variant => write!(f, stringify!($variant)),
                    )*

                    // Display unknown variants in tuple struct format
                    $type(unknown) => {
                        write!(f, "{}({})", stringify!($type), unknown)
                    }
                }
            }
        }
    }
}

/// Type of allocation to perform.
#[derive(Debug, Copy, Clone)]
pub enum AllocateType {
    /// Allocate any possible pages.
    AnyPages,
    /// Allocate pages at any address below the given address.
    MaxAddress(PhysicalAddress),
    /// Allocate pages at the specified address.
    Address(PhysicalAddress),
}

newtype_enum! {
/// The type of a memory range.
///
/// UEFI allows firmwares and operating systems to introduce new memory types
/// in the `0x7000_0000..=0xFFFF_FFFF` range. Therefore, we don't know the full set
/// of memory types at compile time, and it is _not_ safe to model this C enum
/// as a Rust enum.
pub enum MemoryType: u32 => {
    /// Not usable.
    RESERVED                =  0,
    /// The code portions of a loaded UEFI application.
    LOADER_CODE             =  1,
    /// The data portions of a loaded UEFI applications,
    /// as well as any memory allocated by it.
    LOADER_DATA             =  2,
    /// Code of the boot drivers.
    ///
    /// Can be reused after OS is loaded.
    BOOT_SERVICES_CODE      =  3,
    /// Memory used to store boot drivers' data.
    ///
    /// Can be reused after OS is loaded.
    BOOT_SERVICES_DATA      =  4,
    /// Runtime drivers' code.
    RUNTIME_SERVICES_CODE   =  5,
    /// Runtime services' code.
    RUNTIME_SERVICES_DATA   =  6,
    /// Free usable memory.
    CONVENTIONAL            =  7,
    /// Memory in which errors have been detected.
    UNUSABLE                =  8,
    /// Memory that holds ACPI tables.
    /// Can be reclaimed after they are parsed.
    ACPI_RECLAIM            =  9,
    /// Firmware-reserved addresses.
    ACPI_NON_VOLATILE       = 10,
    /// A region used for memory-mapped I/O.
    MMIO                    = 11,
    /// Address space used for memory-mapped port I/O.
    MMIO_PORT_SPACE         = 12,
    /// Address space which is part of the processor.
    PAL_CODE                = 13,
    /// Memory region which is usable and is also non-volatile.
    PERSISTENT_MEMORY       = 14,
    /// Memory that must be accepted by the boot target before it can be used.
    UNACCEPTED              = 15,
    /// End of the defined memory types. Higher values are possible though, see
    /// [`MemoryType::RESERVED_FOR_OEM`] and [`MemoryType::RESERVED_FOR_OS_LOADER`].
    MAX                     = 16,
}}

impl MemoryType {
    /// Range reserved for OEM use.
    pub const RESERVED_FOR_OEM: RangeInclusive<u32> = 0x7000_0000..=0x7fff_ffff;

    /// Range reserved for OS loaders.
    pub const RESERVED_FOR_OS_LOADER: RangeInclusive<u32> = 0x8000_0000..=0xffff_ffff;

    /// Construct a custom `MemoryType`. Values in the range `0x8000_0000..=0xffff_ffff` are free
    /// for use if you are an OS loader.
    #[must_use]
    pub const fn custom(value: u32) -> Self {
        assert!(value >= 0x80000000);
        Self(value)
    }
}

/// Physical memory address. This is always a 64-bit value, regardless
/// of target platform.
pub type PhysicalAddress = u64;

/// Virtual memory address. This is always a 64-bit value, regardless
/// of target platform.
pub type VirtualAddress = u64;

/// Handle to a UEFI entity (protocol, image, etc).
pub type Handle = *mut c_void;

newtype_enum! {
/// UEFI uses status codes in order to report successes, errors, and warnings.
///
/// The spec allows implementation-specific status codes, so the `Status`
/// constants are not a comprehensive list of all possible values.
#[must_use]
pub enum Status: usize => {
    /// The operation completed successfully.
    SUCCESS                 =  0,

    /// The string contained characters that could not be rendered and were skipped.
    WARN_UNKNOWN_GLYPH      =  1,
    /// The handle was closed, but the file was not deleted.
    WARN_DELETE_FAILURE     =  2,
    /// The handle was closed, but the data to the file was not flushed properly.
    WARN_WRITE_FAILURE      =  3,
    /// The resulting buffer was too small, and the data was truncated.
    WARN_BUFFER_TOO_SMALL   =  4,
    /// The data has not been updated within the timeframe set by local policy.
    WARN_STALE_DATA         =  5,
    /// The resulting buffer contains UEFI-compliant file system.
    WARN_FILE_SYSTEM        =  6,
    /// The operation will be processed across a system reset.
    WARN_RESET_REQUIRED     =  7,

    /// The image failed to load.
    LOAD_ERROR              = Self::ERROR_BIT |  1,
    /// A parameter was incorrect.
    INVALID_PARAMETER       = Self::ERROR_BIT |  2,
    /// The operation is not supported.
    UNSUPPORTED             = Self::ERROR_BIT |  3,
    /// The buffer was not the proper size for the request.
    BAD_BUFFER_SIZE         = Self::ERROR_BIT |  4,
    /// The buffer is not large enough to hold the requested data.
    /// The required buffer size is returned in the appropriate parameter.
    BUFFER_TOO_SMALL        = Self::ERROR_BIT |  5,
    /// There is no data pending upon return.
    NOT_READY               = Self::ERROR_BIT |  6,
    /// The physical device reported an error while attempting the operation.
    DEVICE_ERROR            = Self::ERROR_BIT |  7,
    /// The device cannot be written to.
    WRITE_PROTECTED         = Self::ERROR_BIT |  8,
    /// A resource has run out.
    OUT_OF_RESOURCES        = Self::ERROR_BIT |  9,
    /// An inconstency was detected on the file system.
    VOLUME_CORRUPTED        = Self::ERROR_BIT | 10,
    /// There is no more space on the file system.
    VOLUME_FULL             = Self::ERROR_BIT | 11,
    /// The device does not contain any medium to perform the operation.
    NO_MEDIA                = Self::ERROR_BIT | 12,
    /// The medium in the device has changed since the last access.
    MEDIA_CHANGED           = Self::ERROR_BIT | 13,
    /// The item was not found.
    NOT_FOUND               = Self::ERROR_BIT | 14,
    /// Access was denied.
    ACCESS_DENIED           = Self::ERROR_BIT | 15,
    /// The server was not found or did not respond to the request.
    NO_RESPONSE             = Self::ERROR_BIT | 16,
    /// A mapping to a device does not exist.
    NO_MAPPING              = Self::ERROR_BIT | 17,
    /// The timeout time expired.
    TIMEOUT                 = Self::ERROR_BIT | 18,
    /// The protocol has not been started.
    NOT_STARTED             = Self::ERROR_BIT | 19,
    /// The protocol has already been started.
    ALREADY_STARTED         = Self::ERROR_BIT | 20,
    /// The operation was aborted.
    ABORTED                 = Self::ERROR_BIT | 21,
    /// An ICMP error occurred during the network operation.
    ICMP_ERROR              = Self::ERROR_BIT | 22,
    /// A TFTP error occurred during the network operation.
    TFTP_ERROR              = Self::ERROR_BIT | 23,
    /// A protocol error occurred during the network operation.
    PROTOCOL_ERROR          = Self::ERROR_BIT | 24,
    /// The function encountered an internal version that was
    /// incompatible with a version requested by the caller.
    INCOMPATIBLE_VERSION    = Self::ERROR_BIT | 25,
    /// The function was not performed due to a security violation.
    SECURITY_VIOLATION      = Self::ERROR_BIT | 26,
    /// A CRC error was detected.
    CRC_ERROR               = Self::ERROR_BIT | 27,
    /// Beginning or end of media was reached
    END_OF_MEDIA            = Self::ERROR_BIT | 28,
    /// The end of the file was reached.
    END_OF_FILE             = Self::ERROR_BIT | 31,
    /// The language specified was invalid.
    INVALID_LANGUAGE        = Self::ERROR_BIT | 32,
    /// The security status of the data is unknown or compromised and
    /// the data must be updated or replaced to restore a valid security status.
    COMPROMISED_DATA        = Self::ERROR_BIT | 33,
    /// There is an address conflict address allocation
    IP_ADDRESS_CONFLICT     = Self::ERROR_BIT | 34,
    /// A HTTP error occurred during the network operation.
    HTTP_ERROR              = Self::ERROR_BIT | 35,
}}

impl Status {
    /// Bit indicating that an UEFI status code is an error.
    pub const ERROR_BIT: usize = 1 << (core::mem::size_of::<usize>() * 8 - 1);

    /// Returns true if status code indicates success.
    #[inline]
    #[must_use]
    pub fn is_success(self) -> bool {
        self == Self::SUCCESS
    }

    /// Returns true if status code indicates a warning.
    #[inline]
    #[must_use]
    pub fn is_warning(self) -> bool {
        (self != Self::SUCCESS) && (self.0 & Self::ERROR_BIT == 0)
    }

    /// Returns true if the status code indicates an error.
    #[inline]
    #[must_use]
    pub const fn is_error(self) -> bool {
        self.0 & Self::ERROR_BIT != 0
    }
}

/// A structure describing a region of memory. This type corresponds to [version]
/// of this struct in the UEFI spec and is always bound to a corresponding
/// UEFI memory map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct MemoryDescriptor {
    /// Type of memory occupying this range.
    pub ty: MemoryType,
    // Implicit 32-bit padding.
    /// Starting physical address.
    pub phys_start: PhysicalAddress,
    /// Starting virtual address.
    pub virt_start: VirtualAddress,
    /// Number of 4 KiB pages contained in this range.
    pub page_count: u64,
}

impl MemoryDescriptor {
    /// Memory descriptor version number.
    pub const VERSION: u32 = 1;
}

impl Default for MemoryDescriptor {
    fn default() -> Self {
        Self { ty: MemoryType::RESERVED, phys_start: 0, virt_start: 0, page_count: 0 }
    }
}

/// The common header that all UEFI tables begin with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct Header {
    /// Unique identifier for this table.
    pub signature: u64,
    /// Revision of the spec this table conforms to.
    pub revision: Revision,
    /// The size in bytes of the entire table.
    pub size: u32,
    /// 32-bit CRC-32-Castagnoli of the entire table,
    /// calculated with this field set to 0.
    pub crc: u32,
    /// Reserved field that must be set to 0.
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Revision(pub u32);

// Allow missing docs, there's nothing useful to document about these
// constants.
#[allow(missing_docs)]
impl Revision {
    pub const EFI_1_02: Self = Self::new(1, 2);
    pub const EFI_1_10: Self = Self::new(1, 10);
    pub const EFI_2_00: Self = Self::new(2, 00);
    pub const EFI_2_10: Self = Self::new(2, 10);
    pub const EFI_2_20: Self = Self::new(2, 20);
    pub const EFI_2_30: Self = Self::new(2, 30);
    pub const EFI_2_31: Self = Self::new(2, 31);
    pub const EFI_2_40: Self = Self::new(2, 40);
    pub const EFI_2_50: Self = Self::new(2, 50);
    pub const EFI_2_60: Self = Self::new(2, 60);
    pub const EFI_2_70: Self = Self::new(2, 70);
    pub const EFI_2_80: Self = Self::new(2, 80);
    pub const EFI_2_90: Self = Self::new(2, 90);
    pub const EFI_2_100: Self = Self::new(2, 100);
}

impl Revision {
    /// Creates a new revision.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        let major = major as u32;
        let minor = minor as u32;
        let value = (major << 16) | minor;
        Self(value)
    }

    /// Returns the major revision.
    #[must_use]
    pub const fn major(self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// Returns the minor revision.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.0 as u16
    }
}
