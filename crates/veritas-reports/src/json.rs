//! Just enough JSON for Actual's report and widget columns: key order is kept,
//! numbers keep their source text, and output matches `JSON.stringify`, so an
//! untouched value is written back byte for byte.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
	Null,
	Bool(bool),
	/// The number's text as written, so nothing is lost to float rounding.
	Number(String),
	String(String),
	Array(Vec<Json>),
	Object(Vec<(String, Json)>),
}

impl Json {
	pub fn parse(text: &str) -> Result<Json, String> {
		let mut parser = Parser {
			chars: text.chars().collect(),
			at: 0,
		};
		parser.skip_space();
		let value = parser.value()?;
		parser.skip_space();
		if parser.at != parser.chars.len() {
			return Err(format!("unexpected text at {}", parser.at));
		}
		Ok(value)
	}

	pub fn get(&self, key: &str) -> Option<&Json> {
		match self {
			Json::Object(entries) => entries
				.iter()
				.find(|(name, _)| name == key)
				.map(|(_, value)| value),
			_ => None,
		}
	}

	pub fn as_str(&self) -> Option<&str> {
		match self {
			Json::String(text) => Some(text),
			_ => None,
		}
	}

	pub fn as_bool(&self) -> Option<bool> {
		match self {
			Json::Bool(value) => Some(*value),
			_ => None,
		}
	}

	pub fn as_i64(&self) -> Option<i64> {
		match self {
			Json::Number(text) => text.parse::<i64>().ok().or_else(|| {
				text.parse::<f64>()
					.ok()
					.filter(|n| n.fract() == 0.0)
					.map(|n| n as i64)
			}),
			_ => None,
		}
	}

	pub fn as_f64(&self) -> Option<f64> {
		match self {
			Json::Number(text) => text.parse().ok(),
			_ => None,
		}
	}

	pub fn as_array(&self) -> Option<&[Json]> {
		match self {
			Json::Array(items) => Some(items),
			_ => None,
		}
	}

	/// The text `JSON.stringify` produces for this value.
	pub fn stringify(&self) -> String {
		let mut out = String::new();
		self.write(&mut out);
		out
	}

	fn write(&self, out: &mut String) {
		match self {
			Json::Null => out.push_str("null"),
			Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
			Json::Number(text) => out.push_str(text),
			Json::String(text) => write_string(out, text),
			Json::Array(items) => {
				out.push('[');
				for (index, item) in items.iter().enumerate() {
					if index > 0 {
						out.push(',');
					}
					item.write(out);
				}
				out.push(']');
			}
			Json::Object(entries) => {
				out.push('{');
				for (index, (key, value)) in entries.iter().enumerate() {
					if index > 0 {
						out.push(',');
					}
					write_string(out, key);
					out.push(':');
					value.write(out);
				}
				out.push('}');
			}
		}
	}
}

fn write_string(out: &mut String, text: &str) {
	out.push('"');
	for c in text.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\u{08}' => out.push_str("\\b"),
			'\u{0C}' => out.push_str("\\f"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if (c as u32) < 0x20 => {
				let _ = write!(out, "\\u{:04x}", c as u32);
			}
			c => out.push(c),
		}
	}
	out.push('"');
}

struct Parser {
	chars: Vec<char>,
	at: usize,
}

impl Parser {
	fn peek(&self) -> Option<char> {
		self.chars.get(self.at).copied()
	}

	fn next(&mut self) -> Result<char, String> {
		let c = self.peek().ok_or("unexpected end of JSON")?;
		self.at += 1;
		Ok(c)
	}

	fn skip_space(&mut self) {
		while self
			.peek()
			.is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
		{
			self.at += 1;
		}
	}

	fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
		for expected in word.chars() {
			if self.next()? != expected {
				return Err(format!("expected {word}"));
			}
		}
		Ok(value)
	}

	fn value(&mut self) -> Result<Json, String> {
		match self.peek().ok_or("unexpected end of JSON")? {
			'n' => self.literal("null", Json::Null),
			't' => self.literal("true", Json::Bool(true)),
			'f' => self.literal("false", Json::Bool(false)),
			'"' => self.string().map(Json::String),
			'[' => {
				self.at += 1;
				let mut items = Vec::new();
				self.skip_space();
				if self.peek() == Some(']') {
					self.at += 1;
					return Ok(Json::Array(items));
				}
				loop {
					self.skip_space();
					items.push(self.value()?);
					self.skip_space();
					match self.next()? {
						',' => {}
						']' => return Ok(Json::Array(items)),
						_ => return Err("expected , or ]".to_owned()),
					}
				}
			}
			'{' => {
				self.at += 1;
				let mut entries = Vec::new();
				self.skip_space();
				if self.peek() == Some('}') {
					self.at += 1;
					return Ok(Json::Object(entries));
				}
				loop {
					self.skip_space();
					let key = self.string()?;
					self.skip_space();
					if self.next()? != ':' {
						return Err("expected :".to_owned());
					}
					self.skip_space();
					let value = self.value()?;
					match entries.iter_mut().find(|(name, _)| name == &key) {
						Some(entry) => entry.1 = value,
						None => entries.push((key, value)),
					}
					self.skip_space();
					match self.next()? {
						',' => {}
						'}' => return Ok(Json::Object(entries)),
						_ => return Err("expected , or }".to_owned()),
					}
				}
			}
			_ => self.number(),
		}
	}

	fn number(&mut self) -> Result<Json, String> {
		let start = self.at;
		while self
			.peek()
			.is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
		{
			self.at += 1;
		}
		let text: String = self
			.chars
			.get(start..self.at)
			.unwrap_or_default()
			.iter()
			.collect();
		if text.parse::<f64>().is_err() {
			return Err(format!("bad number at {start}"));
		}
		Ok(Json::Number(text))
	}

	fn string(&mut self) -> Result<String, String> {
		if self.next()? != '"' {
			return Err("expected a string".to_owned());
		}
		let mut out = String::new();
		loop {
			match self.next()? {
				'"' => return Ok(out),
				'\\' => match self.next()? {
					'"' => out.push('"'),
					'\\' => out.push('\\'),
					'/' => out.push('/'),
					'b' => out.push('\u{08}'),
					'f' => out.push('\u{0C}'),
					'n' => out.push('\n'),
					'r' => out.push('\r'),
					't' => out.push('\t'),
					'u' => {
						let high = self.hex4()?;
						let code = if (0xD800..0xDC00).contains(&high) && self.peek() == Some('\\')
						{
							self.at += 1;
							if self.next()? != 'u' {
								return Err("expected a low surrogate".to_owned());
							}
							let low = self.hex4()?;
							0x10000 + ((high - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF)
						} else {
							high
						};
						out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
					}
					_ => return Err("bad escape".to_owned()),
				},
				c => out.push(c),
			}
		}
	}

	fn hex4(&mut self) -> Result<u32, String> {
		let mut code = 0;
		for _ in 0..4 {
			let digit = self.next()?.to_digit(16).ok_or("bad \\u escape")?;
			code = code * 16 + digit;
		}
		Ok(code)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips_actual_report_columns_byte_for_byte() {
		for text in [
			r#"[{"field":"category","op":"oneOf","value":["a","b"],"type":"id"}]"#,
			r#"{"name":"Avg dining","content":"{\"type\":\"avgPerMonth\"}","conditions":[]}"#,
			r#"{"content":"Budget notes\n\nPay the card **in full**.","text_align":"left"}"#,
			r#"{"n":1.5,"big":12345678901234567890,"neg":-0.25,"e":1e3,"u":"é\u0001"}"#,
			"[]",
			"{}",
		] {
			let parsed = Json::parse(text).unwrap();
			let again = Json::parse(&parsed.stringify()).unwrap();
			assert_eq!(parsed, again);
		}
		let exact = r#"[{"field":"category","op":"oneOf","value":["a","b"],"type":"id"}]"#;
		assert_eq!(Json::parse(exact).unwrap().stringify(), exact);
	}

	#[test]
	fn stringify_escapes_like_javascript() {
		let value = Json::String("a\"b\\c\n\t\u{1}é/".to_owned());
		assert_eq!(value.stringify(), r#""a\"b\\c\n\t\u0001é/""#);
		assert!(Json::parse("[1,").is_err());
		assert!(Json::parse("{\"a\":1} x").is_err());
	}
}
