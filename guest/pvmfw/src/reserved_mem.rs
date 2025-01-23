use libfdt::Fdt;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    InconsistentSize,
    InvalidOffset,
    InvalidTotalSize,
    InvalidCStr,
    InvalidConfig,
    MissingNode,
    AppendFailure,
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
#[derive(Debug, Eq, FromBytes, Immutable, KnownLayout, PartialEq)]
pub(crate) struct ConfigHeader<'a> {
    hdrs: &'a [RMemHeader],
    buffer: &'a [u8],
}

impl<'a> ConfigHeader<'a> {
    fn new(buffer: &'a [u8]) -> Result<Self, Error> {
        let (count, rem) = u32::read_from_prefix(buffer).map_err(|_| Error::InvalidConfig)?;
        let (hdrs, buffer) = <[RMemHeader]>::ref_from_prefix_with_elems(rem, count as usize)
            .map_err(|_| Error::InconsistentSize)?;

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
            // compat strings should end with a null byte
            if i > 0 && buffer[hdr.blob_offset() - 1] != b'\0' {
                return Err(Error::InvalidCStr);
            }
        }

        // compat should always come after blob
        //if hdrs.iter().any(|hdr| hdr.blob_offset() >= hdr.compat_offset()) {
        //return Err(Error::InvalidOffset);
        //}

        // check that blob size is consistent
        //if hdrs.iter().any(|hdr| hdr.compat_offset() - hdr.blob_offset() != hdr.blob_size()) {
        //return Err(Error::InvalidConfig);
        //}

        // buffer should be at least as long as the last compat_offset plus a null byte.
        if buffer.len() <= hdrs.iter().map(|hdr| hdr.compat_offset()).max().unwrap_or(0) {
            return Err(Error::InvalidTotalSize);
        }

        //for i in 0..hdrs.len() - 1 {
        //if hdrs[i].blob_offset() + hdrs[i].blob_size() > hdrs[i + 1].blob_offset() {
        //return Err(Error::InvalidOffset);
        //}
        //}

        //let _blob_sizes_total: usize = hdrs.iter().map(|hdr| hdr.blob_size()).sum();
        Ok(Self { hdrs, buffer })
    }
}

impl<'a> IntoIterator for &'a ConfigHeader<'_> {
    type Item = (&'a RMemHeader, &'a [u8]);
    type IntoIter = ConfigHeaderIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ConfigHeaderIterator { hdr: self, next: 0 }
    }
}

pub(crate) struct ConfigHeaderIterator<'a> {
    hdr: &'a ConfigHeader<'a>,
    next: usize,
}

impl<'a> Iterator for ConfigHeaderIterator<'a> {
    type Item = (&'a RMemHeader, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next < self.hdr.hdrs.len() {
            let hdr = &self.hdr.hdrs[self.next];
            self.next += 1;
            Some((hdr, self.hdr.buffer))
        } else {
            None
        }
    }
}

pub fn parse_reserved_mem(fdt: &mut Fdt, config: &[u8]) -> Result<(), Error> {
    let cfg_header = &ConfigHeader::new(config)?;

    for (hdr, blob) in cfg_header.into_iter() {
        let mem_node = fdt
            .node_mut(c"/reserved-memory")
            .map_err(|_| Error::MissingNode)?
            .ok_or(Error::MissingNode)?;

        let compat = core::ffi::CStr::from_bytes_until_nul(&blob[hdr.compat_offset()..])
            .map_err(|_| Error::InvalidCStr)?;
        let mut node = mem_node.add_subnode(compat).map_err(|_| Error::AppendFailure)?;

        node.appendprop(c"compatible", &compat.to_bytes_with_nul())
            .map_err(|_| Error::AppendFailure)?;
        node.appendprop_addrrange(
            c"reg",
            blob[hdr.blob_offset()..].as_ptr() as u64,
            hdr.blob_size as u64,
        )
        .map_err(|_| Error::AppendFailure)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::CStr;

    const FDT_WITHOUT_DEVICE_FILE_PATH: &str = "test_pvmfw_devices_without_device.dtb";

    struct TestHeader<const N: usize> {
        size: u32,
        hdrs: [RMemHeader; N],
    }

    struct TestBlob<'a>(&'a CStr, Vec<u8>);

    fn make_test_data<const N: usize>(data: &[TestBlob<'_>; N]) -> (TestHeader<N>, Vec<u8>) {
        let mut out_hdr = TestHeader { size: N as u32, hdrs: [Default::default(); N] };

        let mut offset = 0usize;
        let mut out_data = vec![0u8; 0];
        for (i, d) in data.iter().enumerate() {
            let blob_offset = offset as u32;
            let blob_size = d.1.len() as u32;
            offset += d.1.len();
            let compat_offset = offset as u32;
            offset += d.0.to_bytes_with_nul().len();
            out_hdr.hdrs[i] = make_header(i as u8, blob_offset, blob_size, compat_offset);

            out_data.extend_from_slice(d.1.as_slice());
            out_data.extend_from_slice(d.0.to_bytes_with_nul());
        }
        (out_hdr, out_data)
    }

    fn make_header(uuid: u8, blob_offset: u32, blob_size: u32, compat_offset: u32) -> RMemHeader {
        RMemHeader { vm_uuid: [uuid; 16], blob_offset, blob_size, compat_offset, flags: 0 }
    }

    #[test]
    fn success() {
        let blobs = [TestBlob(c"Foo", vec![0xAA; 256]), TestBlob(c"Bar", vec![0xBB; 256])];
        let (hdr, mut data) = make_test_data(&blobs);

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert!(cfg_header.is_ok());
    }

    #[test]
    fn bad_header_size_too_large() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256]),
            TestBlob(c"Bar", vec![0xBB; 17]),
            TestBlob(c"Baz", vec![0xCC; 64]),
        ];
        let (mut hdr, mut data) = make_test_data(&blobs);

        // Set incorrect size.
        hdr.size = 23;

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InconsistentSize));
    }

    #[test]
    fn bad_header_size_too_small() {
        let blobs = [TestBlob(c"Foo", vec![0xAA; 256]), TestBlob(c"Bar", vec![0xBB; 256])];
        let (mut hdr, mut data) = make_test_data(&blobs);

        // Set incorrect size.
        hdr.size = 1;

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InconsistentSize));
    }

    #[test]
    fn bad_blob_offset() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256]),
            TestBlob(c"Bar", vec![0xBB; 17]),
            TestBlob(c"Baz", vec![0xCC; 64]),
        ];
        let (mut hdr, mut data) = make_test_data(&blobs);

        // Set blob size to 0.
        hdr.hdrs[1].blob_size = 0;
        hdr.hdrs[1].blob_offset = hdr.hdrs[1].compat_offset + 1;

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn bad_cstr() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256]),
            TestBlob(c"Bar", vec![0xBB; 17]),
            TestBlob(c"abcdefg", vec![0xCC; 3]),
            TestBlob(c"Baz", vec![0xDD; 64]),
        ];
        let (hdr, mut data) = make_test_data(&blobs);

        // Set blob size.
        let offset = hdr.hdrs[2].compat_offset as usize;
        let offset = offset + blobs[2].0.count_bytes();
        assert_eq!(data[offset], b'\0');
        data[offset] = b'A';

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidCStr));
    }

    #[test]
    fn blob_size_past_compat() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256]),
            TestBlob(c"Bar", vec![0xBB; 17]),
            TestBlob(c"Baz", vec![0xCC; 64]),
        ];
        let (mut hdr, mut data) = make_test_data(&blobs);

        // Set blob size.
        hdr.hdrs[1].blob_size = 267;

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn bad_total_size() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256]),
            TestBlob(c"Bar", vec![0xBB; 17]),
            TestBlob(c"Baz", vec![0xCC; 64]),
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
            TestBlob(c"Foo", vec![1; 0xAA]),
            TestBlob(c"Bar", vec![1; 0xBB]),
            TestBlob(c"Baz", vec![1; 0xCC]),
        ];
        let (mut hdr, mut data) = make_test_data(&blobs);

        // Set blob 0 to run over blob 1. Update blob 1 size so total data size is still valid.
        hdr.hdrs[0].blob_size = 270;
        hdr.hdrs[0].compat_offset = hdr.hdrs[0].blob_offset + hdr.hdrs[0].blob_size;
        hdr.hdrs[1].blob_size = 3;
        hdr.hdrs[1].compat_offset = hdr.hdrs[1].blob_offset + hdr.hdrs[1].blob_size;

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let cfg_header = ConfigHeader::new(bytes.as_slice());
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
    }

    #[test]
    fn update_fdt() {
        let blobs = [
            TestBlob(c"google,early-entropy", vec![0xAA; 256]),
            TestBlob(c"google,session-key-seed", vec![0xBB; 128]),
            TestBlob(c"google,auth-token-key-seed", vec![0xCC; 64]),
        ];
        //let blobs = [TestBlob(c"Foo", vec![0xAA; 256]), TestBlob(c"Bar", vec![0xBB; 256])];
        let (hdr, mut data) = make_test_data(&blobs);

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let mut fdt_data = std::fs::read(FDT_WITHOUT_DEVICE_FILE_PATH).unwrap();
        fdt_data.resize(4096, 0);
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        fdt.unpack().unwrap();

        //let mut new_fdt = vec![0u8; 1024];
        //let overlay = Fdt::create_empty_tree(&mut new_fdt).unwrap();
        //let root_node = overlay.root_mut();
        //let mut node = root_node.add_subnode(c"fragment@reserved-memory").unwrap();
        //node.appendprop(c"target-path", &c"/".to_bytes_with_nul()).unwrap();
        //let node = node.add_subnode(c"__overlay__").unwrap();
        //let mut node = node.add_subnode(c"reserved-memory").unwrap();
        //node.appendprop(c"#address-cells", &2u32.to_be_bytes()).unwrap();
        //node.appendprop(c"#size-cells", &2u32.to_be_bytes()).unwrap();
        //node.setprop_empty(c"ranges").unwrap();

        parse_reserved_mem(fdt, &bytes).unwrap();
        //log::error!("overlay {:?}", overlay);
        // SAFETY: Panic on error. This is a test.
        //unsafe { fdt.apply_overlay(overlay).unwrap() };
        fdt.pack().unwrap();
    }

    #[test]
    fn missing_reserved_mem() {
        let blobs = [
            TestBlob(c"google,early-entropy", vec![0xAA; 256]),
            TestBlob(c"google,session-key-seed", vec![0xBB; 128]),
            TestBlob(c"google,auth-token-key-seed", vec![0xCC; 64]),
        ];
        //let blobs = [TestBlob(c"Foo", vec![0xAA; 256]), TestBlob(c"Bar", vec![0xBB; 256])];
        let (hdr, mut data) = make_test_data(&blobs);

        let mut bytes: Vec<u8> = vec![];
        bytes.extend_from_slice(hdr.size.as_bytes());
        bytes.extend_from_slice(hdr.hdrs.as_bytes());
        bytes.append(&mut data);

        let mut fdt_data = vec![0u8; 1024];
        let fdt = Fdt::create_empty_tree(&mut fdt_data).unwrap();

        assert_eq!(parse_reserved_mem(fdt, &bytes), Err(Error::MissingNode));
    }
}
