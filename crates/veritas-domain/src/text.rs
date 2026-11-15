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

#[cfg(test)]
mod tests {
	use super::is_js_whitespace;

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
