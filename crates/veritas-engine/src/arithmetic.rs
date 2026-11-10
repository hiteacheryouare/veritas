//! A port of Actual's `evalArithmetic` and `currencyToAmount`
//! (`packages/loot-core/src/shared/arithmetic.ts` and `util.ts`, v26.11.0),
//! including their JavaScript quirks: trailing junk is ignored, a leading `+`
//! fails, `^` is right-associative and unary minus binds to the literal.

/// Actual's `numberFormat` preference. It decides which separator is the
/// thousands separator when an amount is parsed or shown for editing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NumberFormat {
	#[default]
	CommaDot,
	DotComma,
	SpaceComma,
	ApostropheDot,
	CommaDotIn,
}

impl NumberFormat {
	/// Unknown values fall back to comma-dot, as Actual's `parseNumberFormat` does.
	pub fn from_preference(value: &str) -> Self {
		match value {
			"dot-comma" => NumberFormat::DotComma,
			"space-comma" => NumberFormat::SpaceComma,
			"apostrophe-dot" => NumberFormat::ApostropheDot,
			"comma-dot-in" => NumberFormat::CommaDotIn,
			_ => NumberFormat::CommaDot,
		}
	}

	pub fn thousands_separator(self) -> char {
		match self {
			NumberFormat::CommaDot | NumberFormat::CommaDotIn => ',',
			NumberFormat::DotComma => '.',
			NumberFormat::SpaceComma => '\u{202F}',
			NumberFormat::ApostropheDot => '\u{2019}',
		}
	}

	pub fn decimal_separator(self) -> char {
		match self {
			NumberFormat::CommaDot | NumberFormat::CommaDotIn | NumberFormat::ApostropheDot => '.',
			NumberFormat::DotComma | NumberFormat::SpaceComma => ',',
		}
	}
}

/// Evaluates an amount expression in major units. `None` is Actual's default
/// value: empty input, a syntax error or a `NaN` result. Infinity is returned
/// as is, exactly like Actual, so callers must check `is_finite`.
pub fn eval_arithmetic(expression: &str, format: NumberFormat) -> Option<f64> {
	if expression.is_empty() {
		return None;
	}
	let mut parser = Parser {
		chars: expression
			.chars()
			.filter(|c| !is_js_whitespace(*c))
			.collect(),
		index: 0,
		depth: 0,
		format,
	};
	let result = parser.additive().ok()?;
	if result.is_nan() { None } else { Some(result) }
}

/// Parses one amount, choosing between thousands and decimal separator by the
/// last `,` or `.`, as Actual's `currencyToAmount` does.
pub fn currency_to_amount(text: &str, format: NumberFormat) -> Option<f64> {
	let text = text.replace('\u{2212}', "-");
	let keep_digits_and_minus = |s: &str| -> String {
		s.chars()
			.filter(|c| c.is_ascii_digit() || *c == '-')
			.collect()
	};
	let (integer, fraction) = match text.rfind([',', '.']) {
		Some(index) => {
			let separator = text[index..].chars().next();
			let after = &text[index + 1..];
			// JavaScript measures the three-digit rule in UTF-16 code units.
			if separator == Some(format.thousands_separator()) && after.encode_utf16().count() >= 3
			{
				(keep_digits_and_minus(&text), None)
			} else {
				(keep_digits_and_minus(&text[..index]), Some(after))
			}
		}
		None => (keep_digits_and_minus(&text), None),
	};
	let joined = format!("{integer}.{}", fraction.unwrap_or("null"));
	js_parse_float(&joined)
}

/// JavaScript's `parseFloat`: the longest numeric prefix after leading
/// whitespace, or `None` where JavaScript gives `NaN`.
fn js_parse_float(text: &str) -> Option<f64> {
	let text = text.trim_start_matches(is_js_whitespace);
	let (negative, rest) = match text.as_bytes().first() {
		Some(b'-') => (true, &text[1..]),
		Some(b'+') => (false, &text[1..]),
		_ => (false, text),
	};
	let sign = if negative { -1.0 } else { 1.0 };
	if rest.starts_with("Infinity") {
		return Some(sign * f64::INFINITY);
	}
	let bytes = rest.as_bytes();
	let digits_from = |start: usize| {
		bytes[start..]
			.iter()
			.take_while(|b| b.is_ascii_digit())
			.count()
	};
	let whole = digits_from(0);
	let mut end = whole;
	let mut fraction = 0;
	if bytes.get(end) == Some(&b'.') {
		fraction = digits_from(end + 1);
		end += 1 + fraction;
	}
	if whole == 0 && fraction == 0 {
		return None;
	}
	let mut exponent = "";
	if matches!(bytes.get(end), Some(b'e' | b'E')) {
		let sign_len = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
		let exponent_digits = digits_from(end + 1 + sign_len);
		if exponent_digits > 0 {
			exponent = &rest[end..end + 1 + sign_len + exponent_digits];
		}
	}
	let fraction_digits = &rest[(whole + 1).min(end)..end];
	let whole_digits = &rest[..whole];
	let or_zero = |digits: &str| {
		if digits.is_empty() {
			"0".to_owned()
		} else {
			digits.to_owned()
		}
	};
	format!(
		"{}.{}{exponent}",
		or_zero(whole_digits),
		or_zero(fraction_digits)
	)
	.parse::<f64>()
	.ok()
	.map(|value| sign * value)
}

/// The characters JavaScript's `\s` matches (WhiteSpace and LineTerminator).
fn is_js_whitespace(c: char) -> bool {
	matches!(
		c,
		'\u{9}'..='\u{D}'
			| ' ' | '\u{A0}'
			| '\u{1680}'
			| '\u{2000}'..='\u{200A}'
			| '\u{2028}'
			| '\u{2029}'
			| '\u{202F}'
			| '\u{205F}'
			| '\u{3000}'
			| '\u{FEFF}'
	)
}

/// Actual's number-token class, `[0-9,.'’   ]|\p{Sc}`. The
/// parser tests one UTF-16 code unit at a time, so currency signs outside the
/// Basic Multilingual Plane never match.
fn is_number_char(c: char) -> bool {
	c.is_ascii_digit()
		|| matches!(
			c,
			',' | '.' | '\'' | '\u{2019}' | '\u{A0}' | '\u{202F}' | ' '
		) || is_bmp_currency_symbol(c)
}

/// Unicode 17 General Category Sc, the set Node 26 (ICU 78) uses.
fn is_bmp_currency_symbol(c: char) -> bool {
	matches!(
		c,
		'$' | '\u{A2}'..='\u{A5}'
			| '\u{58F}'
			| '\u{60B}'
			| '\u{7FE}'..='\u{7FF}'
			| '\u{9F2}'..='\u{9F3}'
			| '\u{9FB}'
			| '\u{AF1}'
			| '\u{BF9}'
			| '\u{E3F}'
			| '\u{17DB}'
			| '\u{20A0}'..='\u{20C1}'
			| '\u{A838}'
			| '\u{FDFC}'
			| '\u{FE69}'
			| '\u{FF04}'
			| '\u{FFE0}'..='\u{FFE1}'
			| '\u{FFE5}'..='\u{FFE6}'
	)
}

/// JavaScript's `Math.pow` returns NaN where C's `pow` returns 1.
fn js_pow(base: f64, exponent: f64) -> f64 {
	if exponent.is_nan() || (exponent.is_infinite() && base.abs() == 1.0) {
		f64::NAN
	} else {
		base.powf(exponent)
	}
}

struct SyntaxError;

// JavaScript would hit its own stack limit on absurd nesting and return the
// default value too. This keeps hostile input from overflowing our stack.
const MAX_DEPTH: usize = 256;

struct Parser {
	chars: Vec<char>,
	index: usize,
	depth: usize,
	format: NumberFormat,
}

impl Parser {
	fn peek(&self) -> Option<char> {
		self.chars.get(self.index).copied()
	}

	fn eat(&mut self, expected: char) -> bool {
		let matched = self.peek() == Some(expected);
		if matched {
			self.index += 1;
		}
		matched
	}

	fn descend(&mut self) -> Result<(), SyntaxError> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			Err(SyntaxError)
		} else {
			Ok(())
		}
	}

	fn additive(&mut self) -> Result<f64, SyntaxError> {
		let mut value = self.multiplicative()?;
		loop {
			if self.eat('+') {
				value += self.multiplicative()?;
			} else if self.eat('-') {
				value -= self.multiplicative()?;
			} else {
				return Ok(value);
			}
		}
	}

	fn multiplicative(&mut self) -> Result<f64, SyntaxError> {
		let mut value = self.exponent()?;
		loop {
			if self.eat('*') {
				value *= self.exponent()?;
			} else if self.eat('/') {
				value /= self.exponent()?;
			} else {
				return Ok(value);
			}
		}
	}

	fn exponent(&mut self) -> Result<f64, SyntaxError> {
		let base = self.parens()?;
		if !self.eat('^') {
			return Ok(base);
		}
		self.descend()?;
		let exponent = self.exponent()?;
		self.depth -= 1;
		Ok(js_pow(base, exponent))
	}

	fn parens(&mut self) -> Result<f64, SyntaxError> {
		if !self.eat('(') {
			return self.primary();
		}
		self.descend()?;
		let value = self.additive()?;
		self.depth -= 1;
		if self.eat(')') {
			Ok(value)
		} else {
			Err(SyntaxError)
		}
	}

	fn primary(&mut self) -> Result<f64, SyntaxError> {
		let negative = self.eat('-');
		let start = self.index;
		while self.peek().is_some_and(is_number_char) {
			self.index += 1;
		}
		if self.index == start {
			return Err(SyntaxError);
		}
		let token: String = self.chars[start..self.index].iter().collect();
		let value = currency_to_amount(&token, self.format).ok_or(SyntaxError)?;
		Ok(if negative { -value } else { value })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn eval(expression: &str) -> Option<f64> {
		eval_arithmetic(expression, NumberFormat::CommaDot)
	}

	#[test]
	fn js_parse_float_takes_the_longest_numeric_prefix() {
		assert_eq!(js_parse_float("12.5abc"), Some(12.5));
		assert_eq!(js_parse_float("-.5"), Some(-0.5));
		assert_eq!(js_parse_float("5."), Some(5.0));
		assert_eq!(js_parse_float("1.5e3x"), Some(1500.0));
		assert_eq!(js_parse_float("1.5e"), Some(1.5));
		assert_eq!(js_parse_float("1234.null"), Some(1234.0));
		assert_eq!(js_parse_float(".null"), None);
		assert_eq!(js_parse_float("-"), None);
		assert_eq!(js_parse_float("-Infinity"), Some(f64::NEG_INFINITY));
	}

	#[test]
	fn pow_follows_javascript_not_c_for_one_to_the_infinity() {
		assert!(js_pow(1.0, f64::INFINITY).is_nan());
		assert!(js_pow(-1.0, f64::NEG_INFINITY).is_nan());
		assert!(js_pow(1.0, f64::NAN).is_nan());
		assert_eq!(js_pow(f64::NAN, 0.0), 1.0);
		assert_eq!(eval("1^(1/0)"), None);
	}

	#[test]
	fn hostile_nesting_returns_the_default_instead_of_overflowing_the_stack() {
		let deep = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
		assert_eq!(eval(&deep), None);
		let tower = vec!["1"; 100_000].join("^");
		assert_eq!(eval(&tower), None);
		assert_eq!(
			eval(&format!("{}1{}", "(".repeat(200), ")".repeat(200))),
			Some(1.0)
		);
	}

	#[test]
	fn unknown_preferences_fall_back_to_comma_dot() {
		assert_eq!(
			NumberFormat::from_preference("space-comma"),
			NumberFormat::SpaceComma
		);
		assert_eq!(
			NumberFormat::from_preference("bogus"),
			NumberFormat::CommaDot
		);
	}
}
