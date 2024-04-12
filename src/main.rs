use std::error::Error;

mod qoob;

fn main() -> Result<(), Box<dyn Error>> {
	let qoob = qoob::QoobDevice::connect()?;

	//qoob.erase(1)?;

	let mut buf = [0; 64];
	qoob.read(0x1_0000, &mut buf)?;
	dbg!(&buf);

	/*
	buf.iter_mut().enumerate().for_each(|(i, cell)| *cell = i as u8);
	qoob.write(0x1_0000, &buf)?;

	let mut buf = [0; 64];
	qoob.read(0x1_0000, &mut buf)?;
	dbg!(&buf);
	*/

	Ok(())
}
