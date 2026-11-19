//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! Turns formula text (after its `=`) into a tree. The grammar is
//! OpenFormula's as HyperFormula's docs describe it: whitespace limited to
//! space, tab, CR and LF (`ignoreWhiteSpace: 'standard'`), `,` between
//! arguments, `{a,b;c,d}` arrays, and these operators from loosest to
//! tightest: comparisons, `&`, `+ -`, `* /`, `^`, prefix `+ -`, `%`.

use veritas_domain::CoreError;

use super::FormulaErrorKind as Kind;
use crate::report_query::unsupported;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Comparison {
	Equal,
	NotEqual,
	Less,
	LessOrEqual,
	Greater,
	GreaterOrEqual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operator {
	Add,
	Subtract,
	Multiply,
	Divide,
	Power,
	Concat,
	Compare(Comparison),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Node {
	Number(f64),
	Text(String),
	Fault(Kind),
	/// A named expression; the card defines none.
	Name(String),
	/// A cell, column, row or range reference, kept as written.
	Reference(String),
	Plus(Box<Node>),
	Minus(Box<Node>),
	Percent(Box<Node>),
	Binary(Operator, Box<Node>, Box<Node>),
	/// The name as written, and each argument, `None` where it was left
	/// empty.
	Call(String, Vec<Option<Node>>),
	Array(Vec<Vec<Node>>),
}

/// Why a formula has no tree.
pub(super) enum ParseError {
	/// HyperFormula's parse error, which the card shows as `#ERROR!`.
	Syntax,
	Refuse(CoreError),
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
	/// The value, and whether it was written as a plain integer (a row).
	Number(f64, bool),
	Text(String),
	Fault(Kind),
	Word(String),
	/// A name written straight before `(`, which this token includes.
	Function(String),
	QuotedSheet,
	Symbol(&'static str),
}

const SYMBOLS: [&str; 19] = [
	"<=", ">=", "<>", "=", "<", ">", "+", "-", "*", "/", "^", "&", "%", "(", ")", ",", ";", ":",
	"!",
];

const ERROR_LITERALS: [(&str, Kind); 9] = [
	("#DIV/0!", Kind::DivByZero),
	("#N/A", Kind::Na),
	("#NAME?", Kind::Name),
	("#VALUE!", Kind::Value),
	("#REF!", Kind::Ref),
	("#NUM!", Kind::Num),
	("#ERROR!", Kind::Error),
	("#CYCLE!", Kind::Cycle),
	("#SPILL!", Kind::Spill),
];

fn tokenize(text: &str) -> Result<Vec<Token>, ParseError> {
	let chars: Vec<char> = text.chars().collect();
	let mut tokens = Vec::new();
	let mut at = 0;
	while at < chars.len() {
		let c = chars[at];
		if matches!(c, ' ' | '\t' | '\n' | '\r') {
			at += 1;
		} else if c.is_ascii_digit()
			|| (c == '.' && chars.get(at + 1).is_some_and(char::is_ascii_digit))
		{
			let (token, length) = number(&chars[at..]);
			tokens.push(token);
			at += length;
		} else if c == '"' {
			// A backslash keeps the next character in the text, quotes
			// included, and stays in the text itself.
			let mut end = at + 1;
			loop {
				match chars.get(end) {
					None => return Err(ParseError::Syntax),
					Some('"') => break,
					Some('\\') if chars.get(end + 1).is_some() => end += 2,
					Some(_) => end += 1,
				}
			}
			tokens.push(Token::Text(chars[at + 1..end].iter().collect()));
			at = end + 1;
		} else if c == '#' {
			let mut length = 1 + chars[at + 1..]
				.iter()
				.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_'))
				.count();
			if matches!(chars.get(at + length), Some('!' | '?')) {
				length += 1;
			}
			let word: String = chars[at..at + length].iter().collect();
			let (_, kind) = ERROR_LITERALS
				.iter()
				.find(|(literal, _)| literal.eq_ignore_ascii_case(&word))
				.ok_or(ParseError::Syntax)?;
			tokens.push(Token::Fault(*kind));
			at += length;
		} else if c == '\'' {
			let close = chars[at + 1..]
				.iter()
				.position(|c| *c == '\'')
				.ok_or(ParseError::Syntax)?;
			tokens.push(Token::QuotedSheet);
			at += close + 2;
		} else if c.is_alphabetic() || c == '_' || c == '$' {
			let length = chars[at..]
				.iter()
				.take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$'))
				.count();
			let word: String = chars[at..at + length].iter().collect();
			at += length;
			if chars.get(at) == Some(&'(') {
				tokens.push(Token::Function(word));
				at += 1;
			} else {
				tokens.push(Token::Word(word));
			}
		} else if c == '{' || c == '}' {
			tokens.push(Token::Symbol(if c == '{' { "{" } else { "}" }));
			at += 1;
		} else {
			let next_two: String = chars[at..].iter().take(2).collect();
			let symbol = SYMBOLS
				.iter()
				.find(|symbol| next_two.starts_with(*symbol))
				.ok_or(ParseError::Syntax)?;
			tokens.push(Token::Symbol(symbol));
			at += symbol.len();
		}
	}
	Ok(tokens)
}

/// `123`, `1.5`, `.5`, `5.` and a lowercase `e` exponent. HyperFormula
/// reads `1E3` as 1 followed by the cell E3.
fn number(chars: &[char]) -> (Token, usize) {
	let digits = |from: usize| {
		chars[from..]
			.iter()
			.take_while(|c| c.is_ascii_digit())
			.count()
	};
	let mut length = digits(0);
	let mut integer = true;
	if chars.get(length) == Some(&'.') {
		integer = false;
		length += 1 + digits(length + 1);
	}
	let sign = usize::from(matches!(chars.get(length + 1), Some('+' | '-')));
	if chars.get(length) == Some(&'e') && digits(length + 1 + sign) > 0 {
		integer = false;
		length += 1 + sign + digits(length + 1 + sign);
	}
	let text: String = chars[..length].iter().collect();
	let text = if text.starts_with('.') {
		format!("0{text}")
	} else {
		text.replacen(".e", ".0e", 1)
	};
	let text = text.strip_suffix('.').unwrap_or(&text);
	(
		Token::Number(text.parse().unwrap_or(f64::NAN), integer),
		length,
	)
}

fn is_cell(word: &str) -> bool {
	let word = word.strip_prefix('$').unwrap_or(word);
	let letters = word.bytes().take_while(u8::is_ascii_alphabetic).count();
	let rest = &word[letters..];
	let rest = rest.strip_prefix('$').unwrap_or(rest);
	letters > 0 && !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

fn is_column(word: &str) -> bool {
	let word = word.strip_prefix('$').unwrap_or(word);
	!word.is_empty() && word.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_row(word: &str) -> bool {
	word.strip_prefix('$')
		.is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// `R1C1`-style names, which HyperFormula's parser rejects.
fn is_r1c1(word: &str) -> bool {
	let Some(rest) = word.strip_prefix(['R', 'r']) else {
		return false;
	};
	let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
	rest.strip_prefix(['C', 'c'])
		.is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Past this depth Veritas refuses rather than risk its stack.
const MAX_DEPTH: usize = 200;

struct Parser {
	tokens: Vec<Token>,
	at: usize,
	depth: usize,
}

pub(super) fn parse(text: &str) -> Result<Node, ParseError> {
	let mut parser = Parser {
		tokens: tokenize(text)?,
		at: 0,
		depth: 0,
	};
	let node = parser.expression()?;
	if parser.at == parser.tokens.len() {
		Ok(node)
	} else {
		Err(ParseError::Syntax)
	}
}

impl Parser {
	fn peek(&self) -> Option<&Token> {
		self.tokens.get(self.at)
	}

	fn next(&mut self) -> Option<Token> {
		let token = self.tokens.get(self.at).cloned();
		self.at += 1;
		token
	}

	fn eat(&mut self, symbol: &str) -> bool {
		let found = matches!(self.peek(), Some(Token::Symbol(found)) if *found == symbol);
		if found {
			self.at += 1;
		}
		found
	}

	fn expect(&mut self, symbol: &str) -> Result<(), ParseError> {
		if self.eat(symbol) {
			Ok(())
		} else {
			Err(ParseError::Syntax)
		}
	}

	fn expression(&mut self) -> Result<Node, ParseError> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return Err(ParseError::Refuse(unsupported(
				"a formula nested this deeply".to_owned(),
			)));
		}
		let node = self.comparison();
		self.depth -= 1;
		node
	}

	fn comparison(&mut self) -> Result<Node, ParseError> {
		let mut node = self.concatenation()?;
		loop {
			let comparison = match self.peek() {
				Some(Token::Symbol("=")) => Comparison::Equal,
				Some(Token::Symbol("<>")) => Comparison::NotEqual,
				Some(Token::Symbol("<")) => Comparison::Less,
				Some(Token::Symbol("<=")) => Comparison::LessOrEqual,
				Some(Token::Symbol(">")) => Comparison::Greater,
				Some(Token::Symbol(">=")) => Comparison::GreaterOrEqual,
				_ => return Ok(node),
			};
			self.at += 1;
			let right = self.concatenation()?;
			node = Node::Binary(
				Operator::Compare(comparison),
				Box::new(node),
				Box::new(right),
			);
		}
	}

	fn concatenation(&mut self) -> Result<Node, ParseError> {
		let mut node = self.additive()?;
		while self.eat("&") {
			let right = self.additive()?;
			node = Node::Binary(Operator::Concat, Box::new(node), Box::new(right));
		}
		Ok(node)
	}

	fn additive(&mut self) -> Result<Node, ParseError> {
		let mut node = self.multiplicative()?;
		loop {
			let operator = if self.eat("+") {
				Operator::Add
			} else if self.eat("-") {
				Operator::Subtract
			} else {
				return Ok(node);
			};
			let right = self.multiplicative()?;
			node = Node::Binary(operator, Box::new(node), Box::new(right));
		}
	}

	fn multiplicative(&mut self) -> Result<Node, ParseError> {
		let mut node = self.power()?;
		loop {
			let operator = if self.eat("*") {
				Operator::Multiply
			} else if self.eat("/") {
				Operator::Divide
			} else {
				return Ok(node);
			};
			let right = self.power()?;
			node = Node::Binary(operator, Box::new(node), Box::new(right));
		}
	}

	/// `^` groups left to right, and prefix minus binds tighter, so
	/// `-2^2` is 4 and `2^3^2` is 64.
	fn power(&mut self) -> Result<Node, ParseError> {
		let mut node = self.prefix()?;
		while self.eat("^") {
			let right = self.prefix()?;
			node = Node::Binary(Operator::Power, Box::new(node), Box::new(right));
		}
		Ok(node)
	}

	fn prefix(&mut self) -> Result<Node, ParseError> {
		if self.eat("+") {
			Ok(Node::Plus(Box::new(self.nested_prefix()?)))
		} else if self.eat("-") {
			Ok(Node::Minus(Box::new(self.nested_prefix()?)))
		} else {
			let node = self.primary()?;
			if self.eat("%") {
				Ok(Node::Percent(Box::new(node)))
			} else {
				Ok(node)
			}
		}
	}

	fn nested_prefix(&mut self) -> Result<Node, ParseError> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return Err(ParseError::Refuse(unsupported(
				"a formula nested this deeply".to_owned(),
			)));
		}
		let node = self.prefix();
		self.depth -= 1;
		node
	}

	fn primary(&mut self) -> Result<Node, ParseError> {
		match self.next() {
			Some(Token::Number(value, integer)) => {
				if integer && self.peek() == Some(&Token::Symbol(":")) {
					let text = js_integer_text(value);
					return self.range(text);
				}
				Ok(Node::Number(value))
			}
			Some(Token::Text(text)) => Ok(Node::Text(text)),
			Some(Token::Fault(kind)) => Ok(Node::Fault(kind)),
			Some(Token::Symbol("(")) => {
				let node = self.expression()?;
				self.expect(")")?;
				Ok(node)
			}
			Some(Token::Symbol("{")) => self.array(),
			Some(Token::Function(name)) => self.call(name),
			Some(Token::QuotedSheet) => {
				self.expect("!")?;
				self.sheet_reference("a sheet".to_owned())
			}
			Some(Token::Word(word)) => self.word(word),
			_ => Err(ParseError::Syntax),
		}
	}

	fn word(&mut self, word: String) -> Result<Node, ParseError> {
		if self.eat("!") {
			return self.sheet_reference(word);
		}
		let reference_like = is_cell(&word) || is_column(&word) || is_row(&word);
		if reference_like && self.peek() == Some(&Token::Symbol(":")) {
			return self.range(word);
		}
		if is_cell(&word) {
			Ok(Node::Reference(word))
		} else if is_r1c1(&word) || word.contains('$') {
			Err(ParseError::Syntax)
		} else {
			Ok(Node::Name(word))
		}
	}

	fn sheet_reference(&mut self, sheet: String) -> Result<Node, ParseError> {
		match self.next() {
			Some(Token::Word(word)) if is_cell(&word) || is_column(&word) || is_row(&word) => {
				let text = format!("{sheet}!{word}");
				if self.peek() == Some(&Token::Symbol(":")) {
					self.range(text)
				} else {
					Ok(Node::Reference(text))
				}
			}
			Some(Token::Number(value, true)) => {
				self.range(format!("{sheet}!{}", js_integer_text(value)))
			}
			_ => Err(ParseError::Syntax),
		}
	}

	fn range(&mut self, start: String) -> Result<Node, ParseError> {
		self.expect(":")?;
		let end = match self.next() {
			Some(Token::Word(word)) if is_cell(&word) || is_column(&word) || is_row(&word) => word,
			Some(Token::Number(value, true)) => js_integer_text(value),
			_ => return Err(ParseError::Syntax),
		};
		Ok(Node::Reference(format!("{start}:{end}")))
	}

	fn call(&mut self, name: String) -> Result<Node, ParseError> {
		if name.contains('$') {
			return Err(ParseError::Syntax);
		}
		let mut arguments = Vec::new();
		if self.eat(")") {
			return Ok(Node::Call(name, arguments));
		}
		loop {
			let empty = matches!(self.peek(), Some(Token::Symbol("," | ")")));
			arguments.push(if empty {
				None
			} else {
				Some(self.expression()?)
			});
			if self.eat(")") {
				return Ok(Node::Call(name, arguments));
			}
			self.expect(",")?;
		}
	}

	fn array(&mut self) -> Result<Node, ParseError> {
		let mut rows = vec![vec![self.expression()?]];
		loop {
			if self.eat("}") {
				break;
			}
			if self.eat(";") {
				rows.push(Vec::new());
			} else {
				self.expect(",")?;
			}
			let element = self.expression()?;
			if let Some(row) = rows.last_mut() {
				row.push(element);
			}
		}
		let width = rows.first().map_or(0, Vec::len);
		if rows.iter().all(|row| row.len() == width) {
			Ok(Node::Array(rows))
		} else {
			Err(ParseError::Syntax)
		}
	}
}

fn js_integer_text(value: f64) -> String {
	super::value::js_number_text(value)
}
