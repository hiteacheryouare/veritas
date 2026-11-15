use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, Write};

/// A string as JSON.parse produces it: UTF-16 units, so a lone surrogate from
/// a `\ud800` escape survives. Strings read from SQLite are always valid UTF-8.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Str {
	Utf8(String),
	Utf16(Vec<u16>),
}

impl Str {
	fn code_points(&self) -> Box<dyn Iterator<Item = u32> + '_> {
		match self {
			Str::Utf8(text) => Box::new(text.chars().map(u32::from)),
			Str::Utf16(units) => Box::new(char::decode_utf16(units.iter().copied()).map(|unit| {
				unit.map_or_else(|lone| u32::from(lone.unpaired_surrogate()), u32::from)
			})),
		}
	}

	/// `None` when it holds a lone surrogate.
	pub(crate) fn to_utf8(&self) -> Option<String> {
		match self {
			Str::Utf8(text) => Some(text.clone()),
			Str::Utf16(units) => String::from_utf16(units).ok(),
		}
	}

	pub(crate) fn eq_ascii(&self, ascii: &str) -> bool {
		match self {
			Str::Utf8(text) => text == ascii,
			Str::Utf16(units) => units.iter().copied().eq(ascii.encode_utf16()),
		}
	}
}

impl From<&str> for Str {
	fn from(text: &str) -> Self {
		Str::Utf8(text.to_owned())
	}
}

/// Every value a canonical dump can hold. `Integer` and `SqlReal` are SQLite
/// cells; `Number` is a JavaScript number from metadata.json.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
	Null,
	Bool(bool),
	Integer(i64),
	SqlReal(f64),
	Number(f64),
	String(Str),
	Blob(Vec<u8>),
	Array(Vec<Value>),
	Object(Vec<(Str, Value)>),
}

/// Unicode code point order, which is UTF-8 byte order (`compareCodePoints`).
pub(crate) fn compare_code_points(a: &Str, b: &Str) -> Ordering {
	match (a, b) {
		(Str::Utf8(a), Str::Utf8(b)) => a.as_bytes().cmp(b.as_bytes()),
		_ => a.code_points().cmp(b.code_points()),
	}
}

/// ECMAScript `Number.prototype.toString` for a finite number.
pub(crate) fn js_number(value: f64) -> String {
	if value == 0.0 {
		return "0".to_owned();
	}
	if value < 0.0 {
		return format!("-{}", js_number(-value));
	}
	// `{:e}` prints the shortest digits that round-trip, as ECMAScript requires.
	let scientific = format!("{value:e}");
	let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
	let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
	let k = digits.len() as i64;
	let n = exponent.parse::<i64>().unwrap_or(0) + 1;
	if k <= n && n <= 21 {
		format!("{digits}{}", "0".repeat((n - k) as usize))
	} else if 0 < n && n <= 21 {
		let (whole, fraction) = digits.split_at(n as usize);
		format!("{whole}.{fraction}")
	} else if -6 < n && n <= 0 {
		format!("0.{}{digits}", "0".repeat((-n) as usize))
	} else {
		let (first, rest) = digits.split_at(1);
		let sign = if n - 1 < 0 { '-' } else { '+' };
		let point = if rest.is_empty() { "" } else { "." };
		format!("{first}{point}{rest}e{sign}{}", (n - 1).abs())
	}
}

/// A SQLite REAL: the JavaScript text, with `.0` added when it would
/// otherwise read as an integer.
pub(crate) fn format_real(value: f64) -> String {
	let text = js_number(value);
	if text.contains(['.', 'e']) {
		text
	} else {
		format!("{text}.0")
	}
}

fn write_escaped_char(out: &mut impl Write, c: u32) -> io::Result<()> {
	match c {
		0x22 => out.write_all(b"\\\""),
		0x5c => out.write_all(b"\\\\"),
		0x08 => out.write_all(b"\\b"),
		0x09 => out.write_all(b"\\t"),
		0x0a => out.write_all(b"\\n"),
		0x0c => out.write_all(b"\\f"),
		0x0d => out.write_all(b"\\r"),
		0x00..=0x1f | 0xd800..=0xdfff => write!(out, "\\u{c:04x}"),
		_ => {
			let mut buffer = [0; 4];
			let encoded = char::from_u32(c)
				.unwrap_or('\u{fffd}')
				.encode_utf8(&mut buffer);
			out.write_all(encoded.as_bytes())
		}
	}
}

pub(crate) fn write_string(out: &mut impl Write, text: &Str) -> io::Result<()> {
	out.write_all(b"\"")?;
	match text {
		Str::Utf8(text) => {
			let bytes = text.as_bytes();
			let mut start = 0;
			for (index, &byte) in bytes.iter().enumerate() {
				if byte < 0x20 || byte == b'"' || byte == b'\\' {
					out.write_all(&bytes[start..index])?;
					write_escaped_char(out, u32::from(byte))?;
					start = index + 1;
				}
			}
			out.write_all(&bytes[start..])?;
		}
		Str::Utf16(_) => {
			for c in text.code_points() {
				write_escaped_char(out, c)?;
			}
		}
	}
	out.write_all(b"\"")
}

fn sorted(entries: &[(Str, Value)]) -> Vec<&(Str, Value)> {
	let mut sorted: Vec<&(Str, Value)> = entries.iter().collect();
	sorted.sort_by(|a, b| compare_code_points(&a.0, &b.0));
	sorted
}

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn write_compact(out: &mut impl Write, value: &Value) -> io::Result<()> {
	match value {
		Value::Null => out.write_all(b"null"),
		Value::Bool(flag) => out.write_all(if *flag { b"true" } else { b"false" }),
		Value::Integer(n) => write!(out, "{n}"),
		Value::SqlReal(x) if x.is_finite() => out.write_all(format_real(*x).as_bytes()),
		Value::SqlReal(x) => {
			let name = if *x > 0.0 { "Infinity" } else { "-Infinity" };
			write!(out, "{{\"$real\":\"{name}\"}}")
		}
		Value::Number(x) if x.is_finite() => out.write_all(js_number(*x).as_bytes()),
		Value::Number(_) => Err(io::Error::other(
			"a number in metadata.json is too large for JSON",
		)),
		Value::String(text) => write_string(out, text),
		Value::Blob(bytes) => {
			out.write_all(b"{\"$blob\":\"")?;
			let mut hex = Vec::with_capacity(bytes.len() * 2);
			for byte in bytes {
				hex.push(HEX[usize::from(byte >> 4)]);
				hex.push(HEX[usize::from(byte & 0x0f)]);
			}
			out.write_all(&hex)?;
			out.write_all(b"\"}")
		}
		Value::Array(items) => {
			out.write_all(b"[")?;
			for (index, item) in items.iter().enumerate() {
				if index > 0 {
					out.write_all(b",")?;
				}
				write_compact(out, item)?;
			}
			out.write_all(b"]")
		}
		Value::Object(entries) => {
			out.write_all(b"{")?;
			for (index, (key, item)) in sorted(entries).into_iter().enumerate() {
				if index > 0 {
					out.write_all(b",")?;
				}
				write_string(out, key)?;
				out.write_all(b":")?;
				write_compact(out, item)?;
			}
			out.write_all(b"}")
		}
	}
}

pub(crate) fn write_indent(out: &mut impl Write, depth: usize) -> io::Result<()> {
	for _ in 0..depth {
		out.write_all(b"\t")?;
	}
	Ok(())
}

/// The dump layout (`writeCanonical`): objects one key per line, arrays one
/// compact element per line, tabs for indentation.
pub(crate) fn write_pretty(out: &mut impl Write, value: &Value, depth: usize) -> io::Result<()> {
	match value {
		Value::Array(items) if items.is_empty() => out.write_all(b"[]"),
		Value::Array(items) => {
			out.write_all(b"[\n")?;
			for (index, item) in items.iter().enumerate() {
				write_indent(out, depth + 1)?;
				write_compact(out, item)?;
				out.write_all(if index + 1 < items.len() {
					b",\n"
				} else {
					b"\n"
				})?;
			}
			write_indent(out, depth)?;
			out.write_all(b"]")
		}
		Value::Object(entries) if entries.is_empty() => out.write_all(b"{}"),
		Value::Object(entries) => {
			out.write_all(b"{\n")?;
			let entries = sorted(entries);
			for (index, (key, item)) in entries.iter().enumerate() {
				write_indent(out, depth + 1)?;
				write_string(out, key)?;
				out.write_all(b": ")?;
				write_pretty(out, item, depth + 1)?;
				out.write_all(if index + 1 < entries.len() {
					b",\n"
				} else {
					b"\n"
				})?;
			}
			write_indent(out, depth)?;
			out.write_all(b"}")
		}
		_ => write_compact(out, value),
	}
}

/// An ECMAScript array index: the canonical decimal form of 0 to 2^32 - 2.
fn array_index(key: &Str) -> Option<u32> {
	let text = match key {
		Str::Utf8(text) => text.clone(),
		Str::Utf16(units) => String::from_utf16(units).ok()?,
	};
	if text.is_empty()
		|| (text.len() > 1 && text.starts_with('0'))
		|| !text.bytes().all(|b| b.is_ascii_digit())
	{
		return None;
	}
	text.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

/// `JSON.stringify` with no indentation: array-index keys first, then the
/// rest in insertion order, and a
/// number JSON can't hold written as `null`.
pub(crate) fn stringify(out: &mut impl Write, value: &Value) -> io::Result<()> {
	match value {
		Value::Number(x) if !x.is_finite() => out.write_all(b"null"),
		Value::Array(items) => {
			out.write_all(b"[")?;
			for (index, item) in items.iter().enumerate() {
				if index > 0 {
					out.write_all(b",")?;
				}
				stringify(out, item)?;
			}
			out.write_all(b"]")
		}
		Value::Object(entries) => {
			// A JavaScript object lists array-index keys first, ascending.
			let mut ordered: Vec<(Option<u32>, &(Str, Value))> = entries
				.iter()
				.map(|entry| (array_index(&entry.0), entry))
				.collect();
			ordered.sort_by_key(|(index, _)| index.map_or((1, 0), |n| (0, n)));
			out.write_all(b"{")?;
			for (position, (_, (key, item))) in ordered.into_iter().enumerate() {
				if position > 0 {
					out.write_all(b",")?;
				}
				write_string(out, key)?;
				out.write_all(b":")?;
				stringify(out, item)?;
			}
			out.write_all(b"}")
		}
		_ => write_compact(out, value),
	}
}

#[cfg(test)]
pub(crate) fn to_canonical(value: &Value) -> String {
	let mut out = Vec::new();
	write_pretty(&mut out, value, 0).unwrap();
	out.push(b'\n');
	String::from_utf8(out).unwrap()
}

const MAX_DEPTH: usize = 512;

/// `JSON.parse`, for metadata.json: numbers become doubles, duplicate keys
/// keep the last value, and strings keep lone surrogates.
pub(crate) fn parse(text: &str) -> Result<Value, &'static str> {
	let mut parser = Parser { text, at: 0 };
	parser.skip_whitespace();
	let value = parser.value(0)?;
	parser.skip_whitespace();
	if parser.at != parser.text.len() {
		return Err("metadata.json has text after its JSON value");
	}
	Ok(value)
}

/// `at` is a byte offset into `text`. Only ASCII is ever stepped over with
/// `at += 1`; everything else goes through `next`.
struct Parser<'a> {
	text: &'a str,
	at: usize,
}

const INVALID: &str = "metadata.json isn't valid JSON";

impl Parser<'_> {
	fn peek(&self) -> Option<char> {
		self.text.get(self.at..)?.chars().next()
	}

	fn next(&mut self) -> Result<char, &'static str> {
		let c = self.peek().ok_or(INVALID)?;
		self.at += c.len_utf8();
		Ok(c)
	}

	fn expect(&mut self, c: char) -> Result<(), &'static str> {
		if self.next()? == c {
			Ok(())
		} else {
			Err(INVALID)
		}
	}

	fn skip_whitespace(&mut self) {
		while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
			self.at += 1;
		}
	}

	fn literal(&mut self, word: &str, value: Value) -> Result<Value, &'static str> {
		for c in word.chars() {
			self.expect(c)?;
		}
		Ok(value)
	}

	fn value(&mut self, depth: usize) -> Result<Value, &'static str> {
		if depth > MAX_DEPTH {
			return Err("metadata.json is nested too deeply");
		}
		match self.peek().ok_or(INVALID)? {
			'{' => self.object(depth),
			'[' => self.array(depth),
			'"' => self.string().map(Value::String),
			't' => self.literal("true", Value::Bool(true)),
			'f' => self.literal("false", Value::Bool(false)),
			'n' => self.literal("null", Value::Null),
			'-' | '0'..='9' => self.number(),
			_ => Err(INVALID),
		}
	}

	fn object(&mut self, depth: usize) -> Result<Value, &'static str> {
		self.expect('{')?;
		let mut entries: Vec<(Str, Value)> = Vec::new();
		let mut index: HashMap<Str, usize> = HashMap::new();
		self.skip_whitespace();
		if self.peek() == Some('}') {
			self.at += 1;
			return Ok(Value::Object(entries));
		}
		loop {
			self.skip_whitespace();
			let key = self.string()?;
			self.skip_whitespace();
			self.expect(':')?;
			self.skip_whitespace();
			let value = self.value(depth + 1)?;
			match index.get(&key) {
				Some(&at) => entries[at].1 = value,
				None => {
					index.insert(key.clone(), entries.len());
					entries.push((key, value));
				}
			}
			self.skip_whitespace();
			match self.next()? {
				',' => {}
				'}' => return Ok(Value::Object(entries)),
				_ => return Err(INVALID),
			}
		}
	}

	fn array(&mut self, depth: usize) -> Result<Value, &'static str> {
		self.expect('[')?;
		let mut items = Vec::new();
		self.skip_whitespace();
		if self.peek() == Some(']') {
			self.at += 1;
			return Ok(Value::Array(items));
		}
		loop {
			self.skip_whitespace();
			items.push(self.value(depth + 1)?);
			self.skip_whitespace();
			match self.next()? {
				',' => {}
				']' => return Ok(Value::Array(items)),
				_ => return Err(INVALID),
			}
		}
	}

	fn hex4(&mut self) -> Result<u16, &'static str> {
		let mut unit = 0u16;
		for _ in 0..4 {
			let digit = self.next()?.to_digit(16).ok_or(INVALID)?;
			unit = unit * 16 + digit as u16;
		}
		Ok(unit)
	}

	fn string(&mut self) -> Result<Str, &'static str> {
		self.expect('"')?;
		let mut units = Vec::new();
		loop {
			match self.next()? {
				'"' => return Ok(Str::Utf16(units)),
				'\\' => {
					let unit = match self.next()? {
						'"' => 0x22,
						'\\' => 0x5c,
						'/' => 0x2f,
						'b' => 0x08,
						'f' => 0x0c,
						'n' => 0x0a,
						'r' => 0x0d,
						't' => 0x09,
						'u' => self.hex4()?,
						_ => return Err(INVALID),
					};
					units.push(unit);
				}
				c if (c as u32) < 0x20 => return Err(INVALID),
				c => {
					let mut buffer = [0; 2];
					units.extend_from_slice(c.encode_utf16(&mut buffer));
				}
			}
		}
	}

	fn digits(&mut self) -> usize {
		let start = self.at;
		while matches!(self.peek(), Some('0'..='9')) {
			self.at += 1;
		}
		self.at - start
	}

	fn number(&mut self) -> Result<Value, &'static str> {
		let start = self.at;
		if self.peek() == Some('-') {
			self.at += 1;
		}
		match self.peek() {
			Some('0') => self.at += 1,
			Some('1'..='9') => {
				self.digits();
			}
			_ => return Err(INVALID),
		}
		if self.peek() == Some('.') {
			self.at += 1;
			if self.digits() == 0 {
				return Err(INVALID);
			}
		}
		if matches!(self.peek(), Some('e' | 'E')) {
			self.at += 1;
			if matches!(self.peek(), Some('+' | '-')) {
				self.at += 1;
			}
			if self.digits() == 0 {
				return Err(INVALID);
			}
		}
		self.text[start..self.at]
			.parse::<f64>()
			.map(Value::Number)
			.map_err(|_| INVALID)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn text(s: &str) -> Value {
		Value::String(Str::from(s))
	}

	fn object(entries: Vec<(&str, Value)>) -> Value {
		Value::Object(
			entries
				.into_iter()
				.map(|(k, v)| (Str::from(k), v))
				.collect(),
		)
	}

	// The byte-level examples of tools/actual-oracle/test/canonicalJson.test.ts.

	#[test]
	fn sqlite_storage_classes_stay_distinguishable() {
		let row = object(vec![
			("integer", Value::Integer(16384)),
			("real", Value::SqlReal(16384.0)),
			("fraction", Value::SqlReal(0.5)),
			("text", text("16384")),
			("blob", Value::Blob(vec![0, 255])),
			("none", Value::Null),
		]);
		assert_eq!(
			to_canonical(&object(vec![("rows", Value::Array(vec![row]))])),
			"{\n\t\"rows\": [\n\t\t{\"blob\":{\"$blob\":\"00ff\"},\"fraction\":0.5,\"integer\":16384,\"none\":null,\"real\":16384.0,\"text\":\"16384\"}\n\t]\n}\n"
		);
	}

	#[test]
	fn integers_beyond_2_pow_53_keep_every_digit() {
		assert_eq!(
			to_canonical(&Value::Integer(9_007_199_254_740_993)),
			"9007199254740993\n"
		);
	}

	#[test]
	fn reals_use_ecmascript_shortest_round_trip_digits() {
		assert_eq!(format_real(1e21), "1e+21");
		assert_eq!(format_real(1e-7), "1e-7");
		assert_eq!(format_real(-0.0), "0.0");
		assert_eq!(format_real(0.1 + 0.2), "0.30000000000000004");
		assert_eq!(
			to_canonical(&Value::SqlReal(f64::INFINITY)),
			"{\"$real\":\"Infinity\"}\n"
		);
	}

	#[test]
	fn keys_sort_by_code_point_not_by_utf16_unit() {
		let mut keys: Vec<Str> = ["\u{1F600}", "\u{FFFD}", "b", "B", "a"]
			.into_iter()
			.map(|key| Str::Utf16(key.encode_utf16().collect()))
			.collect();
		keys.sort_by(compare_code_points);
		let expected: Vec<Str> = ["B", "a", "b", "\u{FFFD}", "\u{1F600}"]
			.into_iter()
			.map(|key| Str::Utf16(key.encode_utf16().collect()))
			.collect();
		assert_eq!(keys, expected);
	}

	#[test]
	fn empty_containers_and_nested_objects_follow_the_layout_rules() {
		let value = object(vec![
			("b", Value::Array(vec![])),
			("a", object(vec![])),
			(
				"c",
				object(vec![(
					"d",
					Value::Array(vec![
						Value::Number(1.0),
						Value::Array(vec![
							Value::Number(2.0),
							object(vec![("z", Value::Number(1.0)), ("y", Value::Number(2.0))]),
						]),
					]),
				)]),
			),
		]);
		assert_eq!(
			to_canonical(&value),
			"{\n\t\"a\": {},\n\t\"b\": [],\n\t\"c\": {\n\t\t\"d\": [\n\t\t\t1,\n\t\t\t[2,{\"y\":2,\"z\":1}]\n\t\t]\n\t}\n}\n"
		);
	}

	#[test]
	fn strings_escape_only_quotes_backslashes_controls_and_lone_surrogates() {
		let mut units: Vec<u16> = "\u{0000}\u{0008}\t\n\u{000c}\r\u{001f}\"\\/\u{007f} é"
			.encode_utf16()
			.collect();
		units.push(0xd800);
		assert_eq!(
			to_canonical(&Value::String(Str::Utf16(units))),
			"\"\\u0000\\b\\t\\n\\f\\r\\u001f\\\"\\\\/\u{007f} é\\ud800\"\n"
		);
		assert_eq!(
			to_canonical(&text(
				"\u{0000}\u{0008}\t\n\u{000c}\r\u{001f}\"\\/\u{007f} é\u{2028}"
			)),
			"\"\\u0000\\b\\t\\n\\f\\r\\u001f\\\"\\\\/\u{007f} é\u{2028}\"\n"
		);
	}

	#[test]
	fn numbers_print_like_javascript() {
		for (value, expected) in [
			(1.0, "1"),
			(1e3, "1000"),
			(123456789012345680000.0, "123456789012345680000"),
			(1e21, "1e+21"),
			(1.5e-7, "1.5e-7"),
			(0.000001, "0.000001"),
			(0.0000012345, "0.0000012345"),
			(-2.5, "-2.5"),
			(9007199254740993.0, "9007199254740992"),
			(5e-324, "5e-324"),
			(1.7976931348623157e308, "1.7976931348623157e+308"),
			(123.456, "123.456"),
		] {
			assert_eq!(js_number(value), expected, "{value:e}");
		}
	}

	#[test]
	fn stringify_puts_array_index_keys_first_like_json_stringify() {
		// Node: JSON.stringify(JSON.parse(this input)).
		let parsed = parse(
			r#"{"b":1,"10":2,"2":3,"a":4,"01":5,"4294967294":6,"4294967295":7,"-1":8,"b":9}"#,
		)
		.unwrap();
		let mut out = Vec::new();
		stringify(&mut out, &parsed).unwrap();
		assert_eq!(
			String::from_utf8(out).unwrap(),
			r#"{"2":3,"10":2,"4294967294":6,"b":9,"a":4,"01":5,"4294967295":7,"-1":8}"#
		);
	}

	#[test]
	fn stringify_keeps_insertion_order_like_json_stringify() {
		let parsed = parse("{\"b\": 1.50, \"a\": [1e400, \"\\ud800\"], \"b\": true}").unwrap();
		let mut out = Vec::new();
		stringify(&mut out, &parsed).unwrap();
		assert_eq!(
			String::from_utf8(out).unwrap(),
			"{\"b\":true,\"a\":[null,\"\\ud800\"]}"
		);
	}

	#[test]
	fn many_keys_parse_in_linear_time() {
		let keys: Vec<String> = (0..50_000).map(|n| format!("\"k{n}\":{n}")).collect();
		let started = std::time::Instant::now();
		let Value::Object(entries) = parse(&format!("{{{}}}", keys.join(","))).unwrap() else {
			panic!("not an object");
		};
		assert_eq!(entries.len(), 50_000);
		assert!(started.elapsed() < std::time::Duration::from_secs(2));
	}

	#[test]
	fn metadata_parses_like_json_parse() {
		let parsed =
			parse(" {\"a\": 1.0, \"b\": [true, null], \"a\": 2e3, \"s\": \"\\ud800x\\u00e9\"} ")
				.unwrap();
		let Value::Object(entries) = parsed else {
			panic!("not an object");
		};
		assert_eq!(entries.len(), 3);
		assert_eq!(entries[0].1, Value::Number(2000.0));
		assert_eq!(
			entries[2].1,
			Value::String(Str::Utf16(vec![0xd800, u16::from(b'x'), 0xe9]))
		);
		for bad in [
			"",
			"{",
			"{'a': 1}",
			"[1,]",
			"01",
			"1.",
			"+1",
			"\"a\nb\"",
			"{} x",
			"\u{feff}{}",
			"NaN",
		] {
			assert!(parse(bad).is_err(), "{bad:?} should be rejected");
		}
		assert!(parse(&"[".repeat(10_000)).is_err());
	}
}
