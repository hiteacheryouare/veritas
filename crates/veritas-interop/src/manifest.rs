use std::collections::BTreeSet;

use veritas_domain::{CoreError, ReadOnlyReason};

const MANIFEST_JSON: &str = include_str!("../manifest/actual-26.11.0.json");

/// `patchBadMigrations` renames this id before Actual checks anything.
const BAD_FILTERS_MIGRATION: i64 = 1_685_375_406_832;
const NEW_FILTERS_MIGRATION: i64 = 1_688_749_527_273;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
	pub api_version: String,
	pub additive_only_cutoff: i64,
	pub migrations: Vec<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
	ReadWrite,
	ReadOnly(ReadOnlyReason),
}

impl Manifest {
	/// The manifest this build supports, compiled in.
	pub fn supported() -> Result<Manifest, CoreError> {
		Manifest::parse(MANIFEST_JSON)
	}

	pub fn parse(json: &str) -> Result<Manifest, CoreError> {
		let broken = |what: &str| CoreError::Internal(format!("the migration manifest {what}"));
		let value: serde_json::Value =
			serde_json::from_str(json).map_err(|_| broken("isn't valid JSON"))?;
		let api_version = value["apiVersion"]
			.as_str()
			.ok_or_else(|| broken("has no apiVersion"))?
			.to_owned();
		let additive_only_cutoff = value["additiveOnlyCutoff"]
			.as_i64()
			.ok_or_else(|| broken("has no additiveOnlyCutoff"))?;
		let mut migrations = value["migrations"]
			.as_array()
			.ok_or_else(|| broken("has no migrations"))?
			.iter()
			.map(|id| {
				id.as_i64()
					.ok_or_else(|| broken("has a non-integer migration id"))
			})
			.collect::<Result<Vec<_>, _>>()?;
		migrations.sort_unstable();
		Ok(Manifest {
			api_version,
			additive_only_cutoff,
			migrations,
		})
	}

	/// R-IO-7, with Actual's own `checkDatabaseValidity` rules
	/// (docs/actual-format.md §3.7): a file Actual itself would refuse is not
	/// a budget; one with unknown post-cutoff ids is from a newer Actual and
	/// opens read-only; one with ids still to apply is from an older Actual.
	pub fn check(&self, applied: &[i64]) -> Result<Access, CoreError> {
		let applied: BTreeSet<i64> = applied
			.iter()
			.map(|&id| {
				if id == BAD_FILTERS_MIGRATION {
					NEW_FILTERS_MIGRATION
				} else {
					id
				}
			})
			.collect();
		let available: BTreeSet<i64> = self.migrations.iter().copied().collect();
		let cutoff = self.additive_only_cutoff;
		let out_of_sync = || {
			CoreError::NotAnActualBudget(
				"its migration history is inconsistent, so Actual itself would refuse to open it"
					.to_owned(),
			)
		};

		let unknown: Vec<i64> = applied.difference(&available).copied().collect();
		if unknown.iter().any(|&id| id <= cutoff) {
			return Err(out_of_sync());
		}
		let known = applied.intersection(&available).count();
		if !unknown.is_empty() && known != available.len() {
			return Err(out_of_sync());
		}
		let pre_available: Vec<i64> = available
			.iter()
			.copied()
			.filter(|&id| id <= cutoff)
			.collect();
		let pre_applied: Vec<i64> = applied
			.iter()
			.copied()
			.filter(|&id| id <= cutoff && available.contains(&id))
			.collect();
		if applied.iter().any(|&id| id > cutoff) && pre_applied.len() != pre_available.len() {
			return Err(out_of_sync());
		}
		if pre_applied.iter().zip(&pre_available).any(|(a, b)| a != b) {
			return Err(out_of_sync());
		}

		if !unknown.is_empty() {
			Ok(Access::ReadOnly(ReadOnlyReason::NewerActualVersion))
		} else if known == available.len() {
			Ok(Access::ReadWrite)
		} else {
			Err(CoreError::UpgradeInActualFirst {
				required: self.api_version.clone(),
			})
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn manifest() -> Manifest {
		Manifest::supported().unwrap()
	}

	const CUTOFF: i64 = 1_780_606_215_001;

	#[test]
	fn the_compiled_manifest_is_actual_26_11_0() {
		let manifest = manifest();
		assert_eq!(manifest.api_version, "26.11.0");
		assert_eq!(manifest.migrations.len(), 60);
		assert_eq!(manifest.additive_only_cutoff, CUTOFF);
		let pre = manifest
			.migrations
			.iter()
			.filter(|&&id| id <= CUTOFF)
			.count();
		assert_eq!(pre, 57);
	}

	#[test]
	fn an_exact_match_is_read_write_r_io_7() {
		let manifest = manifest();
		assert!(matches!(
			manifest.check(&manifest.migrations),
			Ok(Access::ReadWrite)
		));
	}

	#[test]
	fn unknown_post_cutoff_ids_open_read_only_r_io_7() {
		let manifest = manifest();
		let mut newer = manifest.migrations.clone();
		newer.push(1_790_000_000_000);
		assert!(matches!(
			manifest.check(&newer),
			Ok(Access::ReadOnly(ReadOnlyReason::NewerActualVersion))
		));
		// An id authored earlier but merged later sorts below known ids.
		let mut interleaved = manifest.migrations.clone();
		interleaved.push(1_784_000_000_000);
		assert!(matches!(
			manifest.check(&interleaved),
			Ok(Access::ReadOnly(ReadOnlyReason::NewerActualVersion))
		));
	}

	#[test]
	fn a_prefix_means_upgrade_in_actual_first_r_io_7() {
		let manifest = manifest();
		for len in [0, 1, 30, 57, 58, 59] {
			let older = &manifest.migrations[..len];
			assert!(
				matches!(
					manifest.check(older),
					Err(CoreError::UpgradeInActualFirst { ref required }) if required == "26.11.0"
				),
				"prefix of {len}"
			);
		}
	}

	#[test]
	fn a_post_cutoff_gap_is_older_not_corrupt() {
		let manifest = manifest();
		let gap: Vec<i64> = manifest
			.migrations
			.iter()
			.copied()
			.filter(|&id| id != 1_783_004_650_757)
			.collect();
		assert!(matches!(
			manifest.check(&gap),
			Err(CoreError::UpgradeInActualFirst { .. })
		));
	}

	#[test]
	fn histories_actual_refuses_are_not_budgets() {
		let manifest = manifest();
		let all = &manifest.migrations;
		let mut unknown_pre_cutoff = all.clone();
		unknown_pre_cutoff.push(1_600_000_000_000);
		let mut unknown_with_missing_known = all[..59].to_vec();
		unknown_with_missing_known.push(1_790_000_000_000);
		let mut post_cutoff_with_pre_gap = all.clone();
		post_cutoff_with_pre_gap.retain(|&id| id != 1_550_601_598_648);
		let pre_gap: Vec<i64> = all[..20]
			.iter()
			.copied()
			.filter(|&id| id != all[5])
			.collect();
		for (case, applied) in [
			("unknown pre-cutoff id", unknown_pre_cutoff),
			(
				"unknown id next to a missing known one",
				unknown_with_missing_known,
			),
			(
				"post-cutoff id next to a pre-cutoff gap",
				post_cutoff_with_pre_gap,
			),
			("pre-cutoff gap", pre_gap),
		] {
			assert!(
				matches!(
					manifest.check(&applied),
					Err(CoreError::NotAnActualBudget(_))
				),
				"{case}"
			);
		}
	}

	#[test]
	fn the_legacy_filters_id_is_patched_like_actual_does() {
		let manifest = manifest();
		let legacy: Vec<i64> = manifest
			.migrations
			.iter()
			.map(|&id| {
				if id == NEW_FILTERS_MIGRATION {
					BAD_FILTERS_MIGRATION
				} else {
					id
				}
			})
			.collect();
		assert!(matches!(manifest.check(&legacy), Ok(Access::ReadWrite)));
	}
}
