/// The characters JavaScript's `\s` matches (WhiteSpace and LineTerminator),
/// which Actual's regexes and `trim` use. Unlike Unicode White_Space and
/// Rust's `char::is_whitespace`, it includes U+FEFF and excludes U+0085.
pub fn is_js_whitespace(c: char) -> bool {
	matches!(
		c,
		'\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
			..='\u{200A}'
				| '\u{2028}' | '\u{2029}'
				| '\u{202F}' | '\u{205F}'
				| '\u{3000}' | '\u{FEFF}'
	)
}

/// The longest emoji a goal icon may be, in UTF-8 bytes: room for every
/// RGI emoji, the longest being a kiss with two skin tones (35).
pub const MAX_EMOJI_BYTES: usize = 35;

/// The longest SF Symbol name a goal icon may be.
pub const MAX_SYMBOL_CHARS: usize = 64;

/// A goal's icon: an SF Symbol name or exactly one emoji. ASCII means a
/// symbol name, anything else an emoji.
pub fn is_goal_icon(text: &str) -> bool {
	is_symbol_name(text) || is_single_emoji(text)
}

/// An SF Symbol name: 1 to 64 of `a-z`, `0-9` and `.`, not starting or
/// ending with `.`.
pub fn is_symbol_name(text: &str) -> bool {
	(1..=MAX_SYMBOL_CHARS).contains(&text.len())
		&& text
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
		&& !text.starts_with('.')
		&& !text.ends_with('.')
}

/// Whether `text` is one emoji: a flag (two regional indicators), a keycap,
/// or pictographs with their variation selectors, skin tones and tags joined
/// by ZWJ. Each of these is exactly one extended grapheme cluster (UAX #29
/// GB9, GB11, GB12), so anything accepted is a single grapheme.
pub fn is_single_emoji(text: &str) -> bool {
	if text.is_empty() || text.len() > MAX_EMOJI_BYTES {
		return false;
	}
	let chars: Vec<char> = text.chars().collect();
	match chars.as_slice() {
		[first, second] if is_regional_indicator(*first) && is_regional_indicator(*second) => true,
		['0'..='9' | '#' | '*', rest @ ..] => {
			matches!(rest, ['\u{20E3}'] | ['\u{FE0F}', '\u{20E3}'])
		}
		_ => text.split('\u{200D}').all(|element| {
			let mut element = element.chars();
			element.next().is_some_and(is_pictographic) && element.all(is_emoji_extend)
		}),
	}
}

fn is_regional_indicator(c: char) -> bool {
	matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

/// Variation selectors, skin tone modifiers and tag characters: the Extend
/// characters an emoji sequence carries (UTS #51).
fn is_emoji_extend(c: char) -> bool {
	matches!(
		c,
		'\u{FE0E}' | '\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0020}'..='\u{E007F}'
	)
}

/// Unicode's Extended_Pictographic ranges, without the modifiers and
/// regional indicators that only attach to or pair with a pictograph.
fn is_pictographic(c: char) -> bool {
	if is_regional_indicator(c) || matches!(c, '\u{1F3FB}'..='\u{1F3FF}') {
		return false;
	}
	matches!(
		c,
		'\u{A9}'
			| '\u{AE}' | '\u{203C}'
			| '\u{2049}' | '\u{2122}'
			| '\u{2139}' | '\u{2194}'..='\u{2199}'
			| '\u{21A9}'..='\u{21AA}'
			| '\u{231A}'..='\u{231B}'
			| '\u{2328}' | '\u{2388}'
			| '\u{23CF}' | '\u{23E9}'..='\u{23F3}'
			| '\u{23F8}'..='\u{23FA}'
			| '\u{24C2}' | '\u{25AA}'..='\u{25AB}'
			| '\u{25B6}' | '\u{25C0}'
			| '\u{25FB}'..='\u{25FE}'
			| '\u{2600}'..='\u{27BF}'
			| '\u{2934}'..='\u{2935}'
			| '\u{2B05}'..='\u{2B07}'
			| '\u{2B1B}'..='\u{2B1C}'
			| '\u{2B50}' | '\u{2B55}'
			| '\u{3030}' | '\u{303D}'
			| '\u{3297}' | '\u{3299}'
			| '\u{1F000}'..='\u{1FAFF}'
			| '\u{1FC00}'..='\u{1FFFD}'
	)
}

#[cfg(test)]
mod tests {
	use super::{is_goal_icon, is_js_whitespace, is_single_emoji, is_symbol_name};

	const FAMILY_OF_THREE: &str = "👨‍👩‍👧";
	const ENGLAND: &str = "🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
	const KISS_WITH_SKIN_TONES: &str = "👩🏻‍❤️‍💋‍👨🏼";

	#[test]
	fn one_emoji_of_any_shape_up_to_35_bytes_and_nothing_else() {
		for emoji in [
			"🎯",
			"💻",
			"❤",
			"❤️",
			"👍🏽",
			"🇯🇵",
			"1️⃣",
			"#⃣",
			"🧑‍💻",
			"👩🏽‍💻",
			"🏳️‍🌈",
			"©",
			FAMILY_OF_THREE,
			ENGLAND,
			KISS_WITH_SKIN_TONES,
		] {
			assert!(is_single_emoji(emoji), "{emoji}");
		}
		for not_one in [
			"",
			" ",
			"a",
			"1",
			"ab",
			"🎯🎯",
			"🎯 ",
			" 🎯",
			"🇯",
			"🇯🇵🇺",
			"🏽",
			"\u{200D}",
			"🧑\u{200D}",
			"\u{200D}💻",
			"🧑\u{200D}\u{200D}💻",
			"é",
			"e\u{301}",
			"🎯\u{301}",
			"1\u{20E3}\u{20E3}",
			"laptopcomputer",
		] {
			assert!(!is_single_emoji(not_one), "{not_one:?}");
		}
		assert_eq!(
			(
				FAMILY_OF_THREE.len(),
				ENGLAND.len(),
				KISS_WITH_SKIN_TONES.len()
			),
			(18, 28, 35)
		);
		let six = "🧑‍🧑‍🧑‍🧑‍🧑‍🧑";
		assert_eq!(six.len(), 39);
		assert!(!is_single_emoji(six), "one grapheme, but past 35 bytes");
	}

	#[test]
	fn a_goal_icon_is_a_symbol_name_or_one_emoji() {
		for name in [
			"laptopcomputer",
			"car.fill",
			"graduationcap",
			"1",
			"airplane.circle.fill",
		] {
			assert!(is_symbol_name(name) && is_goal_icon(name), "{name}");
		}
		let longest = "a".repeat(64);
		assert!(is_goal_icon(&longest));
		for not_a_name in [
			"",
			"Laptop",
			".car",
			"car.",
			".",
			"car fill",
			"car-fill",
			"car_fill",
			"café",
			&"a".repeat(65),
		] {
			assert!(!is_symbol_name(not_a_name), "{not_a_name:?}");
		}
		for emoji in ["🎯", FAMILY_OF_THREE, ENGLAND] {
			assert!(is_goal_icon(emoji) && !is_symbol_name(emoji), "{emoji}");
		}
		for neither in ["", "ab🎯", "🎯🎯", "Car", "car ", " ", "🎯car"] {
			assert!(!is_goal_icon(neither), "{neither:?}");
		}
	}

	#[test]
	fn differs_from_rust_only_on_the_byte_order_mark_and_next_line() {
		let differing: Vec<char> = (0..=0x10FFFF)
			.filter_map(char::from_u32)
			.filter(|&c| is_js_whitespace(c) != c.is_whitespace())
			.collect();
		assert_eq!(differing, ['\u{85}', '\u{FEFF}']);
		assert!(is_js_whitespace('\u{FEFF}') && !is_js_whitespace('\u{85}'));
	}
}
