//! The assistant's category and group name matching (R-AI-1,
//! docs/finance/assistant.md AI-3.6 and AI-3.7), the one copy of the name key
//! that the core's ranking and the app's tools share.

use std::cmp::Ordering;

use crate::{FfiError, guard};

/// Lowercase letters of Latin-1 Supplement and Latin Extended-A and -B with
/// their base letters, generated once from the Unicode decompositions and
/// written out here, plus the letters that have no decomposition (ß, æ, ǣ,
/// ǽ, œ, ø, ǿ, đ, ł, ħ, ı, ŋ, þ, ð and the rest). Four entries (ɗ, ɠ, ɲ, ʈ)
/// are IPA Extensions letters, the lowercase forms of Latin Extended-B
/// capitals. Sorted by code point for binary search.
const LATIN_FOLDS: &[(char, &str)] = &[
	('ß', "ss"),
	('à', "a"),
	('á', "a"),
	('â', "a"),
	('ã', "a"),
	('ä', "a"),
	('å', "a"),
	('æ', "ae"),
	('ç', "c"),
	('è', "e"),
	('é', "e"),
	('ê', "e"),
	('ë', "e"),
	('ì', "i"),
	('í', "i"),
	('î', "i"),
	('ï', "i"),
	('ð', "d"),
	('ñ', "n"),
	('ò', "o"),
	('ó', "o"),
	('ô', "o"),
	('õ', "o"),
	('ö', "o"),
	('ø', "o"),
	('ù', "u"),
	('ú', "u"),
	('û', "u"),
	('ü', "u"),
	('ý', "y"),
	('þ', "th"),
	('ÿ', "y"),
	('ā', "a"),
	('ă', "a"),
	('ą', "a"),
	('ć', "c"),
	('ĉ', "c"),
	('ċ', "c"),
	('č', "c"),
	('ď', "d"),
	('đ', "d"),
	('ē', "e"),
	('ĕ', "e"),
	('ė', "e"),
	('ę', "e"),
	('ě', "e"),
	('ĝ', "g"),
	('ğ', "g"),
	('ġ', "g"),
	('ģ', "g"),
	('ĥ', "h"),
	('ħ', "h"),
	('ĩ', "i"),
	('ī', "i"),
	('ĭ', "i"),
	('į', "i"),
	('ı', "i"),
	('ĳ', "ij"),
	('ĵ', "j"),
	('ķ', "k"),
	('ĸ', "k"),
	('ĺ', "l"),
	('ļ', "l"),
	('ľ', "l"),
	('ŀ', "l"),
	('ł', "l"),
	('ń', "n"),
	('ņ', "n"),
	('ň', "n"),
	('ŉ', "n"),
	('ŋ', "ng"),
	('ō', "o"),
	('ŏ', "o"),
	('ő', "o"),
	('œ', "oe"),
	('ŕ', "r"),
	('ŗ', "r"),
	('ř', "r"),
	('ś', "s"),
	('ŝ', "s"),
	('ş', "s"),
	('š', "s"),
	('ţ', "t"),
	('ť', "t"),
	('ŧ', "t"),
	('ũ', "u"),
	('ū', "u"),
	('ŭ', "u"),
	('ů', "u"),
	('ű', "u"),
	('ų', "u"),
	('ŵ', "w"),
	('ŷ', "y"),
	('ź', "z"),
	('ż', "z"),
	('ž', "z"),
	('ſ', "s"),
	('ƀ', "b"),
	('ƈ', "c"),
	('ƒ', "f"),
	('ƕ', "hv"),
	('ƙ', "k"),
	('ƚ', "l"),
	('ƞ', "n"),
	('ơ', "o"),
	('ƣ', "oi"),
	('ƥ', "p"),
	('ƫ', "t"),
	('ƭ', "t"),
	('ư', "u"),
	('ƴ', "y"),
	('ƶ', "z"),
	('ǆ', "dz"),
	('ǉ', "lj"),
	('ǌ', "nj"),
	('ǎ', "a"),
	('ǐ', "i"),
	('ǒ', "o"),
	('ǔ', "u"),
	('ǖ', "u"),
	('ǘ', "u"),
	('ǚ', "u"),
	('ǜ', "u"),
	('ǟ', "a"),
	('ǡ', "a"),
	('ǣ', "ae"),
	('ǥ', "g"),
	('ǧ', "g"),
	('ǩ', "k"),
	('ǫ', "o"),
	('ǭ', "o"),
	('ǰ', "j"),
	('ǳ', "dz"),
	('ǵ', "g"),
	('ǹ', "n"),
	('ǻ', "a"),
	('ǽ', "ae"),
	('ǿ', "o"),
	('ȁ', "a"),
	('ȃ', "a"),
	('ȅ', "e"),
	('ȇ', "e"),
	('ȉ', "i"),
	('ȋ', "i"),
	('ȍ', "o"),
	('ȏ', "o"),
	('ȑ', "r"),
	('ȓ', "r"),
	('ȕ', "u"),
	('ȗ', "u"),
	('ș', "s"),
	('ț', "t"),
	('ȟ', "h"),
	('ȣ', "ou"),
	('ȥ', "z"),
	('ȧ', "a"),
	('ȩ', "e"),
	('ȫ', "o"),
	('ȭ', "o"),
	('ȯ', "o"),
	('ȱ', "o"),
	('ȳ', "y"),
	('ȴ', "l"),
	('ȵ', "n"),
	('ȶ', "t"),
	('ȷ', "j"),
	('ȼ', "c"),
	('ȿ', "s"),
	('ɀ', "z"),
	('ɇ', "e"),
	('ɉ', "j"),
	('ɋ', "q"),
	('ɍ', "r"),
	('ɏ', "y"),
	('ɗ', "d"),
	('ɠ', "g"),
	('ɲ', "n"),
	('ʈ', "t"),
];

/// Case and accents folded: Latin-1 Supplement and Latin Extended-A and -B
/// letters, and the IPA Extensions lowercase forms of Extended-B capitals, to
/// their base letters (`LATIN_FOLDS`), and combining marks
/// (U+0300 to U+036F) dropped. Other scripts are lowercased only.
pub(crate) fn fold(name: &str) -> String {
	let mut folded = String::with_capacity(name.len());
	for c in name.chars().flat_map(char::to_lowercase) {
		if ('\u{0300}'..='\u{036F}').contains(&c) {
			continue;
		}
		match LATIN_FOLDS.binary_search_by(|(letter, _)| letter.cmp(&c)) {
			Ok(index) => folded.push_str(LATIN_FOLDS[index].1),
			Err(_) => folded.push(c),
		}
	}
	folded
}

/// A name's words: folded, "&" read as "and", anything that isn't a letter
/// or digit a word break, each word made singular.
fn words(text: &str) -> Vec<String> {
	fold(text)
		.replace('&', " and ")
		.split(|c: char| !c.is_alphanumeric())
		.filter(|word| !word.is_empty())
		.map(singular)
		.collect()
}

fn singular(word: &str) -> String {
	let letters = word.chars().count();
	if letters > 4 && word.ends_with("ies") {
		format!("{}y", &word[..word.len() - 3])
	} else if letters > 3 && word.ends_with('s') && !word.ends_with("ss") {
		word[..word.len() - 1].to_owned()
	} else {
		word.to_owned()
	}
}

const ARTICLES: [&str; 3] = ["the", "my", "our"];

/// AI-3.6's name key: the words, with a leading "the", "my" or "our"
/// dropped when other words follow.
pub(crate) fn name_key(name: &str) -> String {
	let mut words = words(name);
	if words.len() > 1 && ARTICLES.contains(&words[0].as_str()) {
		words.remove(0);
	}
	words.join(" ")
}

/// A category or group a question may name.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NameCandidate {
	pub id: String,
	pub name: String,
	pub is_group: bool,
	/// A category's group, which tells two categories of one name apart.
	pub group: Option<String>,
}

/// What `matchCategoryName` found (AI-3.6, AI-3.7).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum NameMatch {
	/// Exactly one exact key match.
	Found { candidate: NameCandidate },
	/// Several exact matches, categories first: no figure, ask which one.
	Several { candidates: Vec<NameCandidate> },
	/// No exact match: at most 3 closest names, possibly none.
	NotFound { closest: Vec<String> },
}

const MAXIMUM_SUGGESTIONS: usize = 3;
const MAXIMUM_EDITS: usize = 2;

/// AI-3.6: resolves a name the user typed to one candidate, or says why not.
pub(crate) fn match_name(wanted: &str, all: &[NameCandidate]) -> NameMatch {
	let candidates: Vec<&NameCandidate> = all
		.iter()
		.filter(|c| !name_key(&c.name).is_empty())
		.collect();
	let mut words = words(wanted);
	if words.len() > 1 && ARTICLES.contains(&words[0].as_str()) {
		words.remove(0);
	}
	let mut scope = candidates.clone();
	if words.len() > 1 {
		match words.last().map(String::as_str) {
			Some("category" | "envelope") => {
				scope.retain(|c| !c.is_group);
				words.pop();
			}
			Some("group") => {
				scope.retain(|c| c.is_group);
				words.pop();
			}
			_ => {}
		}
	}
	if let Some(split) = words.iter().rposition(|word| word == "in")
		&& split > 0
		&& split < words.len() - 1
	{
		let name = words[..split].join(" ");
		let group = words[split + 1..].join(" ");
		let exact: Vec<&&NameCandidate> = scope
			.iter()
			.filter(|c| {
				!c.is_group
					&& name_key(&c.name) == name
					&& c.group.as_deref().map(name_key) == Some(group.clone())
			})
			.collect();
		if let [one] = exact.as_slice() {
			return NameMatch::Found {
				candidate: (**one).clone(),
			};
		}
	}
	let asked = words.join(" ");
	let exact: Vec<&NameCandidate> = scope
		.iter()
		.copied()
		.filter(|c| name_key(&c.name) == asked)
		.collect();
	match exact.as_slice() {
		[one] => NameMatch::Found {
			candidate: (*one).clone(),
		},
		[] => NameMatch::NotFound {
			closest: closest(&asked, &candidates),
		},
		_ => NameMatch::Several {
			candidates: exact
				.iter()
				.filter(|c| !c.is_group)
				.chain(exact.iter().filter(|c| c.is_group))
				.map(|c| (*c).clone())
				.collect(),
		},
	}
}

/// AI-3.7: names that start with, hold, or are held by what was asked; then
/// any other name at most 2 edits away. Within a tier, fewest edits first,
/// then the name ignoring case. Each name once, at most 3.
fn closest(asked: &str, candidates: &[&NameCandidate]) -> Vec<String> {
	let mut ranked: Vec<(u8, usize, String, &str)> = candidates
		.iter()
		.filter_map(|candidate| {
			let name = name_key(&candidate.name);
			if name.is_empty() {
				return None;
			}
			let edits = distance(asked, &name);
			let tier = if asked.is_empty() {
				3
			} else if name.starts_with(asked) {
				0
			} else if name.contains(asked) {
				1
			} else if asked.contains(name.as_str()) {
				2
			} else {
				3
			};
			(tier < 3 || edits <= MAXIMUM_EDITS).then(|| {
				(
					tier,
					edits,
					candidate.name.to_lowercase(),
					candidate.name.as_str(),
				)
			})
		})
		.collect();
	ranked.sort_by(|a, b| {
		a.0.cmp(&b.0)
			.then(a.1.cmp(&b.1))
			.then_with(|| a.2.cmp(&b.2))
			.then(Ordering::Equal)
	});
	let mut names: Vec<String> = Vec::new();
	for (_, _, _, name) in ranked {
		if !names.iter().any(|known| known == name) {
			names.push(name.to_owned());
			if names.len() == MAXIMUM_SUGGESTIONS {
				break;
			}
		}
	}
	names
}

fn distance(a: &str, b: &str) -> usize {
	let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
	if a.is_empty() {
		return b.len();
	}
	let mut row: Vec<usize> = (0..=b.len()).collect();
	for i in 1..=a.len() {
		let mut previous = row[0];
		row[0] = i;
		for j in 1..=b.len() {
			let saved = row[j];
			row[j] = (row[j] + 1)
				.min(row[j - 1] + 1)
				.min(previous + usize::from(a[i - 1] != b[j - 1]));
			previous = saved;
		}
	}
	row[b.len()]
}

/// v21, R-AI-1 (AI-3.6, AI-3.7): resolves a category or group name the user
/// typed against the tool's candidates.
#[uniffi::export]
pub fn match_category_name(
	wanted: String,
	candidates: Vec<NameCandidate>,
) -> Result<NameMatch, FfiError> {
	guard(|| Ok(match_name(&wanted, &candidates)))
}

/// v21: AI-3.6's name key, for telling two shown names apart.
#[uniffi::export]
pub fn category_name_key(name: String) -> Result<String, FfiError> {
	guard(|| Ok(name_key(&name)))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn candidate(id: &str, name: &str, is_group: bool, group: Option<&str>) -> NameCandidate {
		NameCandidate {
			id: id.to_owned(),
			name: name.to_owned(),
			is_group,
			group: group.map(str::to_owned),
		}
	}

	fn candidates() -> Vec<NameCandidate> {
		vec![
			candidate("everyday", "Everyday", true, None),
			candidate("groceries", "Groceries", false, Some("Everyday")),
			candidate("gas", "Gas", false, Some("Everyday")),
			candidate("gifts", "Gifts", false, Some("Fun")),
			candidate("cafe", "Café", false, Some("Fun")),
			candidate("kids", "The Kids", false, Some("Fun")),
			candidate("misc-a", "Misc", false, Some("Everyday")),
			candidate("misc-b", "Misc", false, Some("Fun")),
			candidate("food-group", "Food", true, None),
			candidate("food", "Food", false, Some("Everyday")),
			candidate("gone", "", true, None),
		]
	}

	fn found(wanted: &str) -> Option<String> {
		match match_name(wanted, &candidates()) {
			NameMatch::Found { candidate } => Some(candidate.id),
			_ => None,
		}
	}

	#[test]
	fn a_name_matches_by_its_key() {
		for (wanted, id) in [
			("groceries", "groceries"),
			("GROCERIES", "groceries"),
			("grocery", "groceries"),
			("the Groceries category", "groceries"),
			("  groceries ", "groceries"),
			("cafe", "cafe"),
			("CAFÉ", "cafe"),
			("everyday", "everyday"),
			("Everyday group", "everyday"),
			("the kids", "kids"),
			("kids", "kids"),
			("food group", "food-group"),
			("food category", "food"),
			("misc in fun", "misc-b"),
		] {
			assert_eq!(found(wanted).as_deref(), Some(id), "{wanted}");
		}
	}

	#[test]
	fn several_exact_matches_are_named_categories_first() {
		let NameMatch::Several { candidates: food } = match_name("food", &candidates()) else {
			panic!()
		};
		assert_eq!(
			food.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
			["food", "food-group"]
		);
		let NameMatch::Several { candidates: misc } = match_name("misc", &candidates()) else {
			panic!()
		};
		assert_eq!(
			misc.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
			["misc-a", "misc-b"]
		);
	}

	#[test]
	fn closest_names_follow_the_tiers_and_the_edit_limit() {
		let closest_to = |wanted: &str| match match_name(wanted, &candidates()) {
			NameMatch::NotFound { closest } => closest,
			other => panic!("{other:?}"),
		};
		assert_eq!(
			closest_to("groc"),
			["Groceries"],
			"Gas and Gifts are 3 edits away"
		);
		assert_eq!(
			closest_to("gift card"),
			["Gifts"],
			"the asked key holds the name"
		);
		assert_eq!(closest_to("ga"), ["Gas"]);
		assert_eq!(closest_to("gs"), ["Gas"], "one edit");
		assert!(closest_to("zzzzzz").is_empty());
		assert!(
			matches!(match_name("", &candidates()), NameMatch::NotFound { .. }),
			"a nameless candidate is never matched"
		);
	}

	#[test]
	fn accents_fold_across_latin_extended_a_and_b() {
		assert_eq!(fold("Café Crème"), "cafe creme");
		assert_eq!(
			fold("Łódź Żurek"),
			"lodz zurek",
			"Latin Extended-A, including ł, which has no decomposition"
		);
		assert_eq!(fold("Straße"), "strasse");
		assert_eq!(
			fold("Ǣ ǽ Ǿ Ŋ"),
			"ae ae o ng",
			"the letters added after review"
		);
		assert_eq!(
			fold("Ɗ Ɠ Ɲ Ʈ"),
			"d g n t",
			"Extended-B capitals whose lowercase is in IPA Extensions"
		);
		assert_eq!(fold("Œuvre Ærø"), "oeuvre aero");
		assert_eq!(
			fold("Đặng"),
			"dặng",
			"đ folds; ặ is Latin Extended Additional, beyond the table, so it stays"
		);
		assert_eq!(
			fold("Ǆemal Ǉubljana"),
			"dzemal ljubljana",
			"Latin Extended-B digraphs"
		);
		assert_eq!(
			fold("Ș Ț ș ț"),
			"s t s t",
			"Latin Extended-B comma-below letters"
		);
		assert_eq!(
			fold("Cafe\u{0301}"),
			"cafe",
			"a decomposed accent's combining mark is dropped"
		);
		assert_eq!(name_key("Çhêap Ëats"), name_key("cheap eats"));
		assert_eq!(found("lodz"), None);
	}

	#[test]
	fn the_table_is_sorted_and_lowercase() {
		assert!(LATIN_FOLDS.windows(2).all(|pair| pair[0].0 < pair[1].0));
		assert!(
			LATIN_FOLDS
				.iter()
				.all(|(letter, base)| letter.is_lowercase()
					&& base.chars().all(|c| c.is_ascii_lowercase()))
		);
	}

	#[test]
	fn the_key_drops_articles_singularises_and_breaks_on_punctuation() {
		assert_eq!(name_key("The Kids"), "kid");
		assert_eq!(name_key("Café & Snacks"), "cafe and snack");
		assert_eq!(name_key("Gifts"), name_key("gift"));
		assert_eq!(name_key("Car-Loan/Fees"), "car loan fee");
		assert_eq!(name_key("the"), "the", "an article alone stays");
	}
}
