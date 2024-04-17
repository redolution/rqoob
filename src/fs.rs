use std::collections::HashMap;

use crate::device;
use crate::util::{ProgressBar, ProgressBarFactory as PBF};
use crate::QoobDevice;
use crate::{QoobError, QoobResult};

#[derive(Clone, Copy, Debug)]
/// Describes the contents of a sector
pub enum SectorOccupancy {
	/// Sector is blank
	Empty,
	/// Sector is not blank, but the contents could not be identified
	Unknown,
	/// Sector contains a file starting in a given sector
	Slot(usize),
}

#[derive(Debug)]
/// Known file types
pub enum FileType {
	Bios,
	/// An MPEG-1 I-Frame
	///
	/// Used by the original Qoob BIOS
	Background,
	/// Used by the original Qoob BIOS
	Config,
	/// Used by the original Qoob BIOS
	CheatDb,
	/// Used by the original Qoob BIOS
	CheatEngine,
	/// Unused, but specified by the Qoob NFO
	Bin,
	/// Unused, but specified by the Qoob NFO
	Dol,
	/// Can be an ELF or a DOL
	///
	/// Used by the original Qoob BIOS
	Elf,
	/// Used by Swiss to store arbitrary data
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

	pub fn str(&self) -> &'static str {
		match self {
			Self::Bios => "BIOS",
			Self::Background => "QPIC",
			Self::Config => "QCFG",
			Self::CheatDb => "QCHT",
			Self::CheatEngine => "QCHE",
			Self::Bin => "BIN",
			Self::Dol => "DOL",
			Self::Elf => "ELF",
			Self::Swiss => "Swiss",
			Self::Unknown(_) => "???",
		}
	}
}

/// Size of a Qoob file header
pub const HEADER_SIZE: usize = 256;

/// Newtype for Qoob file headers with accessors
pub struct Header([u8; HEADER_SIZE]);

impl Header {
	/// Returns the file type
	pub fn r#type(&self) -> FileType {
		FileType::from_magic(self.0[0..4].try_into().unwrap())
	}

	/// The raw description field
	pub fn description(&self) -> &[u8; 244] {
		self.0[0x04..=0xF7].try_into().unwrap()
	}

	/// The description field as an escaped string
	pub fn description_string(&self) -> String {
		String::from_iter(
			self.description()
				.iter()
				.take_while(|&&b| b != 0)
				.flat_map(|&b| std::ascii::escape_default(b))
				.map(|b| b as char),
		)
	}

	/// The size in bytes
	pub fn size(&self) -> usize {
		u32::from_be_bytes(self.0[0xFC..=0xFF].try_into().unwrap()) as usize
	}

	/// How many sectors the file spans
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

/// The result of a pre-write range check
pub enum RangeCheck {
	/// The destination range is blank
	Empty,
	/// The destination range is occupied by a single file at its start
	Occupied,
	/// The destination range is obstructed by another file
	Overlap,
	/// The destination range overflows flash
	Overflow,
}

/// A wrapper for [`QoobDevice`] that's aware of the "filesystem"
///
/// This API uses sectors as the addressing unit
pub struct QoobFs {
	dev: QoobDevice,
	sector_map: [SectorOccupancy; device::SECTOR_COUNT],
	toc: HashMap<usize, Header>,
}

impl QoobFs {
	/// Initialize the filesystem wrapper
	pub fn from_device(dev: QoobDevice, pbf: &impl PBF) -> QoobResult<Self> {
		let mut fs = Self {
			dev,
			sector_map: [SectorOccupancy::Unknown; device::SECTOR_COUNT],
			toc: HashMap::new(),
		};

		fs.scan(pbf)?;

		Ok(fs)
	}

	fn inspect_sector(&mut self, sector: usize) -> QoobResult<()> {
		let mut header = [0; HEADER_SIZE];
		self.dev
			.read_raw(sector * device::SECTOR_SIZE, &mut header, &())?;

		if header == [0xFF; HEADER_SIZE] {
			self.sector_map[sector] = SectorOccupancy::Empty;
		} else {
			let file = Header(header);
			if !matches!(file.r#type(), FileType::Unknown(_))
				&& file.size() >= HEADER_SIZE
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

	/// Trigger a rescan of slot headers
	pub fn scan(&mut self, pbf: &impl PBF) -> QoobResult<()> {
		let pb = pbf.create(device::SECTOR_COUNT, "Scanning", Some(" sectors"));
		self.toc.clear();
		self.dev.get_bus()?;
		let mut cursor = 0;
		while cursor < device::SECTOR_COUNT {
			self.inspect_sector(cursor)?;
			cursor += match self.sector_map[cursor] {
				SectorOccupancy::Slot(n) => self.toc[&n].sector_count(),
				_ => 1,
			};
			pb.set(cursor);
		}
		self.dev.release_bus()?;
		pb.finish();
		Ok(())
	}

	/// Iterate over sectors, returning their occupancy status
	pub fn iter_slots(&self) -> impl Iterator<Item = &SectorOccupancy> {
		self.sector_map.iter()
	}

	/// Get the header for a slot
	pub fn slot_info(&self, slot: usize) -> QoobResult<&Header> {
		self.toc.get(&slot).ok_or(QoobError::NoSuchFile(slot))
	}

	/// Read a file
	pub fn read(&self, slot: usize, pbf: &impl PBF) -> QoobResult<Vec<u8>> {
		let info = self.slot_info(slot)?;
		let mut data = vec![0; info.sector_count() * device::SECTOR_SIZE];
		self.dev
			.read(slot * device::SECTOR_SIZE, data.as_mut_slice(), pbf)?;
		Ok(data)
	}

	/// Erase a file
	pub fn remove(&mut self, slot: usize, pbf: &impl PBF) -> QoobResult<()> {
		let info = self.slot_info(slot)?;
		let range = slot..slot + info.sector_count();
		self.dev.erase(range.clone(), pbf)?;

		for i in range {
			self.sector_map[i] = SectorOccupancy::Empty;
		}
		self.toc.remove(&slot);

		Ok(())
	}

	/// Check whether it's possible to write to a given range
	pub fn check_dest_range(&self, range: std::ops::Range<usize>) -> RangeCheck {
		if range.end >= device::SECTOR_COUNT {
			return RangeCheck::Overflow;
		}

		let mut status = RangeCheck::Empty;
		for i in range.clone() {
			match self.sector_map[i] {
				SectorOccupancy::Empty => {}
				SectorOccupancy::Unknown => {
					status = RangeCheck::Occupied;
				}
				SectorOccupancy::Slot(i) if i == range.start => {
					status = RangeCheck::Occupied;
				}
				SectorOccupancy::Slot(_) => return RangeCheck::Overlap,
			}
		}
		status
	}

	/// Write a new file, optionally verifying the written data
	pub fn write(
		&mut self,
		slot: usize,
		data: &[u8],
		verify: bool,
		pbf: &impl PBF,
	) -> QoobResult<()> {
		let header = validate_header(data).ok_or(QoobError::InvalidHeader)?;

		let dest_range = slot..slot + header.sector_count();
		match self.check_dest_range(dest_range.clone()) {
			RangeCheck::Empty => Ok(()),
			RangeCheck::Overflow => Err(QoobError::TooBig),
			RangeCheck::Occupied | RangeCheck::Overlap => Err(QoobError::RangeOccupied),
		}?;

		let mut data = data.to_vec();
		// The size is specified to be a multiple of 64KiB
		let new_size = u32::to_be_bytes((header.sector_count() * device::SECTOR_SIZE) as _);
		data[0xFC..=0xFF].copy_from_slice(&new_size);

		self.dev.write(slot * device::SECTOR_SIZE, &data, pbf)?;

		if verify {
			let mut verif_data = vec![0; data.len()];
			self.dev
				.read(slot * device::SECTOR_SIZE, &mut verif_data, pbf)?;
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

	/// Retrieve the underlying device handle
	pub fn into_device(self) -> QoobDevice {
		self.dev
	}
}

/// Validate a file header
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
