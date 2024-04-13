use std::collections::HashMap;

use crate::device;
use crate::QoobDevice;
use crate::{QoobError, QoobResult};

#[derive(Clone, Copy, Debug)]
pub enum SectorOccupancy {
	Empty,
	Unknown,
	Slot(usize),
}

#[derive(Debug)]
pub enum FileType {
	Bios,
	Background,
	Config,
	CheatDb,
	CheatEngine,
	Bin,
	Dol,
	Elf,
	Swiss,
	Unknown([u8; 4]),
}

impl FileType {
	fn from_magic(magic: &[u8; 4]) -> Self {
		match magic {
			b"(C) " => Self::Bios,
			b"QPIC" => Self::Background,
			b"QCFG" => Self::Config,
			b"QCHT" => Self::CheatDb,
			b"QCHE" => Self::CheatEngine,
			b"BIN\0" => Self::Bin,
			b"DOL\0" => Self::Dol,
			b"ELF\0" => Self::Elf,
			b"SWIS" => Self::Swiss,
			_ => Self::Unknown(*magic),
		}
	}
}

pub const HEADER_SIZE: usize = 256;

pub struct Header([u8; HEADER_SIZE]);

impl Header {
	pub fn r#type(&self) -> FileType {
		FileType::from_magic(self.0[0..4].try_into().unwrap())
	}

	pub fn description(&self) -> &[u8; 244] {
		self.0[0x04..=0xF7].try_into().unwrap()
	}

	pub fn description_string(&self) -> String {
		String::from_iter(
			self.description()
				.iter()
				.take_while(|&&b| b != 0)
				.flat_map(|&b| std::ascii::escape_default(b))
				.map(|b| b as char),
		)
	}

	pub fn size(&self) -> usize {
		u32::from_be_bytes(self.0[0xFC..=0xFF].try_into().unwrap()) as usize
	}

	pub fn sector_count(&self) -> usize {
		device::size_to_sectors(self.size())
	}
}

impl std::fmt::Debug for Header {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Header")
			.field("type", &self.r#type())
			.field("description", &self.description_string())
			.field("size", &self.size())
			.field("sector_count", &self.sector_count())
			.finish()
	}
}

pub enum SlotStatus {
	Empty,
	Occupied,
	Overlap,
	Overflow,
}

pub struct QoobFs {
	dev: QoobDevice,
	sector_map: [SectorOccupancy; device::SECTOR_COUNT],
	toc: HashMap<usize, Header>,
}

impl QoobFs {
	pub fn from_device(dev: QoobDevice) -> QoobResult<Self> {
		let mut fs = Self {
			dev,
			sector_map: [SectorOccupancy::Unknown; device::SECTOR_COUNT],
			toc: HashMap::new(),
		};

		fs.scan()?;

		Ok(fs)
	}

	fn inspect_sector(&mut self, sector: usize) -> QoobResult<()> {
		let mut header = [0; HEADER_SIZE];
		self.dev.read(sector * device::SECTOR_SIZE, &mut header)?;

		if header == [0xFF; HEADER_SIZE] {
			self.sector_map[sector] = SectorOccupancy::Empty;
		} else {
			let file = Header(header);
			if !matches!(file.r#type(), FileType::Unknown(_))
				&& file.sector_count() < device::SECTOR_COUNT - sector
			{
				for i in sector..sector + file.sector_count() {
					self.sector_map[i] = SectorOccupancy::Slot(sector);
				}
				self.toc.insert(sector, file);
			} else {
				self.sector_map[sector] = SectorOccupancy::Unknown;
			}
		}
		Ok(())
	}

	pub fn scan(&mut self) -> QoobResult<()> {
		self.toc.clear();
		let mut cursor = 0;
		while cursor < device::SECTOR_COUNT {
			self.inspect_sector(cursor)?;
			cursor += match self.sector_map[cursor] {
				SectorOccupancy::Slot(n) => self.toc[&n].sector_count(),
				_ => 1,
			};
		}
		Ok(())
	}

	pub fn iter_slots(&self) -> impl Iterator<Item = &SectorOccupancy> {
		self.sector_map.iter()
	}

	pub fn slot_info(&self, slot: usize) -> Option<&Header> {
		self.toc.get(&slot)
	}

	pub fn read(&self, slot: usize) -> QoobResult<Vec<u8>> {
		let info = self.slot_info(slot).ok_or(QoobError::NoSuchFile(slot))?;
		let mut data = vec![0; info.size()];
		self.dev
			.read(slot * device::SECTOR_SIZE, data.as_mut_slice())?;
		Ok(data)
	}

	pub fn remove(&mut self, slot: usize) -> QoobResult<()> {
		let info = self.slot_info(slot).ok_or(QoobError::NoSuchFile(slot))?;
		let range = slot..slot + info.sector_count();
		self.dev.erase_range(range.clone())?;

		for i in range {
			self.sector_map[i] = SectorOccupancy::Empty;
		}
		self.toc.remove(&slot);

		Ok(())
	}

	pub fn check_dest_range(&self, range: std::ops::Range<usize>) -> SlotStatus {
		if range.end >= device::SECTOR_COUNT {
			return SlotStatus::Overflow;
		}

		let mut status = SlotStatus::Empty;
		for i in range.clone() {
			match self.sector_map[i] {
				SectorOccupancy::Empty => {}
				SectorOccupancy::Unknown => {
					status = SlotStatus::Occupied;
				}
				SectorOccupancy::Slot(i) if i == range.start => {
					status = SlotStatus::Occupied;
				}
				SectorOccupancy::Slot(_) => return SlotStatus::Overlap,
			}
		}
		status
	}

	pub fn write(&mut self, slot: usize, data: &[u8], verify: bool) -> QoobResult<()> {
		let header = validate_header(data).ok_or(QoobError::InvalidHeader)?;

		let dest_range = slot..slot + header.sector_count();
		match self.check_dest_range(dest_range.clone()) {
			SlotStatus::Empty => Ok(()),
			SlotStatus::Overflow => Err(QoobError::TooBig),
			SlotStatus::Occupied | SlotStatus::Overlap => Err(QoobError::RangeOccupied),
		}?;

		let mut data = data.to_vec();
		// The size is specified to be a multiple of 64KiB
		let new_size = u32::to_be_bytes(header.sector_count() as _);
		data[0xFC..=0xFF].copy_from_slice(&new_size);

		self.dev.write(slot * device::SECTOR_SIZE, &data)?;

		if verify {
			let mut verif_data = vec![0; data.len()];
			self.dev.read(slot * device::SECTOR_SIZE, &mut verif_data)?;
			if verif_data != data {
				return Err(QoobError::VerificationError);
			}
		}

		for i in dest_range {
			self.sector_map[i] = SectorOccupancy::Slot(slot);
		}
		let header = Header(data[0..HEADER_SIZE].try_into().unwrap());
		self.toc.insert(slot, header);

		Ok(())
	}

	pub fn into_device(self) -> QoobDevice {
		self.dev
	}
}

pub fn validate_header(data: &[u8]) -> Option<Header> {
	if data.len() < HEADER_SIZE {
		return None;
	}
	let header = Header(data[0..HEADER_SIZE].try_into().unwrap());

	let sector_count = device::size_to_sectors(data.len());
	let size_valid =
		header.size() == data.len() || header.size() == sector_count * device::SECTOR_SIZE;

	(size_valid && !matches!(header.r#type(), FileType::Unknown(_))).then_some(header)
}
