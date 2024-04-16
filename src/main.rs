use std::error::Error;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use rqoob::device;
use rqoob::QoobDevice;
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
		slot: usize,
		/// The destination file
		file: PathBuf,
	},
	/// Remove a file from flash
	Remove {
		/// The slot to wipe
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		slot: usize,
	},
	/// Write a file to flash
	Write {
		/// The destination slot
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		slot: usize,
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
		start: usize,
		/// The last sector to dump (inclusive)
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		end: usize,
		/// The destination file
		file: PathBuf,
	},
	/// Erase sectors
	Erase {
		/// The first sector to erase
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		start: usize,
		/// The last sector to erase (inclusive)
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		end: usize,
	},
	/// Write sectors (does not pre-erase)
	Write {
		/// The first sector to write to
		#[arg(value_parser = 0..=device::SECTOR_COUNT as i64 - 1)]
		start: usize,
		/// The source file
		file: PathBuf,
	},
}

fn main() -> Result<(), Box<dyn Error>> {
	let cli = Cli::parse();

	let qoob = QoobDevice::connect()?;
	let fs = QoobFs::from_device(qoob)?;
	for slot in fs.iter_slots() {
		match slot {
			rqoob::fs::SectorOccupancy::Slot(n) => {
				dbg!(fs.slot_info(*n)?);
			}
			_ => {
				dbg!(slot);
			}
		}
	}

	Ok(())
}
