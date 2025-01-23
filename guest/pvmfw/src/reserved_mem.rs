use alloc::vec::Vec;
use libfdt::Fdt;
use log::error;
use zerocopy::FromBytes;

#[repr(C)]
#[derive(FromBytes)]
pub(crate) struct RMemHeader {
    vm_uuid: [u8; 16],
    blob_offset: u32,
    blob_size: u32,
    compat_offset: u32,
    flags: u32,
}

pub(crate) fn parse_reserved_mem(fdt: &mut Fdt, config: &[u8]) {
    const RMEM_HDR_SIZE: usize = core::mem::size_of::<RMemHeader>();
    let rmem_count: usize = u32::from_le_bytes(config[..4].try_into().unwrap()) as usize;
    error!("count: {}", rmem_count);

    // SAFETY: Figure out how to make this not horrifying.
    let blobs_base =
        unsafe { config.as_ptr().add(4).add(core::mem::size_of::<RMemHeader>() * rmem_count) };
    let blobs_size: usize = config.len() - core::mem::size_of::<u32>() - RMEM_HDR_SIZE * rmem_count;

    let mut hdrs: Vec<RMemHeader> = Vec::with_capacity(rmem_count);
    for i in 0..rmem_count {
        hdrs.push(RMemHeader::read_from_prefix(&config[4 + i * RMEM_HDR_SIZE..]).unwrap().0);
    }

        //?.ok_or(libfdt::FdtError::NotFound)?;
    for hdr in hdrs {
        let mem_node = fdt.node_mut(c"/reserved-memory").unwrap().unwrap();
        let rem: usize = blobs_size - hdr.compat_offset as usize;
        error!("uuid: {:02X?}", hdr.vm_uuid);

        // SAFETY: Figure out how to make this not horrifying.
        let compat: &[u8] =
            unsafe { core::slice::from_raw_parts(blobs_base.add(hdr.compat_offset as usize), rem) };
        let compat = core::ffi::CStr::from_bytes_until_nul(compat).unwrap();
        let mut node = mem_node.next_compatible(compat).unwrap().unwrap();
        error!("compat: {:?}", compat);
        error!("len: {}", hdr.blob_size);
        // SAFETY: debugging
        let blob_ptr = unsafe {blobs_base.add(hdr.blob_offset as usize)};
        error!("blob_ptr {:X?}", blob_ptr);

        node.setprop_addrrange_inplace(c"reg", blob_ptr as u64, hdr.blob_size as u64).unwrap();
    }
}
