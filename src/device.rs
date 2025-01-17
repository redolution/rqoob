use crate::util::{ProgressBar as PB, ProgressBarFactory as PBF};
use crate::{QoobError, QoobResult};

const HID_BUFFER_SIZE: usize = 65;
const DATA_TRANSFER_UNIT: usize = 63;
const MAX_TRANSFER_SIZE: usize = 32 * 1024;

/// The size of a single flash sector
pub const SECTOR_SIZE: usize = 64 * 1024;
/// The total number of sectors in flash
pub const SECTOR_COUNT: usize = 32;
/// The total size of flash ([`SECTOR_SIZE`] * [`SECTOR_COUNT`])
pub const FLASH_SIZE: usize = SECTOR_COUNT * SECTOR_SIZE;

#[repr(u8)]
enum QoobCmd {
	Reset = 1,
	Erase = 2,
	Write = 3,
	Read = 4,
	Status = 5,
	Bus = 8,
}

/// A handle to a connected Qoob
pub struct QoobDevice {
	hid_dev: hidapi::HidDevice,
}

impl QoobDevice {
	/// Connect to the device.
	///
	/// An error is raised if more than one is connected.
	pub fn connect() -> QoobResult<Self> {
		let api = hidapi::HidApi::new()?;

		// Filter the list
		let mut devs = api.device_list().filter(|info| {
			matches!(info.bus_type(), hidapi::BusType::Usb)
				&& info.vendor_id() == 0x03eb // Atmel Corp.
				&& info.product_id() == 0x0001 // Not listed in usb.ids
				&& info.manufacturer_string() == Some("QooB Team")
				&& info.product_string() == Some("QOOB Chip Pro")
		});

		let dev = devs.next().ok_or(QoobError::NoDev)?;

		if devs.next().is_some() {
			return Err(QoobError::MultipleDevs);
		}

		Ok(Self {
			hid_dev: dev.open_device(&api)?,
		})
	}

	fn send_buffer(&self, buf: &[u8; HID_BUFFER_SIZE]) -> QoobResult<()> {
		// Report ID is always 0
		assert_eq!(buf[0], 0);
		let transferred = self.hid_dev.write(buf)?;
		if transferred != buf.len() {
			Err(QoobError::PartialTransfer {
				transferred,
				requested: buf.len(),
			})
		} else {
			Ok(())
		}
	}

	fn receive_buffer(&self) -> QoobResult<[u8; HID_BUFFER_SIZE]> {
		let mut buf = [0; HID_BUFFER_SIZE];
		// Report ID is always 0
		assert_eq!(buf[0], 0);
		let transferred = self.hid_dev.get_feature_report(&mut buf)?;
		if transferred != buf.len() {
			Err(QoobError::PartialTransfer {
				transferred,
				requested: buf.len(),
			})
		} else {
			Ok(buf)
		}
	}

	/// Query the device's status.
	fn status(&self) -> QoobResult<[u8; HID_BUFFER_SIZE]> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Status as _;
		self.send_buffer(&buf)?;

		self.receive_buffer()
	}

	/// Reset the device.
	///
	/// Takes self by move because it will cause the connection to drop.
	///
	/// Note: does not appear to work. Maybe this only worked in development firmware?
	pub fn reset(self) -> QoobResult<()> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Reset as _;
		self.send_buffer(&buf)
	}

	/// Acquire some kind of lock.
	///
	/// Flash access will not work without this.
	/// This is to protect against concurrent access by the GameCube.
	/// The GC can't access flash while the bus is held.
	pub(crate) fn get_bus(&self) -> QoobResult<()> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Bus as _;
		buf[3] = 1;
		self.send_buffer(&buf)?;

		loop {
			let status = self.status()?[4];
			if status == 0 {
				return Ok(());
			}
			if status & 2 != 0 {
				return Err(QoobError::BusBusy);
			}
		}
	}

	/// Release the bus lock.
	pub(crate) fn release_bus(&self) -> QoobResult<()> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Bus as _;
		buf[3] = 0;
		self.send_buffer(&buf)?;

		loop {
			let status = self.status()?[4];
			if status == 1 {
				return Ok(());
			}
		}
	}

	/// Read up to [`MAX_TRANSFER_SIZE`] bytes from flash.
	pub(crate) fn read_raw(&self, offset: usize, dest: &mut [u8], pb: &impl PB) -> QoobResult<()> {
		assert!(dest.len() <= MAX_TRANSFER_SIZE);
		assert!(offset + dest.len() <= FLASH_SIZE);

		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Read as _;

		buf[2] = (offset >> 16) as u8;
		buf[3] = (offset >> 8) as u8;
		buf[4] = offset as u8;

		buf[5] = (dest.len() >> 8) as u8;
		buf[6] = dest.len() as u8;

		self.send_buffer(&buf)?;

		for chunk in dest.chunks_mut(DATA_TRANSFER_UNIT) {
			let buf = self.receive_buffer()?;
			chunk.copy_from_slice(&buf[2..2 + chunk.len()]);
			pb.inc(chunk.len());
		}
		Ok(())
	}

	/// Read data from flash
	pub fn read(&self, offset: usize, dest: &mut [u8], pbf: &impl PBF) -> QoobResult<()> {
		assert!(offset + dest.len() <= FLASH_SIZE);
		let pb = pbf.create(dest.len(), "Reading", None);
		self.get_bus()?;
		let mut cursor = offset;
		for chunk in dest.chunks_mut(MAX_TRANSFER_SIZE) {
			self.read_raw(cursor, chunk, &pb)?;
			cursor += chunk.len();
		}
		self.release_bus()?;
		pb.finish();
		Ok(())
	}

	/// Erase a sector
	fn erase_raw(&self, sector: usize) -> QoobResult<()> {
		assert!(sector < SECTOR_COUNT);
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Erase as _;
		buf[2] = sector as u8;
		// Presumably these are part of the offset argument,
		// but impossible to erase at an address that's not sector-aligned.
		// Regardless, the Windows flasher writes buf[3] as a 16 bit values, so let's preserve it.
		buf[3] = 0;
		buf[4] = 0;
		self.send_buffer(&buf)?;

		loop {
			let status = self.status()?[2];
			if status == 0 {
				return Ok(());
			}
		}
	}

	/// Erase a range of sectors
	pub fn erase(&self, sectors: std::ops::Range<usize>, pbf: &impl PBF) -> QoobResult<()> {
		assert!(sectors.start < SECTOR_COUNT);
		assert!(sectors.end <= SECTOR_COUNT);
		let pb = pbf.create(sectors.len(), "Erasing", Some(" sectors"));
		self.get_bus()?;
		for sector in sectors {
			self.erase_raw(sector)?;
			pb.inc(1);
		}
		self.release_bus()?;
		pb.finish();
		Ok(())
	}

	/// Write up to [`MAX_TRANSFER_SIZE`] bytes to flash.
	fn write_raw(&self, offset: usize, source: &[u8], pb: &impl PB) -> QoobResult<()> {
		assert!(source.len() <= MAX_TRANSFER_SIZE);
		assert!(offset + source.len() <= FLASH_SIZE);

		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Write as _;

		buf[2] = (offset >> 16) as u8;
		buf[3] = (offset >> 8) as u8;
		buf[4] = offset as u8;

		buf[5] = (source.len() >> 8) as u8;
		buf[6] = source.len() as u8;

		self.send_buffer(&buf)?;

		for chunk in source.chunks(DATA_TRANSFER_UNIT) {
			let mut buf = [0; HID_BUFFER_SIZE];
			buf[2..2 + chunk.len()].copy_from_slice(chunk);
			self.send_buffer(&buf)?;
			pb.inc(chunk.len());
		}
		Ok(())
	}

	/// Write data to flash
	pub fn write(&self, offset: usize, source: &[u8], pbf: &impl PBF) -> QoobResult<()> {
		assert!(offset + source.len() <= FLASH_SIZE);
		let pb = pbf.create(source.len(), "Writing", None);
		self.get_bus()?;
		let mut cursor = offset;
		for chunk in source.chunks(MAX_TRANSFER_SIZE) {
			self.write_raw(cursor, chunk, &pb)?;
			cursor += chunk.len();
		}
		self.release_bus()?;
		pb.finish();
		Ok(())
	}
}

/// How many sectors `size` would span
pub fn size_to_sectors(size: usize) -> usize {
	(size + SECTOR_SIZE - 1) / SECTOR_SIZE
}
