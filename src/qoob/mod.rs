mod error;
use error::{QoobError, QoobResult};

const HID_BUFFER_SIZE: usize = 65;
const DATA_TRANSFER_UNIT: usize = 63;
const MAX_TRANSFER_SIZE: usize = 0x8000;
const FLASH_SIZE: usize = 0x20_0000;

#[repr(u8)]
enum QoobCmd {
	Reset = 1,
	Erase = 2,
	Write = 3,
	Read = 4,
	Status = 5,
	Bus = 8,
}

pub struct QoobDevice {
	hid_dev: hidapi::HidDevice,
}

impl QoobDevice {
	/// Connects to the device.
	///
	/// An error is raised if more than one is connected.
	pub fn connect() -> QoobResult<Self> {
		let mut api = hidapi::HidApi::new()?;

		// Filter the list
		api.reset_devices()?;
		api.add_devices(0x03eb, 0x0001)?;

		let mut devs = api.device_list();
		let dev = devs.next().ok_or(QoobError::NoDev)?;

		if devs.next().is_some() {
			return Err(QoobError::MultipleDevs);
		}

		let qoob = Self {
			hid_dev: dev.open_device(&api)?,
		};

		qoob.get_bus()?;

		Ok(qoob)
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

	/// Queries the device's status.
	fn status(&self) -> QoobResult<[u8; HID_BUFFER_SIZE]> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Status as _;
		self.send_buffer(&buf)?;

		self.receive_buffer()
	}

	/// Resets the device.
	///
	/// Takes self by move because it will cause the connection to drop.
	///
	/// Note: does not appear to work. Maybe this only worked in development firmware?
	#[allow(dead_code)]
	pub fn reset(self) -> QoobResult<()> {
		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Reset as _;
		self.send_buffer(&buf)
	}

	fn get_bus(&self) -> QoobResult<()> {
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

	fn release_bus(&self) -> QoobResult<()> {
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

	pub fn read(&self, offset: u32, dest: &mut [u8]) -> QoobResult<()> {
		assert!(dest.len() <= MAX_TRANSFER_SIZE);
		assert!(offset as usize + dest.len() <= FLASH_SIZE);

		let mut buf = [0; HID_BUFFER_SIZE];
		buf[1] = QoobCmd::Read as _;

		buf[2] = (offset >> 16) as u8;
		buf[3] = (offset >> 8) as u8;
		buf[4] = offset as u8;

		buf[5] = (buf.len() >> 8) as u8;
		buf[6] = buf.len() as u8;

		self.send_buffer(&buf)?;

		for chunk in dest.chunks_mut(DATA_TRANSFER_UNIT) {
			let buf = self.receive_buffer()?;
			chunk.copy_from_slice(&buf[2..2 + chunk.len()]);
		}
		Ok(())
	}
}

impl Drop for QoobDevice {
	fn drop(&mut self) {
		self.release_bus();
		// TODO warn about failure
	}
}
