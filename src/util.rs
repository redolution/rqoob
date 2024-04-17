pub trait ProgressBarFactory {
	type BarType: ProgressBar;
	fn create(&self, len: usize) -> Self::BarType;
}

pub trait ProgressBar {
	fn inc(&self, n: usize);
	fn set(&self, n: usize);
}

impl ProgressBarFactory for () {
	type BarType = ();
	fn create(&self, _len: usize) -> Self::BarType {}
}

impl ProgressBar for () {
	fn inc(&self, _n: usize) {}
	fn set(&self, _n: usize) {}
}
