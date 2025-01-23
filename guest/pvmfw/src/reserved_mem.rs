use libfdt::Fdt;
use log::error;
use zerocopy::{FromBytes, Immutable, KnownLayout};

//pub enum Error {
//Size,
//}

#[repr(C)]
#[derive(FromBytes, Immutable, KnownLayout)]
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

    fn compat_offset(&self) -> usize {
        self.compat_offset as usize
    }
}

#[repr(C)]
#[derive(FromBytes, Immutable, KnownLayout)]
pub(crate) struct ConfigHeader {
    size: u32,
    buffer: [u8],
}

impl ConfigHeader {
    fn size(&self) -> usize {
        self.size as usize
    }
}

impl<'a> IntoIterator for &'a ConfigHeader {
    type Item = (&'a RMemHeader, &'a [u8]);
    type IntoIter = ConfigHeaderIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ConfigHeaderIterator { hdr: self, next: 0 }
    }
}

pub(crate) struct ConfigHeaderIterator<'a> {
    hdr: &'a ConfigHeader,
    next: usize,
}

impl<'a> Iterator for ConfigHeaderIterator<'a> {
    type Item = (&'a RMemHeader, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let (hdrs, rem) =
            <[RMemHeader]>::ref_from_prefix_with_elems(&self.hdr.buffer, self.hdr.size()).expect(
                "indicated size should be consistent with the number of reserved memory headers",
            );
        if self.next < self.hdr.size() {
            let hdr = &hdrs[self.next];
            self.next += 1;
            let blob = rem;
            Some((hdr, blob))
        } else {
            None
        }
    }
}

pub(crate) fn parse_reserved_mem(fdt: &mut Fdt, config: &[u8]) {
    let cfg_header = &mut ConfigHeader::ref_from_bytes(config)
        .expect("config should be a valid resrved memory header");

    for (hdr, blob) in cfg_header.into_iter() {
        let mem_node = fdt
            .node_mut(c"/reserved-memory")
            .expect("device tree should have a reserved-memory node")
            .expect("device tree should have a reserved-memory node");
        error!("uuid: {:02X?}", hdr.vm_uuid);

        let compat = core::ffi::CStr::from_bytes_until_nul(&blob[hdr.compat_offset()..])
            .expect("compat should be a valid C string");
        let mut node =
            mem_node.next_compatible(compat).expect("node not found").expect("node not found");
        error!("compat: {:?}", compat);
        error!("len: {}", hdr.blob_size);

        node.setprop_addrrange_inplace(
            c"reg",
            blob[hdr.blob_offset()..].as_ptr() as u64,
            hdr.blob_size as u64,
        )
        .expect("failed to update device tree node");
    }
}
