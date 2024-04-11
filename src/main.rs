use std::error::Error;

mod qoob;

fn main() -> Result<(), Box<dyn Error>> {
	let qoob = qoob::QoobDevice::connect()?;
	let mut buf = [0; 256];
	qoob.read(0, &mut buf)?;
	dbg!(&buf);

	Ok(())
}
