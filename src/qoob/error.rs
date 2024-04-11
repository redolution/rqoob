use std::convert::From;
use std::error::Error;
use std::fmt;

use hidapi::HidError;

#[derive(Debug)]
pub enum QoobError {
	NoDev,
	MultipleDevs,
	PartialTransfer {
		transferred: usize,
		requested: usize,
	},
	BusBusy,
	HidError(HidError),
}

impl fmt::Display for QoobError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NoDev => write!(f, "Device not found"),
			Self::MultipleDevs => write!(f, "Multiple devices are connected, can't choose one"),
			Self::PartialTransfer {
				transferred,
				requested,
			} => {
				write!(
					f,
					"Partial transfer: {transferred} out of {requested} bytes transferred",
				)
			}
			Self::BusBusy => write!(f, "Bus busy, try again later"),
			Self::HidError(e) => write!(f, "{e}"),
		}
	}
}

impl From<HidError> for QoobError {
	fn from(error: HidError) -> Self {
		Self::HidError(error)
	}
}

impl Error for QoobError {}

pub type QoobResult<T> = Result<T, QoobError>;
