use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use rqoob::device;
use rqoob::fs;
use rqoob::util::{ProgressBar, ProgressBarFactory};
use rqoob::QoobDevice;
use rqoob::QoobError;
use rqoob::QoobFs;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// List flash contents
	List,
	/// Dump a file from flash
	Read {
		/// The slot to read from
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		slot: i64,
		/// The destination file
		file: PathBuf,
	},
	/// Remove a file from flash
	Remove {
		/// The slot to wipe
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		slot: i64,
	},
	/// Write a file to flash
	Write {
		/// The destination slot
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		slot: i64,
		/// The source file
		file: PathBuf,
		/// Overwrite an existing file in the slot
		#[arg(long)]
		overwrite: bool,
		/// Read back the written data and check that it matches
		#[arg(long)]
		verify: bool,
	},
	/// Operate on raw flash sectors
	Raw {
		#[command(subcommand)]
		command: RawCommands,
	},
}

#[derive(Subcommand)]
enum RawCommands {
	/// Dump sectors
	Read {
		/// The first sector to dump
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		start: i64,
		/// The last sector to dump (inclusive)
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		end: i64,
		/// The destination file
		file: PathBuf,
	},
	/// Erase sectors
	Erase {
		/// The first sector to erase
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		start: i64,
		/// The last sector to erase (inclusive)
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		end: i64,
	},
	/// Write sectors (does not pre-erase)
	Write {
		/// The first sector to write to
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		start: i64,
		/// The source file
		file: PathBuf,
	},
}

struct IndicatifProgressBarFactory;

impl ProgressBarFactory for IndicatifProgressBarFactory {
	type BarType = IndicatifProgressBar;
	fn create(&self, len: usize) -> Self::BarType {
		let pb = indicatif::ProgressBar::new(len as u64);
		pb.set_position(0);
		IndicatifProgressBar(pb)
	}
}

struct IndicatifProgressBar(indicatif::ProgressBar);

impl ProgressBar for IndicatifProgressBar {
	fn inc(&self, n: usize) {
		self.0.inc(n as u64);
	}
	fn set(&self, n: usize) {
		self.0.set_position(n as u64);
	}
	fn finish(&self) {
		self.0.finish();
	}
}

fn main() -> Result<(), Box<dyn Error>> {
	let cli = Cli::parse();

	let qoob = QoobDevice::connect()?;
	let pbf = IndicatifProgressBarFactory;

	match cli.command {
		Commands::List => {
			let fs = QoobFs::from_device(qoob, &pbf)?;

			println!("Slot Blocks Type  Description");
			for (i, &slot) in fs.iter_slots().enumerate() {
				let info = fs.slot_info(i);
				let (r#type, blocks, desc) = match slot {
					fs::SectorOccupancy::Slot(n) if n == i => {
						let info = info.unwrap();
						(
							info.r#type().str(),
							info.sector_count(),
							info.description_string(),
						)
					}
					fs::SectorOccupancy::Slot(_) => continue,
					fs::SectorOccupancy::Unknown => ("???", 1, String::from("Unknown")),
					fs::SectorOccupancy::Empty => continue,
				};
				println!("{i:>4} {blocks:>6} {type:<5} {desc}");
			}
		}
		Commands::Read { slot, file } => {
			let slot = slot as usize;
			let fs = QoobFs::from_device(qoob, &pbf)?;
			let data = fs.read(slot, &pbf)?;
			let mut file = File::create(file)?;
			file.write_all(&data)?;
		}
		Commands::Remove { slot } => {
			let slot = slot as usize;
			let mut fs = QoobFs::from_device(qoob, &pbf)?;
			fs.remove(slot, &pbf)?;
		}
		Commands::Write {
			slot,
			file,
			overwrite,
			verify,
		} => {
			let slot = slot as usize;
			let mut fs = QoobFs::from_device(qoob, &pbf)?;
			let file = File::open(file)?;
			let mut data = Vec::new();
			file.take(device::FLASH_SIZE as u64)
				.read_to_end(&mut data)?;
			if overwrite
				&& matches!(
					fs.check_dest_range(slot..slot + device::size_to_sectors(data.len())),
					fs::RangeCheck::Occupied,
				) {
				fs.remove(slot, &pbf)?;
			}
			fs.write(slot, &data, verify, &pbf)?;
		}
		Commands::Raw { command } => match command {
			RawCommands::Read { start, end, file } => {
				let start = start as usize;
				let end = end as usize;
				let size = if end >= start {
					(end - start + 1) * device::SECTOR_SIZE
				} else {
					0
				};
				let mut data = vec![0; size];
				qoob.read(start * device::SECTOR_SIZE, &mut data, &pbf)?;
				let mut file = File::create(file)?;
				file.write_all(&data)?;
			}
			RawCommands::Erase { start, end } => {
				let start = start as usize;
				let end = end as usize;
				qoob.erase(start..end + 1, &pbf)?;
			}
			RawCommands::Write { start, file } => {
				let start = start as usize;
				let avail = (device::SECTOR_COUNT - start) * device::SECTOR_SIZE;
				let mut file = File::open(file)?;
				let size = file.metadata()?.len();
				if size > avail as u64 {
					Err(QoobError::TooBig)?;
				}
				let mut data = Vec::new();
				file.read_to_end(&mut data)?;
				qoob.write(start * device::SECTOR_SIZE, &data, &pbf)?;
			}
		},
	};

	Ok(())
}
