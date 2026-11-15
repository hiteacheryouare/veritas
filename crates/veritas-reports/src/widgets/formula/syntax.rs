//! HyperFormula 3.5.0's lexer and parser (`es/parser/LexerConfig.mjs`,
//! `FormulaParser.mjs`) for Actual's config: ',' separates arguments and
//! array columns, ';' array rows, '.' is the decimal point. Chevrotain takes
//! the first token pattern that matches, in `allTokens` order, so the lexer
//! tries them in that order too. Cell references, ranges and OFFSET parse,
//! but evaluating one is refused, since a formula card has no other cells.

use super::ErrorKind;

#[derive(Clone, Debug, PartialEq)]
enum Token {
	Plus,
	Minus,
	Times,
	Div,
	Power,
	Equals,
	NotEqual,
	Percent,
	GreaterOrEqual,
	LessOrEqual,
	Greater,
	Less,
	LParen,
	RParen,
	LBrace,
	RBrace,
	Procedure(String),
	Colon,
	Number(f64),
	Text(String),
	ErrorLiteral(String),
	Ampersand,
	Name(String),
	Semicolon,
	Comma,
	/// A cell reference, column range or row range.
	Reference(&'static str),
	Offset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BinaryOp {
	Add,
	Subtract,
	Multiply,
	Divide,
	Power,
	Concatenate,
	Equal,
	NotEqual,
	Greater,
	Less,
	GreaterOrEqual,
	LessOrEqual,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Ast {
	Empty,
	Number(f64),
	Text(String),
	Error(ErrorKind),
	Name(String),
	Negate(Box<Ast>),
	Plus(Box<Ast>),
	Percent(Box<Ast>),
	Binary(BinaryOp, Box<Ast>, Box<Ast>),
	Call(String, Vec<Ast>),
	Parenthesis(Box<Ast>),
	Array(Vec<Vec<Ast>>),
	/// A reference or OFFSET, which a formula card can't evaluate.
	Unsupported(&'static str),
}

/// `UNICODE_LETTER_PATTERN`: `A-Za-zÀ-ʯ`.
fn is_letter(c: char) -> bool {
	c.is_ascii_alphabetic() || ('\u{C0}'..='\u{2AF}').contains(&c)
}

/// `NON_RESERVED_CHARACTER_PATTERN`.
fn is_name_char(c: char) -> bool {
	is_letter(c) || c.is_ascii_digit() || c == '_' || c == '.'
}

/// Characters the ODFF whitespace token covers (`ignoreWhiteSpace:
/// 'standard'`).
fn is_whitespace(c: char) -> bool {
	matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn count_while(chars: &[char], from: usize, test: impl Fn(char) -> bool) -> usize {
	chars[from.min(chars.len())..]
		.iter()
		.take_while(|c| test(**c))
		.count()
}

/// `SHEET_NAME_PATTERN` followed by `!`: the length it covers at `at`.
fn sheet_prefix(chars: &[char], at: usize) -> Option<usize> {
	if chars.get(at) == Some(&'\'') {
		let mut i = at + 1;
		loop {
			match chars.get(i) {
				Some('\'') if chars.get(i + 1) == Some(&'\'') => i += 2,
				Some('\'') => break,
				Some(_) => i += 1,
				None => return None,
			}
		}
		return (chars.get(i + 1) == Some(&'!')).then_some(i + 2 - at);
	}
	let length = count_while(chars, at, |c| {
		is_letter(c) || c.is_ascii_digit() || c == '_'
	});
	(length > 0 && chars.get(at + length) == Some(&'!')).then_some(length + 1)
}

/// `(sheet!)?\$?` then `part`, which must match at least one character.
fn reference_part(chars: &[char], at: usize, part: fn(char) -> bool) -> Option<usize> {
	let try_from = |start: usize| {
		let dollar = usize::from(chars.get(start) == Some(&'$'));
		let length = count_while(chars, start + dollar, part);
		(length > 0).then_some(start + dollar + length)
	};
	sheet_prefix(chars, at)
		.and_then(|prefix| try_from(at + prefix))
		.or_else(|| try_from(at))
}

/// `ColumnRange` or `RowRange`: a part, ':', then another part.
fn is_range(chars: &[char], at: usize, part: fn(char) -> bool) -> bool {
	reference_part(chars, at, part)
		.filter(|end| chars.get(*end) == Some(&':'))
		.and_then(|end| reference_part(chars, end + 1, part))
		.is_some()
}

fn range_end(chars: &[char], at: usize, part: fn(char) -> bool) -> usize {
	let middle = reference_part(chars, at, part).unwrap_or(at);
	reference_part(chars, middle + 1, part).unwrap_or(middle + 1)
}

/// Where the cell reference `is_cell_reference` found at `at` ends.
fn cell_reference_end(chars: &[char], at: usize) -> usize {
	let from = |start: usize| {
		let mut i = start + usize::from(chars.get(start) == Some(&'$'));
		i += count_while(chars, i, |c| c.is_ascii_alphabetic());
		i += usize::from(chars.get(i) == Some(&'$'));
		i + count_while(chars, i, |c| c.is_ascii_digit())
	};
	match sheet_prefix(chars, at) {
		Some(prefix) if is_cell_reference_from(chars, at + prefix) => from(at + prefix),
		_ => from(at),
	}
}

/// `\$?[A-Za-z]+\$?[0-9]+` at `start`, not followed by a name character.
fn is_cell_reference_from(chars: &[char], start: usize) -> bool {
	let dollar = usize::from(chars.get(start) == Some(&'$'));
	let letters = count_while(chars, start + dollar, |c| c.is_ascii_alphabetic());
	if letters == 0 {
		return false;
	}
	let mut end = start + dollar + letters;
	end += usize::from(chars.get(end) == Some(&'$'));
	let digits = count_while(chars, end, |c| c.is_ascii_digit());
	digits > 0
		&& chars
			.get(end + digits)
			.is_none_or(|next| !is_name_char(*next))
}

/// `CELL_REFERENCE_WITH_NEXT_CHARACTER_PATTERN`.
fn is_cell_reference(chars: &[char], at: usize) -> bool {
	sheet_prefix(chars, at).is_some_and(|prefix| is_cell_reference_from(chars, at + prefix))
		|| is_cell_reference_from(chars, at)
}

/// `NumberLiteral`: `(\.\d+|\d+(\.\d*)?)(e[+-]?\d+)?`.
fn number_literal(chars: &[char], at: usize) -> Option<(usize, f64)> {
	let mut i = at;
	let whole = count_while(chars, i, |c| c.is_ascii_digit());
	i += whole;
	if whole == 0 {
		if chars.get(i) != Some(&'.') {
			return None;
		}
		let fraction = count_while(chars, i + 1, |c| c.is_ascii_digit());
		if fraction == 0 {
			return None;
		}
		i += 1 + fraction;
	} else if chars.get(i) == Some(&'.') {
		i += 1 + count_while(chars, i + 1, |c| c.is_ascii_digit());
	}
	if chars.get(i) == Some(&'e') {
		let sign = usize::from(matches!(chars.get(i + 1), Some('+' | '-')));
		let digits = count_while(chars, i + 1 + sign, |c| c.is_ascii_digit());
		if digits > 0 {
			i += 1 + sign + digits;
		}
	}
	let text: String = chars[at..i].iter().collect();
	let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
	let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
	let value = format!(
		"{}.{}e{exponent}",
		if whole.is_empty() { "0" } else { whole },
		if fraction.is_empty() { "0" } else { fraction }
	)
	.parse()
	.ok()?;
	Some((i - at, value))
}

/// `StringLiteral`: `"([^"\\]*(\\.[^"\\]*)*)"`, kept as written between
/// the quotes; HyperFormula unescapes nothing.
fn text_literal(chars: &[char], at: usize) -> Option<(usize, String)> {
	if chars.get(at) != Some(&'"') {
		return None;
	}
	let mut i = at + 1;
	loop {
		match chars.get(i)? {
			'"' => break,
			'\\' => match chars.get(i + 1)? {
				'\n' | '\r' | '\u{2028}' | '\u{2029}' => return None,
				_ => i += 2,
			},
			_ => i += 1,
		}
	}
	Some((i + 1 - at, chars[at + 1..i].iter().collect()))
}

enum Lexed {
	Tokens(Vec<Token>),
	Error,
}

fn tokenize(formula: &str) -> Lexed {
	let chars: Vec<char> = formula.chars().collect();
	let mut tokens = Vec::new();
	let mut i = 0;
	while i < chars.len() {
		let c = chars[i];
		let rest_starts = |text: &str| {
			text.chars()
				.enumerate()
				.all(|(offset, expected)| chars.get(i + offset) == Some(&expected))
		};
		if is_whitespace(c) {
			i += count_while(&chars, i, is_whitespace);
			continue;
		}
		let fixed = match c {
			'+' => Some((Token::Plus, 1)),
			'-' => Some((Token::Minus, 1)),
			'*' => Some((Token::Times, 1)),
			'/' => Some((Token::Div, 1)),
			'^' => Some((Token::Power, 1)),
			'=' => Some((Token::Equals, 1)),
			'<' if rest_starts("<>") => Some((Token::NotEqual, 2)),
			'%' => Some((Token::Percent, 1)),
			'>' if rest_starts(">=") => Some((Token::GreaterOrEqual, 2)),
			'<' if rest_starts("<=") => Some((Token::LessOrEqual, 2)),
			'>' => Some((Token::Greater, 1)),
			'<' => Some((Token::Less, 1)),
			'(' => Some((Token::LParen, 1)),
			')' => Some((Token::RParen, 1)),
			'{' => Some((Token::LBrace, 1)),
			'}' => Some((Token::RBrace, 1)),
			_ => None,
		};
		if let Some((token, length)) = fixed {
			tokens.push(token);
			i += length;
			continue;
		}
		let upper: String = chars[i..chars.len().min(i + 6)]
			.iter()
			.collect::<String>()
			.to_uppercase();
		if upper == "OFFSET" {
			tokens.push(Token::Offset);
			i += 6;
			continue;
		}
		if is_letter(c) {
			let length = count_while(&chars, i, is_name_char);
			if chars.get(i + length) == Some(&'(') {
				let name: String = chars[i..i + length].iter().collect();
				tokens.push(Token::Procedure(name.to_uppercase()));
				i += length + 1;
				continue;
			}
		}
		if c == ':' {
			tokens.push(Token::Colon);
			i += 1;
			continue;
		}
		if is_range(&chars, i, |c| c.is_ascii_alphabetic())
			|| is_range(&chars, i, |c| c.is_ascii_digit())
		{
			let part: fn(char) -> bool = if is_range(&chars, i, |c| c.is_ascii_alphabetic()) {
				|c| c.is_ascii_alphabetic()
			} else {
				|c| c.is_ascii_digit()
			};
			i = range_end(&chars, i, part);
			tokens.push(Token::Reference("a range"));
			continue;
		}
		if let Some((length, value)) = number_literal(&chars, i) {
			tokens.push(Token::Number(value));
			i += length;
			continue;
		}
		if let Some((length, text)) = text_literal(&chars, i) {
			tokens.push(Token::Text(text));
			i += length;
			continue;
		}
		if c == '#' {
			let body = count_while(&chars, i + 1, |c| c.is_ascii_alphanumeric() || c == '/');
			if body > 0 {
				let mark = usize::from(matches!(chars.get(i + 1 + body), Some('?' | '!')));
				let image: String = chars[i..i + 1 + body + mark].iter().collect();
				tokens.push(Token::ErrorLiteral(image.to_uppercase()));
				i += 1 + body + mark;
				continue;
			}
		}
		if c == '&' {
			tokens.push(Token::Ampersand);
			i += 1;
			continue;
		}
		if is_cell_reference(&chars, i) {
			i = cell_reference_end(&chars, i);
			tokens.push(Token::Reference("a cell reference"));
			continue;
		}
		if is_letter(c) || c == '_' {
			let length = count_while(&chars, i, is_name_char);
			let name: String = chars[i..i + length].iter().collect();
			if !is_r1c1(&name) {
				tokens.push(Token::Name(name));
				i += length;
				continue;
			}
		}
		match c {
			';' => tokens.push(Token::Semicolon),
			',' => tokens.push(Token::Comma),
			_ => return Lexed::Error,
		}
		i += 1;
	}
	Lexed::Tokens(tokens)
}

/// `R1C1_CELL_REFERENCE_PATTERN`, which a named expression may not be.
fn is_r1c1(name: &str) -> bool {
	let mut chars = name.chars().peekable();
	if !matches!(chars.next(), Some('r' | 'R')) {
		return false;
	}
	while chars.peek().is_some_and(char::is_ascii_digit) {
		chars.next();
	}
	if !matches!(chars.next(), Some('c' | 'C')) {
		return false;
	}
	chars.all(|c| c.is_ascii_digit())
}

fn error_literal(image: &str) -> Option<ErrorKind> {
	Some(match image {
		"#CYCLE!" => ErrorKind::Cycle,
		"#DIV/0!" => ErrorKind::DivByZero,
		"#ERROR!" => ErrorKind::Error,
		"#N/A" => ErrorKind::Na,
		"#NAME?" => ErrorKind::Name,
		"#NUM!" => ErrorKind::Num,
		"#REF!" => ErrorKind::Ref,
		"#SPILL!" => ErrorKind::Spill,
		"#VALUE!" => ErrorKind::Value,
		_ => return None,
	})
}

struct Parser {
	tokens: Vec<Token>,
	at: usize,
}

/// A grammar mismatch; HyperFormula turns any of them into `#ERROR!`.
struct Mismatch;

impl Parser {
	fn peek(&self) -> Option<&Token> {
		self.tokens.get(self.at)
	}

	fn next(&mut self) -> Option<Token> {
		let token = self.tokens.get(self.at).cloned();
		self.at += 1;
		token
	}

	fn expect(&mut self, token: &Token) -> Result<(), Mismatch> {
		if self.peek() == Some(token) {
			self.at += 1;
			Ok(())
		} else {
			Err(Mismatch)
		}
	}

	fn starts_expression(&self) -> bool {
		matches!(
			self.peek(),
			Some(
				Token::Plus
					| Token::Minus | Token::LBrace
					| Token::LParen | Token::Procedure(_)
					| Token::Name(_)
					| Token::Number(_)
					| Token::Text(_)
					| Token::ErrorLiteral(_)
					| Token::Reference(_)
					| Token::Offset
			)
		)
	}

	fn comparison(&mut self) -> Result<Ast, Mismatch> {
		let mut left = self.concatenation()?;
		loop {
			let op = match self.peek() {
				Some(Token::Equals) => BinaryOp::Equal,
				Some(Token::NotEqual) => BinaryOp::NotEqual,
				Some(Token::Greater) => BinaryOp::Greater,
				Some(Token::Less) => BinaryOp::Less,
				Some(Token::GreaterOrEqual) => BinaryOp::GreaterOrEqual,
				Some(Token::LessOrEqual) => BinaryOp::LessOrEqual,
				_ => return Ok(left),
			};
			self.at += 1;
			let right = self.concatenation()?;
			left = Ast::Binary(op, Box::new(left), Box::new(right));
		}
	}

	fn concatenation(&mut self) -> Result<Ast, Mismatch> {
		let mut left = self.addition()?;
		while self.peek() == Some(&Token::Ampersand) {
			self.at += 1;
			let right = self.addition()?;
			left = Ast::Binary(BinaryOp::Concatenate, Box::new(left), Box::new(right));
		}
		Ok(left)
	}

	fn addition(&mut self) -> Result<Ast, Mismatch> {
		let mut left = self.multiplication()?;
		loop {
			let op = match self.peek() {
				Some(Token::Plus) => BinaryOp::Add,
				Some(Token::Minus) => BinaryOp::Subtract,
				_ => return Ok(left),
			};
			self.at += 1;
			let right = self.multiplication()?;
			left = Ast::Binary(op, Box::new(left), Box::new(right));
		}
	}

	fn multiplication(&mut self) -> Result<Ast, Mismatch> {
		let mut left = self.power()?;
		loop {
			let op = match self.peek() {
				Some(Token::Times) => BinaryOp::Multiply,
				Some(Token::Div) => BinaryOp::Divide,
				_ => return Ok(left),
			};
			self.at += 1;
			let right = self.power()?;
			left = Ast::Binary(op, Box::new(left), Box::new(right));
		}
	}

	/// Power sits below the sign, so `-2^2` is `(-2)^2`, and chains to the
	/// left.
	fn power(&mut self) -> Result<Ast, Mismatch> {
		let mut left = self.atomic()?;
		while self.peek() == Some(&Token::Power) {
			self.at += 1;
			let right = self.atomic()?;
			left = Ast::Binary(BinaryOp::Power, Box::new(left), Box::new(right));
		}
		Ok(left)
	}

	fn atomic(&mut self) -> Result<Ast, Mismatch> {
		match self.peek() {
			Some(Token::Plus) => {
				self.at += 1;
				Ok(Ast::Plus(Box::new(self.atomic()?)))
			}
			Some(Token::Minus) => {
				self.at += 1;
				Ok(Ast::Negate(Box::new(self.atomic()?)))
			}
			_ => {
				let value = self.positive()?;
				if self.peek() == Some(&Token::Percent) {
					self.at += 1;
					return Ok(Ast::Percent(Box::new(value)));
				}
				Ok(value)
			}
		}
	}

	fn positive(&mut self) -> Result<Ast, Mismatch> {
		match self.next() {
			Some(Token::LBrace) => {
				let mut rows = vec![vec![self.comparison()?]];
				loop {
					match self.peek() {
						Some(Token::Comma) => {
							self.at += 1;
							let value = self.comparison()?;
							rows.last_mut().ok_or(Mismatch)?.push(value);
						}
						Some(Token::Semicolon) => {
							self.at += 1;
							rows.push(vec![self.comparison()?]);
						}
						_ => break,
					}
				}
				self.expect(&Token::RBrace)?;
				Ok(Ast::Array(rows))
			}
			Some(Token::LParen) => {
				let inner = self.comparison()?;
				self.expect(&Token::RParen)?;
				Ok(Ast::Parenthesis(Box::new(inner)))
			}
			Some(Token::Procedure(name)) => {
				let mut args = vec![self.argument()?];
				while self.peek() == Some(&Token::Comma) {
					self.at += 1;
					args.push(self.argument()?);
				}
				if args.len() == 1 && args[0] == Ast::Empty {
					args.clear();
				}
				self.expect(&Token::RParen)?;
				Ok(Ast::Call(name, args))
			}
			Some(Token::Name(name)) => Ok(Ast::Name(name)),
			Some(Token::Number(value)) => Ok(Ast::Number(value)),
			Some(Token::Text(text)) => Ok(Ast::Text(text)),
			Some(Token::Reference(what)) => {
				if self.peek() == Some(&Token::Colon) {
					self.at += 1;
					match self.next() {
						Some(Token::Reference(_)) => {}
						Some(Token::Offset) => self.offset()?,
						_ => return Err(Mismatch),
					}
				}
				Ok(Ast::Unsupported(what))
			}
			Some(Token::Offset) => {
				self.offset()?;
				if self.peek() == Some(&Token::Colon) {
					self.at += 1;
					match self.next() {
						Some(Token::Reference(_)) => {}
						Some(Token::Offset) => self.offset()?,
						_ => return Err(Mismatch),
					}
				}
				Ok(Ast::Unsupported("OFFSET"))
			}
			Some(Token::ErrorLiteral(image)) => {
				error_literal(&image).map(Ast::Error).ok_or(Mismatch)
			}
			_ => Err(Mismatch),
		}
	}

	/// OFFSET's parenthesised arguments, after its name.
	fn offset(&mut self) -> Result<(), Mismatch> {
		self.expect(&Token::LParen)?;
		if self.peek() != Some(&Token::RParen) {
			self.comparison()?;
			while self.peek() == Some(&Token::Comma) {
				self.at += 1;
				self.comparison()?;
			}
		}
		self.expect(&Token::RParen)
	}

	fn argument(&mut self) -> Result<Ast, Mismatch> {
		if self.starts_expression() {
			self.comparison()
		} else {
			Ok(Ast::Empty)
		}
	}
}

/// The formula's tree, or `None` when HyperFormula can't parse it, which
/// shows as `#ERROR!`.
pub(super) fn parse(formula: &str) -> Option<Ast> {
	let Lexed::Tokens(tokens) = tokenize(formula) else {
		return None;
	};
	let mut parser = Parser { tokens, at: 0 };
	parser
		.expect(&Token::Equals)
		.and_then(|()| parser.comparison())
		.ok()
		.filter(|_| parser.at == parser.tokens.len())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tree(formula: &str) -> Option<Ast> {
		parse(formula)
	}

	#[test]
	fn unary_minus_binds_tighter_than_power() {
		let negate_two = Ast::Negate(Box::new(Ast::Number(2.0)));
		assert_eq!(
			tree("=-2^2"),
			Some(Ast::Binary(
				BinaryOp::Power,
				Box::new(negate_two),
				Box::new(Ast::Number(2.0))
			))
		);
	}

	#[test]
	fn malformed_formulas_do_not_parse() {
		for formula in [
			"=",
			"=1+",
			"=1E3",
			"=\"a\"\"b\"",
			"=5%%",
			"=SUM (1)",
			"={}",
			"=#FOO!",
			"=1 2",
		] {
			assert_eq!(tree(formula), None, "{formula}");
		}
		assert!(tree("=  1  +  2  ").is_some());
	}

	#[test]
	fn references_parse_but_are_marked() {
		for formula in [
			"=A1",
			"=A1:B2",
			"=A:B",
			"=1:2",
			"=OFFSET(A1,0,0)",
			"=Sheet1!A1+1",
		] {
			let parsed = tree(formula).unwrap_or_else(|| panic!("{formula} parses"));
			assert!(format!("{parsed:?}").contains("Unsupported"), "{formula}");
		}
	}
}
