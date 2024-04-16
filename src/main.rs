use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use rqoob::device;
use rqoob::fs;
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

fn main() -> Result<(), Box<dyn Error>> {
	let cli = Cli::parse();

	let qoob = QoobDevice::connect()?;

	match cli.command {
		Commands::List => {
			let fs = QoobFs::from_device(qoob)?;

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
			let fs = QoobFs::from_device(qoob)?;
			let data = fs.read(slot)?;
			let mut file = File::create(file)?;
			file.write_all(&data)?;
		}
		Commands::Remove { slot } => {
			let slot = slot as usize;
			let mut fs = QoobFs::from_device(qoob)?;
			fs.remove(slot)?;
		}
		Commands::Write {
			slot,
			file,
			overwrite,
			verify,
		} => {
			let slot = slot as usize;
			let mut fs = QoobFs::from_device(qoob)?;
			let file = File::open(file)?;
			let mut data = Vec::new();
			file.take(device::FLASH_SIZE as u64)
				.read_to_end(&mut data)?;
			if overwrite
				&& matches!(
					fs.check_dest_range(slot..slot + device::size_to_sectors(data.len())),
					fs::RangeCheck::Occupied,
				) {
				fs.remove(slot)?;
			}
			fs.write(slot, &data, verify)?;
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
				qoob.read(start * device::SECTOR_SIZE, &mut data)?;
				let mut file = File::create(file)?;
				file.write_all(&data)?;
			}
			RawCommands::Erase { start, end } => {
				let start = start as usize;
				let end = end as usize;
				qoob.erase(start..end + 1)?;
			}
			RawCommands::Write { start, file } => {
				let start = start as usize;
				let avail = (device::SECTOR_COUNT - start) * device::SECTOR_SIZE;
				let mut file = File::open(file)?;
				let size = file.metadata()?.len();
				if size > avail as u64 {
					return Err(Box::new(QoobError::TooBig));
				}
				let mut data = Vec::new();
				file.read_to_end(&mut data)?;
				qoob.write(start * device::SECTOR_SIZE, &data)?;
			}
		},
	};

	Ok(())
}
