use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use veritas_domain::CoreError;
use veritas_store::AuditReport;

use crate::retention::RetentionPolicy;
use crate::session::Clock;
use crate::snapshot::{Snapshot, SnapshotReason, prune_snapshots, take_snapshot};

pub(crate) type Verify = fn(&Path) -> Result<AuditReport, CoreError>;

/// The slow half of opening a budget: the full integrity check and audit,
/// then the open snapshot. It runs on its own connections, so the budget is
/// usable while it works.
#[derive(Clone)]
pub(crate) struct OpenCheck {
	pub(crate) db: PathBuf,
	pub(crate) snapshots: PathBuf,
	pub(crate) retention: RetentionPolicy,
	pub(crate) clock: Clock,
	pub(crate) take_snapshot: bool,
	pub(crate) verify: Verify,
}

pub(crate) struct Checked {
	/// `None` when the check couldn't run or its thread died, which the
	/// session treats like damage: soundness wasn't shown.
	pub(crate) report: Option<AuditReport>,
	pub(crate) snapshot: Option<Result<Snapshot, CoreError>>,
}

pub(crate) enum Pending {
	Running(JoinHandle<Checked>),
	Done(Checked),
}

impl OpenCheck {
	fn run(self) -> Checked {
		let report = (self.verify)(&self.db).ok();
		let sound = report.as_ref().is_some_and(|report| !report.is_corrupt());
		let snapshot = (sound && self.take_snapshot).then(|| {
			let now = (self.clock)();
			let snapshot = take_snapshot(&self.db, &self.snapshots, SnapshotReason::Open, now)?;
			prune_snapshots(&self.snapshots, &self.retention, now)?;
			Ok(snapshot)
		});
		Checked { report, snapshot }
	}

	/// Starts the check on a thread, or runs it here if no thread can start.
	pub(crate) fn start(self) -> Pending {
		let fallback = self.clone();
		match thread::Builder::new()
			.name("veritas-open-check".to_owned())
			.spawn(move || self.run())
		{
			Ok(handle) => Pending::Running(handle),
			Err(_) => Pending::Done(fallback.run()),
		}
	}
}

impl Pending {
	pub(crate) fn is_finished(&self) -> bool {
		match self {
			Pending::Running(handle) => handle.is_finished(),
			Pending::Done(_) => true,
		}
	}

	pub(crate) fn wait(self) -> Checked {
		match self {
			Pending::Running(handle) => handle.join().unwrap_or(Checked {
				report: None,
				snapshot: None,
			}),
			Pending::Done(checked) => checked,
		}
	}
}
