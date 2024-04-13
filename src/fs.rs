use std::collections::HashMap;

use crate::device;
use crate::QoobDevice;
use crate::QoobResult;

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
		(self.size() + device::SECTOR_SIZE - 1) / device::SECTOR_SIZE
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

	pub fn into_device(self) -> QoobDevice {
		self.dev
	}
}
