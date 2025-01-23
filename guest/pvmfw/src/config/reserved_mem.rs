use core::ffi::CStr;
use core::fmt;
use zerocopy::byteorder::U32;
use zerocopy::{CastError, FromBytes, Immutable, IntoBytes, KnownLayout, NativeEndian};

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
    /// Input buffer was unaligned.
    UnalignedData,
    /// A header referenced an invalid blob size.
    InvalidSize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InconsistentSize => write!(f, "Header count did not match indicated number"),
            Self::InvalidOffset => write!(f, "Encountered an invalid data offset"),
            Self::InvalidCStr => write!(f, "Encountered an invalid CStr"),
            Self::InputBufferSmall => write!(f, "Input buffer too small"),
            Self::UnalignedData => write!(f, "Input buffer was not aligned"),
            Self::InvalidSize => write!(f, "An entry indicated an invalid size"),
        }
    }
}

#[repr(C)]
#[derive(
    Clone, Copy, Debug, Default, Eq, FromBytes, Immutable, IntoBytes, KnownLayout, PartialEq,
)]
pub(crate) struct ResMemEntry {
    vm_name_offset: U32<NativeEndian>,
    blob_offset: U32<NativeEndian>,
    blob_size: U32<NativeEndian>,
    compat_offset: U32<NativeEndian>,
    flags: U32<NativeEndian>,
}

impl ResMemEntry {
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
pub(crate) struct ResMem<'a> {
    entries: &'a [ResMemEntry],
    buffer: &'a [u8],
}

fn check_cstr(buffer: &[u8]) -> Result<(), Error> {
    core::ffi::CStr::from_bytes_until_nul(buffer).map(|_| ()).map_err(|_| Error::InvalidCStr)
}

impl<'a> ResMem<'a> {
    pub(crate) fn new(buffer: &'a [u8], guest_page_size: usize) -> Result<Self, Error> {
        let (count, rem) = u32::read_from_prefix(buffer).map_err(|_| Error::InputBufferSmall)?;
        let (entries, buffer) = <[ResMemEntry]>::ref_from_prefix_with_elems(rem, count as usize)
            .map_err(|e| match e {
                CastError::Alignment(_) => Error::UnalignedData,
                _ => Error::InconsistentSize,
            })?;

        let mut last_blob_end = 0;

        for e in entries.iter() {
            check_cstr(buffer.get(e.compat_offset()..).ok_or(Error::InvalidOffset)?)?;
            check_cstr(buffer.get(e.vm_name_offset()..).ok_or(Error::InvalidOffset)?)?;

            if e.blob_size() == 0 || e.blob_size() > guest_page_size {
                return Err(Error::InvalidSize);
            }

            let start = e.blob_offset();
            let end = start.checked_add(e.blob_size()).ok_or(Error::InvalidOffset)?;
            if start < last_blob_end || end > buffer.len() {
                return Err(Error::InvalidOffset);
            }
            last_blob_end = end;
        }

        Ok(Self { entries, buffer })
    }
}

impl<'a> IntoIterator for ResMem<'a> {
    type Item = ResMemEntryInfo<'a>;
    type IntoIter = ResMemIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ResMemIterator { resmem: self, next: 0 }
    }
}

pub(crate) struct ResMemIterator<'a> {
    resmem: ResMem<'a>,
    next: usize,
}

pub(crate) struct ResMemEntryInfo<'a> {
    pub blob: &'a [u8],
    pub compat: &'a CStr,
    pub vm_name: &'a CStr,
    pub flags: u32,
}

impl<'a> Iterator for ResMemIterator<'a> {
    type Item = ResMemEntryInfo<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = &self.resmem.entries.get(self.next)?;
        let compat =
            core::ffi::CStr::from_bytes_until_nul(&self.resmem.buffer[entry.compat_offset()..])
                .unwrap();
        let vm_name =
            core::ffi::CStr::from_bytes_until_nul(&self.resmem.buffer[entry.vm_name_offset()..])
                .unwrap();
        self.next += 1;
        Some(Self::Item {
            blob: &self.resmem.buffer[entry.blob_offset()..entry.blob_offset() + entry.blob_size()],
            compat,
            vm_name,
            flags: entry.flags.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::ConvertError;

    const PAGE_SIZE: usize = 4096;

    struct TestHeader<const N: usize> {
        size: u32,
        entries: [ResMemEntry; N],
    }

    struct TestBlob<'a>(&'a CStr, Vec<u8>, &'a CStr);

    fn make_test_data<const N: usize>(data: &[TestBlob<'_>; N]) -> (TestHeader<N>, Vec<u8>) {
        let mut out_hdr = TestHeader { size: N as u32, entries: [Default::default(); N] };

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
            out_hdr.entries[i] = make_header(vm_name_offset, blob_offset, blob_size, compat_offset);

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
    ) -> ResMemEntry {
        ResMemEntry {
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
        data.extend_from_slice(hdr.entries.as_bytes());
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

        assert!(ResMem::new(dealign_vec_data(&mut bytes), PAGE_SIZE).is_ok());
    }

    #[test]
    fn success() {
        let blobs = [
            TestBlob(c"Foo", vec![0xAA; 256], c"Fry"),
            TestBlob(c"Bar", vec![0xBB; 256], c"Leela"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ResMem::new(bytes.as_slice(), PAGE_SIZE);
        assert!(cfg_header.is_ok());

        for (i, entry) in cfg_header.unwrap().into_iter().enumerate() {
            assert_eq!(entry.compat, blobs[i].0);
            assert_eq!(entry.blob, blobs[i].1);
            assert_eq!(entry.vm_name, blobs[i].2);
            assert_eq!(entry.flags, 0);
        }
    }

    #[test]
    fn input_too_small() {
        let data = &[0u8; 3];
        let cfg_header = ResMem::new(data, PAGE_SIZE);
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

        let cfg_header = ResMem::new(bytes.as_slice(), PAGE_SIZE);
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
        bytes.extend_from_slice(hdr.entries.as_bytes());
        let len = data.len();
        bytes.extend_from_slice(&data[..len - 64]);

        let cfg_header = ResMem::new(bytes.as_slice(), PAGE_SIZE);
        assert_eq!(cfg_header, Err(Error::InvalidOffset));
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

        let cfg_header = ResMem::new(bytes.as_slice(), PAGE_SIZE);
        assert_eq!(cfg_header, Err(Error::InvalidSize));
    }

    #[test]
    fn size_too_big() {
        let strs =
            [c"google,early-entropy", c"google,session-key-seed", c"google,auth-token-key-seed"];
        let blobs = [
            TestBlob(strs[0], vec![0xAA; 32768], c"Fry"),
            TestBlob(strs[1], vec![0xBB; 128], c"Fry"),
            TestBlob(strs[2], vec![0xCC; 64], c"Fry"),
        ];
        let (hdr, data) = make_test_data(&blobs);

        let bytes = make_test_vec(hdr, data);

        let cfg_header = ResMem::new(bytes.as_slice(), PAGE_SIZE);
        assert_eq!(cfg_header, Err(Error::InvalidSize));
    }
}
