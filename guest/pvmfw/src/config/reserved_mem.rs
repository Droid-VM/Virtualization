use core::alloc::Layout;
use core::ffi::CStr;
use core::ptr::NonNull;
use libfdt::Fdt;
use zerocopy::byteorder::U32;
use zerocopy::{CastError, FromBytes, Immutable, IntoBytes, KnownLayout, NativeEndian};

#[cfg(not(test))]
use alloc::alloc::alloc;
#[cfg(test)]
use std::alloc::alloc;

pub const NO_MAP: u32 = 1u32;

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    /// The struct indicated a different number of headers than were found.
    InconsistentSize,
    /// Data offsets invalid.
    InvalidOffset,
    /// Compat string was not a valid CStr.
    InvalidCStr,
    /// Passed in buffer too small for any data.
    InputBufferSmall,
    /// Unable to find reserved memory node.
    MissingNode,
    /// Unable to add a device tree node.
    AppendFailure,
    /// Input buffer was unaligned.
    UnalignedData,
    /// Memory allocation failed.
    Alloc,
    /// A header referenced a zero size memory region.
    ZeroSize,
    /// Bad guest VM alignment.
    BadGuestAlign,
}

#[repr(C)]
#[derive(
    Clone, Copy, Debug, Default, Eq, FromBytes, Immutable, IntoBytes, KnownLayout, PartialEq,
)]
pub(crate) struct RMemHeader {
    vm_name_offset: U32<NativeEndian>,
    blob_offset: U32<NativeEndian>,
    blob_size: U32<NativeEndian>,
    compat_offset: U32<NativeEndian>,
    flags: U32<NativeEndian>,
}

impl RMemHeader {
    fn blob_offset(&self) -> usize {
        self.blob_offset.get() as usize
    }

    fn blob_size(&self) -> usize {
        self.blob_size.get() as usize
    }

    fn compat_offset(&self) -> usize {
        self.compat_offset.get() as usize
    }

    fn vm_name_offset(&self) -> usize {
        self.vm_name_offset.get() as usize
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, Immutable, KnownLayout, PartialEq)]
pub(crate) struct ConfigHeader<'a> {
    hdrs: &'a [RMemHeader],
    buffer: &'a [u8],
}

fn check_cstr(buffer: &[u8]) -> Result<(), Error> {
    core::ffi::CStr::from_bytes_until_nul(buffer).map(|_| ()).map_err(|_| Error::InvalidCStr)
}

impl<'a> ConfigHeader<'a> {
    fn new(buffer: &'a [u8]) -> Result<Self, Error> {
        let (count, rem) = u32::read_from_prefix(buffer).map_err(|_| Error::InputBufferSmall)?;
        let (hdrs, buffer) = <[RMemHeader]>::ref_from_prefix_with_elems(rem, count as usize)
            .map_err(|e| match e {
                CastError::Alignment(_) => Error::UnalignedData,
                _ => Error::InconsistentSize,
            })?;

        let mut last_blob_end = 0;

        for hdr in hdrs.iter() {
            check_cstr(buffer.get(hdr.compat_offset()..).ok_or(Error::InvalidOffset)?)?;
            check_cstr(buffer.get(hdr.vm_name_offset()..).ok_or(Error::InvalidOffset)?)?;

            let limits = last_blob_end..buffer.len();
            let offset = hdr.blob_offset();
            let blob = offset..(offset + hdr.blob_size());
            if !(blob.start >= limits.start && blob.end <= limits.end) {
                return Err(Error::InvalidOffset);
            }
            last_blob_end = blob.end;
        }

        Ok(Self { hdrs, buffer })
    }
}

impl<'a> IntoIterator for ConfigHeader<'a> {
    type Item = ConfigHeaderEntry<'a>;
    type IntoIter = ConfigHeaderIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ConfigHeaderIterator { hdr: self, next: 0 }
    }
}

pub(crate) struct ConfigHeaderIterator<'a> {
    hdr: ConfigHeader<'a>,
    next: usize,
}

pub(crate) struct ConfigHeaderEntry<'a> {
    blob: &'a [u8],
    compat: &'a CStr,
    vm_name: &'a CStr,
    flags: u32,
}

impl<'a> Iterator for ConfigHeaderIterator<'a> {
    type Item = ConfigHeaderEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next < self.hdr.hdrs.len() {
            let hdr = &self.hdr.hdrs[self.next];
            let compat =
                core::ffi::CStr::from_bytes_until_nul(&self.hdr.buffer[hdr.compat_offset()..])
                    .unwrap();
            let vm_name =
                core::ffi::CStr::from_bytes_until_nul(&self.hdr.buffer[hdr.vm_name_offset()..])
                    .unwrap();
            self.next += 1;
            Some(Self::Item {
                blob: &self.hdr.buffer[hdr.blob_offset()..hdr.blob_offset() + hdr.blob_size()],
                compat,
                vm_name,
                flags: hdr.flags.into(),
            })
        } else {
            None
        }
    }
}

/// Adds reserved memory nodes from config to the device tree in fdt if the vm name matches `name`.
///
/// `add_reserved_mem` parses `config` and a pvmfw reserved memory config and adds all memory nodes
/// to `fdt` if their vm name matches `name`. To ensure only the desired data is available to the
/// target vm, each matching region is allocated a new page or pages where the data is copied.
/// This memory can only be freed by the guest and therefore is leaked by pvmfw.
///
/// -`fdt`: a device tree to add reserved memory nodes to. /reserved-memory must already be
/// present.
/// -`config`: a pvmfw reserved memory blob to add nodes from
/// -`guest_align`: the page alignment of the guest vm. This must be a power of 2.
/// -`name`: a target vm name used to filter which reserved memory nodes to add.
pub fn add_reserved_mem(
    fdt: &mut Fdt,
    config: &[u8],
    guest_align: usize,
    name: &str,
) -> Result<(), Error> {
    let cfg_header = ConfigHeader::new(config)?;

    for hdr in cfg_header.into_iter() {
        if hdr.vm_name.to_str().map_err(|_| Error::InvalidCStr)? == name {
            if hdr.blob.is_empty() {
                return Err(Error::ZeroSize);
            }
            let layout = Layout::from_size_align(hdr.blob.len(), guest_align)
                .map_err(|_| Error::BadGuestAlign)?;
            // SAFETY: hdr.blob_size is non-zero.
            let ptr = unsafe { alloc(layout) };
            let ptr = NonNull::new(ptr).ok_or(Error::Alloc)?.as_ptr();
            // SAFETY: ptr was confirmed allocated for hdr.blob.len().
            unsafe { ptr.copy_from(hdr.blob.as_ptr(), hdr.blob.len()) };

            let mem_node = fdt
                .node_mut(c"/reserved-memory")
                .map_err(|_| Error::MissingNode)?
                .ok_or(Error::MissingNode)?;

            let mut node = mem_node.add_subnode(hdr.compat).map_err(|_| Error::AppendFailure)?;

            node.appendprop(c"compatible", &hdr.compat.to_bytes_with_nul())
                .map_err(|_| Error::AppendFailure)?;
            node.appendprop_addrrange(c"reg", ptr as u64, hdr.blob.len() as u64)
                .map_err(|_| Error::AppendFailure)?;
            if hdr.flags & NO_MAP == NO_MAP {
                node.setprop_empty(c"no-map").map_err(|_| Error::AppendFailure)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::ConvertError;

    const FDT_WITHOUT_DEVICE_FILE_PATH: &str = "test_pvmfw_devices_without_device.dtb";

    struct TestHeader<const N: usize> {
        size: u32,
        hdrs: [RMemHeader; N],
    }

    struct TestBlob<'a>(&'a CStr, Vec<u8>, &'a CStr);

    fn make_test_data<const N: usize>(data: &[TestBlob<'_>; N]) -> (TestHeader<N>, Vec<u8>) {
        let mut out_hdr = TestHeader { size: N as u32, hdrs: [Default::default(); N] };

        let mut offset = 0usize;
        let mut out_data = vec![];
        for (i, d) in data.iter().enumerate() {
            let vm_name_offset = offset as u32;
            offset += d.2.to_bytes_with_nul().len();
            let blob_offset = offset as u32;
            let blob_size = d.1.len() as u32;
            offset += d.1.len();
            let compat_offset = offset as u32;
            offset += d.0.to_bytes_with_nul().len();
            out_hdr.hdrs[i] = make_header(vm_name_offset, blob_offset, blob_size, compat_offset);

            out_data.extend_from_slice(d.2.to_bytes_with_nul());
            out_data.extend_from_slice(d.1.as_slice());
            out_data.extend_from_slice(d.0.to_bytes_with_nul());
        }
        (out_hdr, out_data)
    }

    fn make_header(
        vm_name_offset: u32,
        blob_offset: u32,
        blob_size: u32,
        compat_offset: u32,
    ) -> RMemHeader {
        RMemHeader {
            vm_name_offset: vm_name_offset.into(),
            blob_offset: blob_offset.into(),
            blob_size: blob_size.into(),
            compat_offset: compat_offset.into(),
            flags: 0.into(),
        }
    }

    fn make_test_vec<const N: usize>(hdr: TestHeader<N>, mut data: Vec<u8>) -> Vec<u8> {
        // reuse the `data` allocation
        data.extend_from_slice(hdr.size.as_bytes());
        data.extend_from_slice(hdr.hdrs.as_bytes());
        data.rotate_right(size_of_val(&hdr));
        data
    }

    /// Ensure `data` is aligned on an odd address; output the original data.
    /// Resizes `data` if necessary.
    fn dealign_vec_data(data: &mut Vec<u8>) -> &[u8] {
        assert!(data.len() >= 4);
        data.reserve(1);
        let out = if data.as_ptr() as usize % 2 == 0 {
            data.insert(0, 0);
            &data.as_slice()[1..]
        } else {
            data.as_slice()
        };
        assert!(matches!(u16::ref_from_prefix(out), Err(ConvertError::Alignment(_))));
        out
    }

    #[test]
    fn unaligned() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], c"Fry"),
            TestBlob(c"Bar", vec![0xBB; 256], c"Leela"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let mut bytes = make_test_vec(hdr, data);

        assert!(ConfigHeader::new(dealign_vec_data(&mut bytes)).is_ok());
    }

    #[test]
    fn success() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], c"Fry"),
            TestBlob(c"Bar", vec![0xBB; 256], c"Leela"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert!(cfg_header.is_ok());
    }

    #[test]
    fn input_too_small() {
        let data = &[0u8; 3];
        let cfg_header = ConfigHeader::new(data);
        assert_eq!(cfg_header, Err(Error::InputBufferSmall));
    }

    #[test]
    fn bad_header_size_too_large() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], c"Fry"),
            TestBlob(c"Bar", vec![0xBB; 256], c"Leela"),
            TestBlob(c"Baz", vec![0xCC; 64], c"Bender"),
        ];
        let (mut hdr, data) = make_test_data(&blobs);

        // Set incorrect size.
        hdr.size = 20000;

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InconsistentSize));
    }

    #[test]
    fn bad_total_size() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], c"Fry"),
            TestBlob(c"Bar", vec![0xBB; 17], c"Leela"),
            TestBlob(c"Baz", vec![0xCC; 64], c"Bender"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        let len = data.len();
        bytes.extend_from_slice(&data[..len - 64]);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn update_fdt_match_all() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        add_reserved_mem(fdt, &bytes, 4096, "Fry").unwrap();
        fdt.pack().unwrap();

        let rmem = fdt.node(c"/reserved-memory").unwrap().unwrap();

        for b in blobs {
            let node = rmem.next_compatible(b.0).unwrap().unwrap();
            let reg = node.reg().unwrap().unwrap().next().unwrap();
            let len: usize = reg.size.unwrap().try_into().unwrap();

            // SAFETY: Testing below that the data was copied to the address in the device tree.
            let data = unsafe { core::slice::from_raw_parts(reg.addr as *const u8, len) };
            assert_eq!(b.1, data);
        }
    }

    #[test]
    fn update_fdt_match_some() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], c"Leela"),
            TestBlob(strs[1], vec![0xBB; 128], c"Bender"),
            TestBlob(strs[2], vec![0xCC; 64], c"Leela"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        add_reserved_mem(fdt, &bytes, 4096, "Leela").unwrap();
        fdt.pack().unwrap();

        let rmem = fdt.node(c"/reserved-memory").unwrap().unwrap();

        for s in strs {
            if s == strs[1] {
                assert_eq!(rmem.next_compatible(s).unwrap(), None);
            } else {
                rmem.next_compatible(s).unwrap().unwrap();
            }
        }
    }

    #[test]
    fn update_fdt_match_none() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        add_reserved_mem(fdt, &bytes, 4096, "Zoidberg").unwrap();
        fdt.pack().unwrap();

        let rmem = fdt.node(c"/reserved-memory").unwrap().unwrap();

        for s in strs {
            assert_eq!(rmem.next_compatible(s).unwrap(), None);
        }
    }

    #[test]
    fn missing_reserved_mem() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = vec![0u8; 1024];
        let fdt = Fdt::create_empty_tree(&mut fdt_data).unwrap();

        assert_eq!(add_reserved_mem(fdt, &bytes, 4096, "Fry"), Err(Error::MissingNode));
    }

    #[test]
    fn zero_size() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 0], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        assert_eq!(add_reserved_mem(fdt, &bytes, 4096, "Fry"), Err(Error::ZeroSize));
    }

    #[test]
    fn bad_align() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        assert_eq!(add_reserved_mem(fdt, &bytes, 4095, "Fry"), Err(Error::BadGuestAlign));
    }
}
