use core::ffi::CStr;
use libfdt::Fdt;
use zerocopy::{CastError, FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    /// The struct indicated a different number of headers than were found.
    InconsistentSize,
    /// Data offsets invalid.
    InvalidOffset,
    /// Indicated size of blobs exceeded size of buffer.
    InvalidTotalSize,
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
}

#[repr(C)]
#[derive(
    Clone, Copy, Debug, Default, Eq, FromBytes, Immutable, IntoBytes, KnownLayout, PartialEq,
)]
pub(crate) struct RMemHeader {
    vm_uuid: [u8; 16],
    blob_offset: u32,
    blob_size: u32,
    compat_offset: u32,
    flags: u32,
}

impl RMemHeader {
    fn blob_offset(&self) -> usize {
        self.blob_offset as usize
    }

    fn blob_size(&self) -> usize {
        self.blob_size as usize
    }

    fn compat_offset(&self) -> usize {
        self.compat_offset as usize
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, Immutable, KnownLayout, PartialEq)]
pub(crate) struct ConfigHeader<'a> {
    hdrs: &'a [RMemHeader],
    buffer: &'a [u8],
}

fn check_compat(buffer: &[u8], expected_len: usize) -> Result<(), Error> {
    // compat strings should end with a null byte
    buffer
        .get(..expected_len)
        .and_then(|prev_compat| core::ffi::CStr::from_bytes_with_nul(prev_compat).ok())
        .map(|_| ())
        .ok_or(Error::InvalidCStr)
}

impl<'a> ConfigHeader<'a> {
    fn new(buffer: &'a [u8]) -> Result<Self, Error> {
        let (count, rem) = u32::read_from_prefix(buffer).map_err(|_| Error::InputBufferSmall)?;
        let (hdrs, buffer) = <[RMemHeader]>::ref_from_prefix_with_elems(rem, count as usize)
            .map_err(|e| match e {
                CastError::Alignment(_) => Error::UnalignedData,
                _ => Error::InconsistentSize,
            })?;

        for (i, hdr) in hdrs.iter().enumerate() {
            if i < hdrs.len() - 1 && hdr.blob_offset() + hdr.blob_size() > hdrs[i + 1].blob_offset()
            {
                return Err(Error::InvalidOffset);
            }

            // check that blob size and compat offset are consistent
            if hdr.blob_offset() + hdr.blob_size() > hdr.compat_offset() {
                return Err(Error::InvalidOffset);
            }
            // previous compat string should not come after the next blob
            if i > 0 && hdrs[i - 1].compat_offset() >= hdr.blob_offset() {
                return Err(Error::InvalidOffset);
            }
            if i > 0 {
                check_compat(
                    &buffer[hdrs[i - 1].compat_offset()..],
                    hdr.blob_offset() - hdrs[i - 1].compat_offset(),
                )?;
            }
        }

        // buffer should be at least as long as the last compat_offset plus a null byte.
        if buffer.len() <= hdrs.iter().map(|hdr| hdr.compat_offset()).max().unwrap_or(0) {
            return Err(Error::InvalidTotalSize);
        }

        let last_compat_offset = hdrs[count as usize - 1].compat_offset();
        check_compat(&buffer[last_compat_offset..], buffer.len() - last_compat_offset)?;

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

struct ConfigHeaderEntry<'a> {
    blob_addr: u64,
    blob_size: u64,
    compat: &'a CStr,
    vm_uuid: &'a [u8; 16],
}

impl<'a> Iterator for ConfigHeaderIterator<'a> {
    type Item = ConfigHeaderEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next < self.hdr.hdrs.len() {
            let hdr = &self.hdr.hdrs[self.next];
            let compat =
                core::ffi::CStr::from_bytes_until_nul(&self.hdr.buffer[hdr.compat_offset()..])
                    .unwrap();
            self.next += 1;
            Some(Self::Item {
                blob_addr: self.hdr.buffer[hdr.blob_offset()..].as_ptr() as u64,
                blob_size: hdr.blob_size() as u64,
                compat,
                vm_uuid: &hdr.vm_uuid,
            })
        } else {
            None
        }
    }
}

pub fn parse_reserved_mem(fdt: &mut Fdt, config: &[u8], uuid: &[u8; 16]) -> Result<(), Error> {
    let cfg_header = ConfigHeader::new(config)?;

    for hdr in cfg_header.into_iter() {
        if hdr.vm_uuid == uuid {
            let mem_node = fdt
                .node_mut(c"/reserved-memory")
                .map_err(|_| Error::MissingNode)?
                .ok_or(Error::MissingNode)?;

            let mut node = mem_node.add_subnode(hdr.compat).map_err(|_| Error::AppendFailure)?;

            node.appendprop(c"compatible", &hdr.compat.to_bytes_with_nul())
                .map_err(|_| Error::AppendFailure)?;
            node.appendprop_addrrange(c"reg", hdr.blob_addr, hdr.blob_size)
                .map_err(|_| Error::AppendFailure)?;
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

    struct TestBlob<'a>(&'a CStr, Vec<u8>, [u8; 16]);

    fn make_test_data<const N: usize>(data: &[TestBlob<'_>; N]) -> (TestHeader<N>, Vec<u8>) {
        let mut out_hdr = TestHeader { size: N as u32, hdrs: [Default::default(); N] };

        let mut offset = 0usize;
        let mut out_data = vec![];
        for (i, d) in data.iter().enumerate() {
            let blob_offset = offset as u32;
            let blob_size = d.1.len() as u32;
            offset += d.1.len();
            let compat_offset = offset as u32;
            offset += d.0.to_bytes_with_nul().len();
            out_hdr.hdrs[i] = make_header(d.2, blob_offset, blob_size, compat_offset);

            out_data.extend_from_slice(d.1.as_slice());
            out_data.extend_from_slice(d.0.to_bytes_with_nul());
        }
        (out_hdr, out_data)
    }

    fn make_header(
        uuid: [u8; 16],
        blob_offset: u32,
        blob_size: u32,
        compat_offset: u32,
    ) -> RMemHeader {
        RMemHeader { vm_uuid: uuid, blob_offset, blob_size, compat_offset, flags: 0 }
    }

    fn make_test_vec<const N: usize>(hdr: TestHeader<N>, mut data: Vec<u8>) -> Vec<u8> {
        // reuse the `data` allocation
        data.extend_from_slice(hdr.size.as_bytes());
        data.extend_from_slice(hdr.hdrs.as_bytes());
        data.rotate_right(size_of_val(&hdr));
        data
    }

    #[test]
    fn success() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 256], [0xBB; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert!(cfg_header.is_ok());
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
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 256], [0xBB; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let mut bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(dealign_vec_data(&mut bytes));
        assert_eq!(cfg_header, Err(Error::UnalignedData));
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
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 256], [0xBB; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (mut hdr, data) = make_test_data(&blobs);

        // Set incorrect size.
        hdr.size = 23;

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InconsistentSize));
    }

    #[test]
    fn bad_blob_offset() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 256], [0xBB; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (mut hdr, data) = make_test_data(&blobs);

        // Set blob size to 0.
        hdr.hdrs[1].blob_size = 0;
        hdr.hdrs[1].blob_offset = hdr.hdrs[1].compat_offset + 1;

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn bad_cstr() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 256], [0xBB; 16]),
            TestBlob(c"abcdefg", vec![0xCC; 3], [0x55; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (hdr, mut data) = make_test_data(&blobs);

        // Set blob size.
        let offset = hdr.hdrs[2].compat_offset as usize;
        let offset = offset + blobs[2].0.count_bytes();
        assert_eq!(data[offset], b'\0');
        data[offset] = b'A';

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidCStr));
    }

    #[test]
    fn blob_size_past_compat() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 17], [0xBB; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (mut hdr, data) = make_test_data(&blobs);

        // Set blob size.
        hdr.hdrs[1].blob_size = 267;

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn bad_total_size() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 17], [0xBB; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        let len = data.len();
        bytes.extend_from_slice(&data[..len - 64]);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidTotalSize));
    }

    #[test]
    fn blob_size_past_next_blob() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], [0xAA; 16]),
            TestBlob(c"Bar", vec![0xBB; 17], [0xBB; 16]),
            TestBlob(c"Baz", vec![0xCC; 64], [0xCC; 16]),
        ];
        let (mut hdr, data) = make_test_data(&blobs);

        // Set blob 0 to run over blob 1. Update blob 1 size so total data size is still valid.
        hdr.hdrs[0].blob_size = 270;
        hdr.hdrs[0].compat_offset = hdr.hdrs[0].blob_offset + hdr.hdrs[0].blob_size;
        hdr.hdrs[1].blob_size = 3;
        hdr.hdrs[1].compat_offset = hdr.hdrs[1].blob_offset + hdr.hdrs[1].blob_size;

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn update_fdt_match_all() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], [0xAA; 16]),
            TestBlob(strs[1], vec![0xBB; 128], [0xAA; 16]),
            TestBlob(strs[2], vec![0xCC; 64], [0xAA; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        parse_reserved_mem(fdt, &bytes, &[0xAA; 16]).unwrap();
        fdt.pack().unwrap();

        let rmem = fdt.node(c"/reserved-memory").unwrap().unwrap();

        for s in strs {
            rmem.next_compatible(s).unwrap().unwrap();
        }
    }

    #[test]
    fn update_fdt_match_some() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 256], [0xBB; 16]),
            TestBlob(strs[1], vec![0xBB; 128], [0xCC; 16]),
            TestBlob(strs[2], vec![0xCC; 64], [0xBB; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        parse_reserved_mem(fdt, &bytes, &[0xBB; 16]).unwrap();
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
            TestBlob(strs[0], vec![0xAA; 256], [0xAA; 16]),
            TestBlob(strs[1], vec![0xBB; 128], [0xAA; 16]),
            TestBlob(strs[2], vec![0xCC; 64], [0xAA; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        parse_reserved_mem(fdt, &bytes, &[0x00; 16]).unwrap();
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
            TestBlob(strs[0], vec![0xAA; 256], [0xAA; 16]),
            TestBlob(strs[1], vec![0xBB; 128], [0xAA; 16]),
            TestBlob(strs[2], vec![0xCC; 64], [0xAA; 16]),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let mut fdt_data = vec![0u8; 1024];
        let fdt = Fdt::create_empty_tree(&mut fdt_data).unwrap();

        assert_eq!(parse_reserved_mem(fdt, &bytes, &[0xAA; 16]), Err(Error::MissingNode));
    }
}
