use std::error::Error;

use rqoob::QoobDevice;
use rqoob::QoobFs;

fn main() -> Result<(), Box<dyn Error>> {
	let qoob = QoobDevice::connect()?;
	let fs = QoobFs::from_device(qoob)?;
	for slot in fs.iter_slots() {
		match slot {
			rqoob::fs::SectorOccupancy::Slot(n) => {
				dbg!(fs.slot_info(*n));
			}
			_ => {
				dbg!(slot);
			}
		}
	}

	Ok(())
}
