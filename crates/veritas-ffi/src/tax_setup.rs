//! What the tax summary counts: each category's bucket and what each tag
//! means (R-REP-8, docs/finance/tax-buckets.md TAX-1 to TAX-3).

use veritas_domain::BudgetData;
use veritas_session::Backend;
use veritas_session::tax::{self, TagMeaning};

use crate::FfiError;
use crate::reports::{TaxBucket, tax_bucket};
use crate::session::{Core, Session};

/// What a tag means on the tax summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TaxTagMeaning {
	Bucket {
		bucket: TaxBucket,
	},
	/// Takes a row out of its category's bucket, whatever its other tags say
	/// (TAX-3.3.1).
	Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaxCategoryBucket {
	pub category_id: String,
	pub bucket: Option<TaxBucket>,
	/// The bucket counts the other way from the category, such as Charitable
	/// giving on an income category, so its rows count as negative (TAX-4).
	/// Allowed; the shell says so in a plain note.
	pub opposite_direction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaxTagMapping {
	/// Without the `#`, case-sensitive.
	pub tag: String,
	/// `nil` is a stored bucket id Veritas doesn't know: the tag counts as
	/// unmapped until it's remapped (TAX-3.6).
	pub meaning: Option<TaxTagMeaning>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TaxSetup {
	/// Every live category that has a bucket.
	pub categories: Vec<TaxCategoryBucket>,
	/// By tag, in code point order.
	pub tags: Vec<TaxTagMapping>,
}

impl From<TaxTagMeaning> for TagMeaning {
	fn from(meaning: TaxTagMeaning) -> Self {
		match meaning {
			TaxTagMeaning::Bucket { bucket } => TagMeaning::Bucket(bucket.into()),
			TaxTagMeaning::Exclude => TagMeaning::Exclude,
		}
	}
}

pub(crate) fn tax_setup(data: &BudgetData) -> TaxSetup {
	let setup = tax::tax_setup(data);
	TaxSetup {
		categories: setup
			.categories
			.into_iter()
			.filter_map(|line| {
				Some(TaxCategoryBucket {
					category_id: line.category.to_string(),
					bucket: Some(tax_bucket(line.bucket?)),
					opposite_direction: line.opposite_direction,
				})
			})
			.collect(),
		tags: setup
			.tags
			.into_iter()
			.map(|mapping| TaxTagMapping {
				tag: mapping.tag,
				meaning: mapping.meaning.map(|meaning| match meaning {
					TagMeaning::Bucket(bucket) => TaxTagMeaning::Bucket {
						bucket: tax_bucket(bucket),
					},
					TagMeaning::Exclude => TaxTagMeaning::Exclude,
				}),
			})
			.collect(),
	}
}

impl<B: Backend> Core<B> {
	pub(crate) fn tax_setup(&self) -> Result<TaxSetup, FfiError> {
		self.call(|session| Ok(tax_setup(session.data()?)))
	}
}

#[uniffi::export]
impl Session {
	/// The categories with a tax bucket and every tag mapping, for the tax
	/// setup screen.
	pub fn tax_setup(&self) -> Result<TaxSetup, FfiError> {
		self.core.tax_setup()
	}
}
