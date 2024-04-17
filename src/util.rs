pub trait ProgressBarFactory {
	type BarType: ProgressBar;
	fn create(&self, len: usize, msg: &'static str, unit: Option<&'static str>) -> Self::BarType;
}

pub trait ProgressBar {
	fn inc(&self, n: usize);
	fn set(&self, n: usize);
	fn finish(&self);
}

impl ProgressBarFactory for () {
	type BarType = ();
	fn create(
		&self,
		_len: usize,
		_msg: &'static str,
		_unit: Option<&'static str>,
	) -> Self::BarType {
	}
}

impl ProgressBar for () {
	fn inc(&self, _n: usize) {}
	fn set(&self, _n: usize) {}
	fn finish(&self) {}
}
