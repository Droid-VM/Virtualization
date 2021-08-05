use anyhow::{bail, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io;
use std::io::Read;

const CENTRAL_DIRECTORY_END_SIGNATURE: u32 = 0x06054b50;

pub struct EndOfCentralDirectory {
    pub disk_number: u16,
    pub disk_with_central_directory: u16,
    pub number_of_files_on_this_disk: u16,
    pub number_of_files: u16,
    pub central_directory_size: u32,
    pub central_directory_offset: u32,
    pub zip_file_comment: Vec<u8>,
}

impl EndOfCentralDirectory {
    fn parse<T: Read>(reader: &mut T) -> Result<EndOfCentralDirectory> {
        let magic = reader.read_u32::<LittleEndian>()?;
        if magic != CENTRAL_DIRECTORY_END_SIGNATURE {
            bail!("Invalid digital signature header");
        }
        let disk_number = reader.read_u16::<LittleEndian>()?;
        let disk_with_central_directory = reader.read_u16::<LittleEndian>()?;
        let number_of_files_on_this_disk = reader.read_u16::<LittleEndian>()?;
        let number_of_files = reader.read_u16::<LittleEndian>()?;
        let central_directory_size = reader.read_u32::<LittleEndian>()?;
        let central_directory_offset = reader.read_u32::<LittleEndian>()?;
        let zip_file_comment_length = reader.read_u16::<LittleEndian>()? as usize;
        let mut zip_file_comment = vec![0; zip_file_comment_length];
        reader.read_exact(&mut zip_file_comment)?;

        if disk_number != disk_with_central_directory {
            bail!("Support for multi-disk files is not implemented");
        }
        // TODO(jooyung): reject zip64 file
        Ok(EndOfCentralDirectory {
            disk_number,
            disk_with_central_directory,
            number_of_files_on_this_disk,
            number_of_files,
            central_directory_size,
            central_directory_offset,
            zip_file_comment,
        })
    }

    pub fn find_and_parse<T: Read + io::Seek>(
        reader: &mut T,
    ) -> Result<(EndOfCentralDirectory, u64)> {
        const HEADER_SIZE: u64 = 22;
        const BYTES_BETWEEN_MAGIC_AND_COMMENT_SIZE: u64 = HEADER_SIZE - 6;
        let file_length = reader.seek(io::SeekFrom::End(0))?;

        let search_upper_bound = file_length.saturating_sub(HEADER_SIZE + ::std::u16::MAX as u64);

        if file_length < HEADER_SIZE {
            bail!("Invalid zip header");
        }

        let mut pos = file_length - HEADER_SIZE;
        while pos >= search_upper_bound {
            reader.seek(io::SeekFrom::Start(pos as u64))?;
            if reader.read_u32::<LittleEndian>()? == CENTRAL_DIRECTORY_END_SIGNATURE {
                reader.seek(io::SeekFrom::Current(BYTES_BETWEEN_MAGIC_AND_COMMENT_SIZE as i64))?;
                let cde_start_pos = reader.seek(io::SeekFrom::Start(pos as u64))?;
                return EndOfCentralDirectory::parse(reader).map(|cde| (cde, cde_start_pos));
            }
            pos = match pos.checked_sub(1) {
                Some(p) => p,
                None => break,
            };
        }
        bail!("Could not find central directory end")
    }
}
