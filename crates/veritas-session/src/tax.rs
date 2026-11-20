//! Tax buckets as the app sets them: a category's bucket and what each tag
//! means (docs/finance/tax-buckets.md TAX-1 to TAX-4). Veritas organizes
//! what the user tagged and never judges it, so no bucket is refused on a
//! category of any kind.

use veritas_domain::{
	BudgetData, CategoryId, CategoryKind, CategoryMeta, CategoryMetaRow, CoreError, Row, TaxTag,
	is_js_whitespace,
};
use veritas_reports::{Bucket, Direction};

/// The bucket id a tag maps to when it takes a row out of its category's
/// bucket (TAX-2).
const EXCLUDE: &str = "none";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagMeaning {
	Bucket(Bucket),
	/// Removes a row from its category's bucket, whatever its other tags say.
	Exclude,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryBucket {
	pub category: CategoryId,
	pub bucket: Option<Bucket>,
	/// The bucket counts the other way from the category's kind, such as
	/// Charitable giving on an income category, so its rows count as
	/// negative (TAX-4). Allowed; the app says so in a plain note.
	pub opposite_direction: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagMapping {
	pub tag: String,
	/// `None` is a stored id Veritas doesn't know, which counts as unmapped
	/// (TAX-3.6) until the user remaps it.
	pub meaning: Option<TagMeaning>,
	pub stored: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxSetup {
	/// Every live category, in no particular order.
	pub categories: Vec<CategoryBucket>,
	/// By tag, in code point order.
	pub tags: Vec<TagMapping>,
}

fn opposite(data: &BudgetData, category: &CategoryId, bucket: Bucket) -> bool {
	let income = data.category_kind(category) == CategoryKind::Income;
	(bucket.direction() == Direction::Inflow) != income
}

pub fn tax_setup(data: &BudgetData) -> TaxSetup {
	let categories = data
		.categories
		.values()
		.filter(|category| !category.tombstone)
		.map(|category| {
			let bucket = data
				.category_meta
				.get(&category.id)
				.and_then(|meta| meta.tax_bucket.as_deref())
				.and_then(Bucket::parse);
			CategoryBucket {
				category: category.id.clone(),
				bucket,
				opposite_direction: bucket
					.is_some_and(|bucket| opposite(data, &category.id, bucket)),
			}
		})
		.collect();
	let tags = data
		.tax_tags
		.iter()
		.map(|(tag, stored)| TagMapping {
			tag: tag.clone(),
			meaning: if stored == EXCLUDE {
				Some(TagMeaning::Exclude)
			} else {
				Bucket::parse(stored).map(TagMeaning::Bucket)
			},
			stored: stored.clone(),
		})
		.collect();
	TaxSetup { categories, tags }
}

/// A bucket, or `None` to clear it. `none` is never stored on a category:
/// it only means something on a tag (TAX-2).
pub fn set_category_tax_bucket(
	data: &BudgetData,
	category: &CategoryId,
	bucket: Option<Bucket>,
) -> Result<Vec<Row>, CoreError> {
	data.categories
		.get(category)
		.filter(|found| !found.tombstone)
		.ok_or_else(|| CoreError::NotFound {
			entity: "category",
			id: category.to_string(),
		})?;
	let meta = data
		.category_meta
		.get(category)
		.cloned()
		.unwrap_or(CategoryMeta {
			kind: None,
			tax_bucket: None,
			archived_on: None,
		});
	Ok(vec![Row::CategoryMeta(CategoryMetaRow {
		category: category.clone(),
		meta: CategoryMeta {
			tax_bucket: bucket.map(|bucket| bucket.id().to_owned()),
			..meta
		},
	})])
}

/// The tag as Actual's `extractTagsFromText` would read it (TAX-1): one
/// leading `#` the user typed is dropped, and what's left must be a whole
/// tag, so it can't be blank or hold whitespace or another `#`.
pub fn tag_text(typed: &str) -> Result<String, CoreError> {
	let tag = typed.strip_prefix('#').unwrap_or(typed);
	if tag.is_empty() {
		return Err(CoreError::Invalid("Type a tag.".to_owned()));
	}
	if tag.chars().any(|c| c == '#' || is_js_whitespace(c)) {
		return Err(CoreError::Invalid(
			"A tag is one word with no spaces or # inside it.".to_owned(),
		));
	}
	Ok(tag.to_owned())
}

/// Maps a tag to a bucket or to exclude, or unmaps it with `None`.
pub fn set_tax_tag(
	data: &BudgetData,
	typed: &str,
	meaning: Option<TagMeaning>,
) -> Result<Vec<Row>, CoreError> {
	let tag = tag_text(typed)?;
	let bucket = meaning.map(|meaning| match meaning {
		TagMeaning::Bucket(bucket) => bucket.id().to_owned(),
		TagMeaning::Exclude => EXCLUDE.to_owned(),
	});
	if data.tax_tags.get(&tag) == bucket.as_ref() {
		return Ok(Vec::new());
	}
	Ok(vec![Row::TaxTag(TaxTag { tag, bucket })])
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn tag_text_follows_actuals_grammar() {
		assert_eq!(tag_text("#medical").unwrap(), "medical");
		assert_eq!(tag_text("Medical").unwrap(), "Medical");
		assert!(tag_text("##medical").is_err());
		assert!(tag_text("#").is_err());
		assert!(tag_text("").is_err());
		assert!(tag_text("rx refill").is_err());
		assert!(tag_text("rx\u{A0}refill").is_err());
		assert!(tag_text("med#rx").is_err());
	}
}
