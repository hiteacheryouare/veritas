//! `new RegExp(pattern)` with no flags, translated to the `regex` crate.
//!
//! JavaScript's non-Unicode mode differs from `regex` in ways that change
//! answers: `\d`, `\w`, `\s` and `\b` are ASCII or JS-specific, `.` also stops
//! at `\r`, U+2028 and U+2029, a letter escape like `\a` is just that letter,
//! a lone `]` or `}` is literal, and classes treat `&&`, `--` and `[` as plain
//! characters. Every atom is rewritten so `regex` reads it the JS way. What
//! can't be (lookaround, backreferences, octal escapes, `[^]`, characters
//! outside the BMP that JS splits into surrogates) is refused, so the
//! condition is reported instead of guessed.

use regex::Regex;

const WORD: &str = "A-Za-z0-9_";
const DIGIT: &str = "0-9";
const SPACE: &str = "\\t\\n\\x0B\\x0C\\r \\x{A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}";

#[derive(Clone, Debug)]
pub struct JsPattern {
	pub source: String,
	regex: Regex,
	/// `(?-u:\B)` can match between the bytes of one UTF-8 character, and
	/// `regex` then drops that empty match and can miss another branch.
	non_boundary: bool,
}

/// Why `test` can't give JavaScript's answer for a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
	/// A character JS splits into two surrogates, which `.` and classes see.
	Astral,
	/// A `\B` in the pattern and a non-ASCII character in the text.
	NonBoundary,
}

impl PartialEq for JsPattern {
	fn eq(&self, other: &Self) -> bool {
		self.source == other.source
	}
}

impl JsPattern {
	/// `Err` says why Veritas won't evaluate the pattern. That includes
	/// patterns `regex` rejects, since JS may still accept them.
	pub fn new(source: &str) -> Result<JsPattern, String> {
		if source.chars().any(|c| c.len_utf16() > 1) {
			return Err(
				"a regex with an emoji or another character JavaScript reads as two".into(),
			);
		}
		if source.chars().count() > MAX_SOURCE_CHARS {
			return Err("a regex longer than 10,000 characters".into());
		}
		let mut non_boundary = false;
		let translated = translate(source, &mut non_boundary)
			.map_err(|detail| format!("a regex with {detail}"))?;
		if translated.len() > MAX_TRANSLATED_BYTES {
			return Err("a regex too large for Veritas to check".into());
		}
		// The crate's own diagnostic is for developers, so it stays out of the
		// text people see.
		let regex = Regex::new(&translated)
			.map_err(|_| "a regex Veritas can't compile the way JavaScript reads it".to_owned())?;
		Ok(JsPattern {
			source: source.to_owned(),
			regex,
			non_boundary,
		})
	}

	/// `RegExp.prototype.test`, or why Veritas can't give JS's answer.
	pub fn test(&self, text: &str) -> Result<bool, Unreadable> {
		if text.chars().any(|c| c.len_utf16() > 1) {
			return Err(Unreadable::Astral);
		}
		if self.non_boundary && !text.is_ascii() {
			return Err(Unreadable::NonBoundary);
		}
		Ok(self.regex.is_match(text))
	}
}

/// One character as `regex` reads it literally, inside or outside a class:
/// `regex::escape` covers every character with a meaning there, `-`, `&`,
/// `~` and `#` included.
fn literal(c: char) -> String {
	if c.is_control() {
		return format!("\\x{{{:X}}}", u32::from(c));
	}
	regex::escape(c.encode_utf8(&mut [0; 4]))
}

/// Longer patterns are refused rather than compiled: real rules are short,
/// and a pathological one shouldn't stall loading the rules.
const MAX_SOURCE_CHARS: usize = 10_000;
const MAX_TRANSLATED_BYTES: usize = 50_000;

fn hex(chars: &[char], at: usize, len: usize) -> Option<char> {
	let digits: String = chars.get(at..at + len)?.iter().collect();
	if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
		return None;
	}
	char::from_u32(u32::from_str_radix(&digits, 16).ok()?)
}

/// One escape after a `\`, as JS reads it outside or inside a class.
enum Escape {
	Char(char),
	Class(&'static str, bool),
	Boundary(bool),
}

fn escape(chars: &[char], i: &mut usize, in_class: bool) -> Result<Escape, String> {
	let Some(&c) = chars.get(*i) else {
		return Err("a trailing backslash".into());
	};
	*i += 1;
	Ok(match c {
		'd' => Escape::Class(DIGIT, false),
		'D' => Escape::Class(DIGIT, true),
		'w' => Escape::Class(WORD, false),
		'W' => Escape::Class(WORD, true),
		's' => Escape::Class(SPACE, false),
		'S' => Escape::Class(SPACE, true),
		'b' if in_class => Escape::Char('\u{8}'),
		'b' => Escape::Boundary(true),
		'B' => Escape::Boundary(false),
		't' => Escape::Char('\t'),
		'n' => Escape::Char('\n'),
		'v' => Escape::Char('\u{b}'),
		'f' => Escape::Char('\u{c}'),
		'r' => Escape::Char('\r'),
		'0' if !chars.get(*i).is_some_and(char::is_ascii_digit) => Escape::Char('\0'),
		'1'..='9' | '0' => return Err("backreferences and octal escapes".into()),
		'x' => match hex(chars, *i, 2) {
			Some(value) => {
				*i += 2;
				Escape::Char(value)
			}
			None => Escape::Char('x'),
		},
		'u' if chars.get(*i) == Some(&'{') => return Err("a \\u{…} escape".into()),
		'u' => match hex(chars, *i, 4) {
			Some(value) => {
				*i += 4;
				Escape::Char(value)
			}
			None if chars
				.get(*i..*i + 4)
				.is_some_and(|d| d.iter().all(char::is_ascii_hexdigit)) =>
			{
				return Err("a surrogate escape".into());
			}
			None => Escape::Char('u'),
		},
		'c' => match chars.get(*i) {
			Some(letter) if letter.is_ascii_alphabetic() => {
				*i += 1;
				Escape::Char(char::from(
					u8::try_from(u32::from(*letter) % 32).unwrap_or(0),
				))
			}
			_ => return Err("a \\c escape without a letter".into()),
		},
		'k' => return Err("named backreferences".into()),
		other => Escape::Char(other),
	})
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Repeat {
	No,
	Yes,
	/// Just quantified: only a lazy `?` may follow.
	Lazy,
}

fn class_text(set: &str, negated: bool) -> String {
	format!("[{}{set}]", if negated { "^" } else { "" })
}

fn translate(source: &str, non_boundary: &mut bool) -> Result<String, String> {
	let chars: Vec<char> = source.chars().collect();
	let mut out = String::new();
	let mut i = 0;
	// What the last token lets a quantifier do: JS throws on `^*`, `(*`,
	// `a**` and `a*+`, so those are refused rather than handed to `regex`.
	let mut state = Repeat::No;
	while i < chars.len() {
		let c = chars[i];
		i += 1;
		let quantifier_end = (c == '{').then(|| quantifier(&chars, i)).flatten();
		if matches!(c, '*' | '+' | '?') || quantifier_end.is_some() {
			state = match (state, c) {
				(Repeat::Yes, _) => Repeat::Lazy,
				(Repeat::Lazy, '?') => Repeat::No,
				_ => return Err("a quantifier with nothing JS can repeat".into()),
			};
			match quantifier_end {
				Some(end) => {
					out.extend(&chars[i - 1..end]);
					i = end;
				}
				None => out.push(c),
			}
			continue;
		}
		state = match c {
			'^' | '$' | '|' | '(' => Repeat::No,
			'\\' if matches!(chars.get(i), Some('b' | 'B')) => Repeat::No,
			_ => Repeat::Yes,
		};
		match c {
			'\\' => match escape(&chars, &mut i, false)? {
				Escape::Char(value) => out.push_str(&literal(value)),
				Escape::Class(set, negated) => out.push_str(&class_text(set, negated)),
				Escape::Boundary(true) => out.push_str("(?-u:\\b)"),
				Escape::Boundary(false) => {
					*non_boundary = true;
					out.push_str("(?-u:\\B)");
				}
			},
			'[' => out.push_str(&translate_class(&chars, &mut i)?),
			'(' => {
				if chars.get(i) == Some(&'?') {
					match (chars.get(i + 1), chars.get(i + 2)) {
						(Some(':'), _) => {
							out.push_str("(?:");
							i += 2;
						}
						(Some('<'), Some(&next)) if next != '=' && next != '!' => {
							let close = chars[i + 2..]
								.iter()
								.position(|c| *c == '>')
								.ok_or("an unterminated group name")?;
							let name: String = chars[i + 2..i + 2 + close].iter().collect();
							if name.is_empty()
								|| !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
							{
								return Err("a group name Veritas can't carry over".into());
							}
							out.push_str(&format!("(?<{name}>"));
							i += 3 + close;
						}
						_ => return Err("lookaround or inline flags".into()),
					}
				} else {
					out.push('(');
				}
			}
			')' | '|' | '^' | '$' => out.push(c),
			'.' => out.push_str("[^\\n\\r\\x{2028}\\x{2029}]"),
			other => out.push_str(&literal(other)),
		}
	}
	Ok(out)
}

/// The end of `{n}`, `{n,}` or `{n,m}` starting just after `{`, if it is one.
fn quantifier(chars: &[char], start: usize) -> Option<usize> {
	let mut i = start;
	let digits = |i: &mut usize| {
		let from = *i;
		while chars.get(*i).is_some_and(char::is_ascii_digit) {
			*i += 1;
		}
		*i > from
	};
	if !digits(&mut i) {
		return None;
	}
	if chars.get(i) == Some(&',') {
		i += 1;
		digits(&mut i);
	}
	(chars.get(i) == Some(&'}')).then_some(i + 1)
}

/// A JS class: the first unescaped `]` ends it (so `[]` matches nothing),
/// and every member is written out as an escaped code point or range.
fn translate_class(chars: &[char], i: &mut usize) -> Result<String, String> {
	let negated = chars.get(*i) == Some(&'^');
	if negated {
		*i += 1;
	}
	let mut members = String::new();
	let mut previous: Option<char> = None;
	// After a class escape, `-x` is a literal `-` and a literal `x`, and the
	// `x` can't start a range (Annex B `ClassAtom - ClassAtom` with a class
	// escape on one side): `[\d-a-z]` is digits, `-`, `a`, `-` and `z`.
	let mut after_class_escape = false;
	loop {
		let Some(&c) = chars.get(*i) else {
			return Err("an unterminated class".into());
		};
		*i += 1;
		let member = match c {
			']' => break,
			'\\' => match escape(chars, i, true)? {
				Escape::Char(value) => Some(value),
				Escape::Class(set, false) => {
					members.push_str(set);
					previous = None;
					after_class_escape = true;
					continue;
				}
				Escape::Class(_, true) => {
					return Err("a negated class escape inside a class".into());
				}
				Escape::Boundary(_) => return Err("a boundary inside a class".into()),
			},
			'-' if after_class_escape && chars.get(*i).is_some_and(|next| *next != ']') => {
				after_class_escape = false;
				members.push_str(&literal('-'));
				let next = chars[*i];
				*i += 1;
				match next {
					'\\' => match escape(chars, i, true)? {
						Escape::Char(value) => members.push_str(&literal(value)),
						Escape::Class(set, false) => members.push_str(set),
						Escape::Class(_, true) => {
							return Err("a negated class escape inside a class".into());
						}
						Escape::Boundary(_) => return Err("a boundary inside a class".into()),
					},
					other => members.push_str(&literal(other)),
				}
				previous = None;
				continue;
			}
			'-' if previous.is_some() && chars.get(*i).is_some_and(|next| *next != ']') => {
				let low = previous.take().ok_or("a range without a start")?;
				let high = match chars.get(*i) {
					Some('\\') => {
						*i += 1;
						match escape(chars, i, true)? {
							Escape::Char(value) => value,
							_ => return Err("a range ending in a class escape".into()),
						}
					}
					Some(&high) => {
						*i += 1;
						high
					}
					None => return Err("an unterminated class".into()),
				};
				if high < low {
					return Err("a range out of order".into());
				}
				members.push_str(&format!("-{}", literal(high)));
				continue;
			}
			other => Some(other),
		};
		after_class_escape = false;
		if let Some(member) = member {
			members.push_str(&literal(member));
			previous = Some(member);
		}
	}
	if members.is_empty() {
		return Ok(if negated {
			return Err("a [^] class, which matches anything in JavaScript".into());
		} else {
			"[^\\x{0}-\\x{10FFFF}]".into()
		});
	}
	Ok(format!("[{}{members}]", if negated { "^" } else { "" }))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn test(pattern: &str, text: &str) -> bool {
		JsPattern::new(pattern).unwrap().test(text).unwrap()
	}

	#[test]
	fn actual_vectors() {
		// reference/actual/packages/loot-core/src/server/rules/index.test.ts:233-241
		for (text, expected) in [
			("bar foo baz", false),
			("foo", true),
			("foooo", true),
			("foob", false),
			("bfoo", false),
			("f o o", false),
		] {
			assert_eq!(test("^fo*$", text), expected, "{text}");
		}
	}

	#[test]
	fn classes_are_ascii_like_javascript() {
		assert!(test("^\\d+$", "123") && !test("^\\d+$", "١٢٣"));
		assert!(test("^\\w+$", "abc_1") && !test("^\\w+$", "école"));
		assert!(test("\\bcafe\\b", "the cafe") && test("\\bcaf", "x caf"));
		assert!(!test("^.$", "\r") && test("^.$", "é"));
		assert!(test("^\\s$", "\u{feff}") && !test("^\\s$", "\u{85}"));
	}

	#[test]
	fn javascript_literal_quirks() {
		assert!(test("a]b", "a]b"));
		assert!(test("a{b", "a{b") && test("a{2}", "aa"));
		assert!(test("\\a\\q", "aq"));
		assert!(test("[a&&b]", "&") && test("[--z]", "-"));
		assert!(!test("[]", "x") && !test("x[]", "x"));
		assert!(test("[\\b]", "\u{8}"));
		assert!(test("\\x41\\u0042", "AB"));
		assert!(test("(?<year>\\d{4})-\\d\\d", "2026-11"));
	}

	#[test]
	fn refuses_what_it_cannot_read_the_js_way() {
		for pattern in [
			"^*",
			"a**",
			"a*+",
			"(*a)",
			"a|+",
			"\\b+",
			"(?=a)",
			"(?!a)",
			"(?<=a)b",
			"(?<!a)b",
			"(a)\\1",
			"\\k<x>",
			"[^]",
			"\\012",
			"(?i)a",
			"\\u{1f600}",
			"😀",
		] {
			assert!(JsPattern::new(pattern).is_err(), "{pattern}");
		}
		assert_eq!(
			JsPattern::new("a").unwrap().test("😀a"),
			Err(Unreadable::Astral)
		);
	}

	#[test]
	fn both_size_caps_can_trip() {
		assert!(JsPattern::new(&"ab".repeat(5_000)).is_ok());
		assert!(JsPattern::new(&"a".repeat(10_001)).is_err());
		// `\s` expands the most, so a run of them reaches the translation cap
		// well inside the source cap.
		let per_space = SPACE.len() + 2;
		let fits = MAX_TRANSLATED_BYTES / per_space;
		assert!(fits * 2 < MAX_SOURCE_CHARS);
		assert!(JsPattern::new(&"\\s".repeat(fits)).is_ok());
		assert_eq!(
			JsPattern::new(&"\\s".repeat(fits + 1)).unwrap_err(),
			"a regex too large for Veritas to check"
		);
	}

	#[test]
	fn a_dash_after_a_class_escape_is_literal_and_ends_no_range() {
		// Annex B: `[\d-a-z]` is digits, `-`, `a`, `-`, `z`; `[\d--z]` is
		// digits, `-` and `z`.
		assert!(!test("[\\d-a-z]", "m"));
		assert!(
			test("[\\d-a-z]", "a")
				&& test("[\\d-a-z]", "z")
				&& test("[\\d-a-z]", "-")
				&& test("[\\d-a-z]", "7")
		);
		assert!(!test("[\\d--z]", "a") && !test("[\\d--z]", "!"));
		assert!(test("[\\d--z]", "-") && test("[\\d--z]", "z"));
		assert!(test("[\\w-.-z]", ".") && !test("[\\w-.-z]", "/"));
		assert!(test("[\\d-]", "-"));
		// After the literal pair, a later range still forms as in JS.
		assert!(test("[\\d-a--z]", "m"));
	}

	#[test]
	fn a_non_boundary_on_non_ascii_text_is_left_to_actual() {
		let pattern = JsPattern::new("\\B|é").unwrap();
		assert_eq!(pattern.test("sé1"), Err(Unreadable::NonBoundary));
		assert_eq!(pattern.test("se1"), Ok(true));
		assert_eq!(JsPattern::new("\\b").unwrap().test("é b"), Ok(true));
	}
}
