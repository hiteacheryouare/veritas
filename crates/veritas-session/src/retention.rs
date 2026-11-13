use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, Datelike, TimeDelta, Timelike, Utc};

use crate::snapshot::Snapshot;

/// How many hours, days and months keep a snapshot. Each tier keeps the
/// newest snapshot of each of its most recent periods that has one, so a
/// budget left closed for weeks never loses its history to the calendar.
/// Snapshots taken before an operation (import, export, version change, bulk
/// edit, rule apply, restore) are also kept for `operation_days`, whatever
/// the tiers say, so a later snapshot in the same hour can't replace them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
	pub hourly: usize,
	pub daily: usize,
	pub monthly: usize,
	pub operation_days: i64,
}

impl Default for RetentionPolicy {
	fn default() -> Self {
		RetentionPolicy {
			hourly: 48,
			daily: 30,
			monthly: 12,
			operation_days: 30,
		}
	}
}

#[derive(Clone, Copy)]
enum Tier {
	Hour,
	Day,
	Month,
}

fn period(tier: Tier, at: DateTime<Utc>) -> (i32, u32, u32, u32) {
	match tier {
		Tier::Hour => (at.year(), at.month(), at.day(), at.hour()),
		Tier::Day => (at.year(), at.month(), at.day(), 0),
		Tier::Month => (at.year(), at.month(), 0, 0),
	}
}

/// The snapshots a prune at `now` should delete, newest first. The newest
/// snapshot overall is always kept, even when every limit is zero.
pub fn expired(
	snapshots: &[Snapshot],
	policy: &RetentionPolicy,
	now: DateTime<Utc>,
) -> Vec<Snapshot> {
	let mut newest_first: Vec<&Snapshot> = snapshots.iter().collect();
	newest_first.sort_by(|a, b| (b.taken_at, &b.path).cmp(&(a.taken_at, &a.path)));

	let mut keep: BTreeSet<&Path> = BTreeSet::new();
	if let Some(newest) = newest_first.first() {
		keep.insert(&newest.path);
	}
	let window = TimeDelta::days(policy.operation_days);
	for snapshot in &newest_first {
		if !snapshot.reason.is_routine() && now - snapshot.taken_at < window {
			keep.insert(&snapshot.path);
		}
	}
	for (tier, limit) in [
		(Tier::Hour, policy.hourly),
		(Tier::Day, policy.daily),
		(Tier::Month, policy.monthly),
	] {
		let mut periods = 0;
		let mut current = None;
		for snapshot in &newest_first {
			let this = period(tier, snapshot.taken_at);
			if current == Some(this) {
				continue;
			}
			if periods == limit {
				break;
			}
			periods += 1;
			current = Some(this);
			keep.insert(&snapshot.path);
		}
	}

	newest_first
		.into_iter()
		.filter(|snapshot| !keep.contains(snapshot.path.as_path()))
		.cloned()
		.collect()
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;
	use std::path::PathBuf;

	use chrono::TimeZone;
	use proptest::prelude::*;

	use super::*;
	use crate::snapshot::SnapshotReason;

	fn snapshot(index: usize, millis: i64, reason: SnapshotReason) -> Snapshot {
		Snapshot {
			path: PathBuf::from(format!("snapshots/{index:05}.sqlite")),
			taken_at: Utc.timestamp_millis_opt(millis).unwrap(),
			reason,
		}
	}

	fn at(text: &str) -> DateTime<Utc> {
		DateTime::parse_from_rfc3339(text).unwrap().to_utc()
	}

	fn named(name: &str, taken_at: &str) -> Snapshot {
		tagged(name, taken_at, SnapshotReason::Hourly)
	}

	fn tagged(name: &str, taken_at: &str, reason: SnapshotReason) -> Snapshot {
		Snapshot {
			path: PathBuf::from(name),
			taken_at: at(taken_at),
			reason,
		}
	}

	fn paths(snapshots: Vec<Snapshot>) -> Vec<PathBuf> {
		snapshots
			.into_iter()
			.map(|snapshot| snapshot.path)
			.collect()
	}

	const NOW: &str = "2026-11-10T00:00:00Z";

	fn newest_per_period(snapshots: &[Snapshot], tier: Tier, limit: usize) -> Vec<&Snapshot> {
		let mut newest: BTreeMap<(i32, u32, u32, u32), &Snapshot> = BTreeMap::new();
		for snapshot in snapshots {
			let key = period(tier, snapshot.taken_at);
			let slot = newest.entry(key).or_insert(snapshot);
			if (snapshot.taken_at, &snapshot.path) > (slot.taken_at, &slot.path) {
				*slot = snapshot;
			}
		}
		newest.into_values().rev().take(limit).collect()
	}

	#[test]
	fn keeps_the_newest_snapshot_of_each_hour_day_and_month() {
		// R-REL-3
		let snapshots = [
			named("a", "2026-11-09T20:50:00Z"),
			named("b", "2026-11-09T20:10:00Z"),
			named("c", "2026-11-09T19:30:00Z"),
			named("d", "2026-11-08T09:00:00Z"),
			named("e", "2026-11-08T08:00:00Z"),
			named("f", "2026-10-02T12:00:00Z"),
			named("g", "2026-10-01T12:00:00Z"),
		];
		let policy = RetentionPolicy {
			hourly: 2,
			daily: 2,
			monthly: 2,
			operation_days: 30,
		};
		let gone = paths(expired(&snapshots, &policy, at(NOW)));
		assert_eq!(gone, [PathBuf::from("b"), "e".into(), "g".into()]);
	}

	#[test]
	fn the_newest_snapshot_survives_a_zero_policy() {
		let snapshots = [
			named("old", "2026-01-01T00:00:00Z"),
			named("new", "2026-01-02T00:00:00Z"),
		];
		let policy = RetentionPolicy {
			hourly: 0,
			daily: 0,
			monthly: 0,
			operation_days: 0,
		};
		let gone = expired(&snapshots, &policy, at(NOW));
		assert_eq!(gone.len(), 1);
		assert_eq!(gone[0].path, PathBuf::from("old"));
	}

	#[test]
	fn a_snapshot_before_an_operation_outlives_its_hour_for_thirty_days() {
		// R-REL-3: the pre-import copy survives a later hourly snapshot.
		let snapshots = [
			named("hourly", "2026-11-09T20:40:00Z"),
			tagged(
				"before-import",
				"2026-11-09T20:05:00Z",
				SnapshotReason::Import,
			),
			named("routine", "2026-11-09T20:01:00Z"),
		];
		let policy = RetentionPolicy::default();
		assert_eq!(
			paths(expired(&snapshots, &policy, at(NOW))),
			[PathBuf::from("routine")]
		);
		assert_eq!(
			paths(expired(&snapshots, &policy, at("2026-12-10T00:00:00Z"))),
			[PathBuf::from("before-import"), "routine".into()]
		);
	}

	proptest! {
		// R-REL-3: random snapshot histories never lose the newest snapshot of
		// a kept period or a recent pre-operation snapshot, never keep more
		// than the limits allow, and a second prune deletes nothing more.
		#[test]
		fn pruning_never_loses_the_newest_of_any_tier(
			taken in prop::collection::vec(
				(1_700_000_000_000i64..1_800_000_000_000, prop::sample::select(SnapshotReason::ALL.to_vec())),
				0..300,
			),
			hourly in 0usize..60,
			daily in 0usize..40,
			monthly in 0usize..15,
			operation_days in 0i64..60,
			now in 1_700_000_000_000i64..1_850_000_000_000,
		) {
			let snapshots: Vec<Snapshot> = taken
				.iter()
				.enumerate()
				.map(|(index, &(millis, reason))| snapshot(index, millis, reason))
				.collect();
			let now = Utc.timestamp_millis_opt(now).unwrap();
			let policy = RetentionPolicy { hourly, daily, monthly, operation_days };
			let gone = expired(&snapshots, &policy, now);
			let gone_paths: BTreeSet<&Path> = gone.iter().map(|s| s.path.as_path()).collect();
			let kept: Vec<Snapshot> = snapshots
				.iter()
				.filter(|s| !gone_paths.contains(s.path.as_path()))
				.cloned()
				.collect();

			prop_assert_eq!(gone_paths.len(), gone.len());
			prop_assert_eq!(kept.len() + gone.len(), snapshots.len());

			if let Some(newest) = newest_per_period(&snapshots, Tier::Hour, 1).first() {
				prop_assert!(!gone_paths.contains(newest.path.as_path()));
			}
			for (tier, limit) in [(Tier::Hour, hourly), (Tier::Day, daily), (Tier::Month, monthly)] {
				for newest in newest_per_period(&snapshots, tier, limit) {
					prop_assert!(!gone_paths.contains(newest.path.as_path()));
				}
			}

			let recent_operations: Vec<&Snapshot> = snapshots
				.iter()
				.filter(|s| !s.reason.is_routine() && now - s.taken_at < TimeDelta::days(operation_days))
				.collect();
			for snapshot in &recent_operations {
				prop_assert!(!gone_paths.contains(snapshot.path.as_path()));
			}

			let allowed = (hourly + daily + monthly + recent_operations.len())
				.max(1)
				.min(snapshots.len());
			prop_assert!(kept.len() <= allowed);

			prop_assert!(expired(&kept, &policy, now).is_empty());
		}
	}
}
