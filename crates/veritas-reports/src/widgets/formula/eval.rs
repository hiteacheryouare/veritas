//! HyperFormula 3.5.0's interpreter for the functions the formula card
//! supports: `Interpreter.mjs`, `ArithmeticHelper.mjs`, `FunctionPlugin.mjs`'s
//! `runFunction`, the Boolean, NumericAggregation, Rounding, Abs, Text and
//! DateTime plugins, and Actual's `CustomFunctionsPlugin`
//! (`loot-core/src/shared/formulas/customFunctions.ts`). Every function
//! evaluates all of its arguments before it runs, IF included.

use std::cmp::Ordering;
use std::collections::HashMap;

use veritas_domain::CoreError;

use super::ErrorKind;
use super::dates::{self, SimpleDate};
use super::js;
use super::syntax::{Ast, BinaryOp};
use crate::report_query::unsupported;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
	Number(f64),
	Text(String),
	Boolean(bool),
	Empty,
	Error(ErrorKind),
}

/// One `BUDGET_QUERY` call, as `createBudgetQueryPrefetchKey` tells calls
/// apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct BudgetRequest {
	pub dimension: String,
	pub categories: Vec<String>,
	pub start: String,
	pub end: String,
}

/// `FormulaQueryContext`: what each evaluation asked for, and what was
/// fetched for it. JavaScript's `Set` and `Map` keep insertion order, so the
/// names are lists.
#[derive(Default)]
pub(super) struct Session {
	pub query_names: Vec<String>,
	pub category_names: Vec<String>,
	pub start_names: Vec<String>,
	pub end_names: Vec<String>,
	pub budget_requests: Vec<BudgetRequest>,
	pub query_sums: HashMap<String, f64>,
	pub categories: HashMap<String, Vec<String>>,
	pub starts: HashMap<String, String>,
	pub ends: HashMap<String, String>,
	pub budget_values: HashMap<BudgetRequest, f64>,
	pub budget_errors: HashMap<BudgetRequest, String>,
}

fn remember<T: PartialEq>(list: &mut Vec<T>, item: T) {
	if !list.contains(&item) {
		list.push(item);
	}
}

#[derive(Clone, Copy)]
enum Kind {
	Number { min: Option<f64> },
	Text,
	Boolean,
	Scalar,
}

#[derive(Clone)]
struct Param {
	kind: Kind,
	default: Option<Value>,
}

const fn param(kind: Kind) -> Param {
	Param {
		kind,
		default: None,
	}
}

const NUMBER: Param = param(Kind::Number { min: None });
const NON_NEGATIVE: Param = param(Kind::Number { min: Some(0.0) });
const TEXT: Param = param(Kind::Text);
const BOOLEAN: Param = param(Kind::Boolean);
const SCALAR: Param = param(Kind::Scalar);

fn with_default(mut param: Param, value: Value) -> Param {
	param.default = Some(value);
	param
}

/// `ArithmeticHelper.coerceScalarToNumberOrError`.
fn to_number(value: Value) -> Result<f64, ErrorKind> {
	match value {
		Value::Number(number) => Ok(number),
		Value::Empty => Ok(0.0),
		Value::Boolean(flag) => Ok(f64::from(u8::from(flag))),
		Value::Text(text) => text_to_number(&text).ok_or(ErrorKind::Value),
		Value::Error(kind) => Err(kind),
	}
}

/// `coerceToMaybeNumber` for text: empty, a percent, a currency amount, a
/// plain number, then a date or time.
fn text_to_number(text: &str) -> Option<f64> {
	if text.is_empty() {
		return Some(0.0);
	}
	let trimmed = js::trim(text);
	if let Some(percent) = trimmed.strip_suffix('%')
		&& let Some(number) = js::numeric_text(js::trim(percent))
	{
		return Some(number / 100.0);
	}
	let currency = trimmed
		.strip_prefix('$')
		.or_else(|| trimmed.strip_suffix('$'))
		.and_then(|amount| js::numeric_text(js::trim(amount)));
	if currency.is_some() {
		return currency;
	}
	js::numeric_text(trimmed).or_else(|| dates::text_to_serial(text))
}

/// `coerceScalarToString`.
fn to_text(value: Value) -> Result<String, ErrorKind> {
	match value {
		Value::Text(text) => Ok(text),
		Value::Empty => Ok(String::new()),
		Value::Number(number) => Ok(js::number_text(number)),
		Value::Boolean(flag) => Ok(if flag { "TRUE" } else { "FALSE" }.to_owned()),
		Value::Error(kind) => Err(kind),
	}
}

/// `coerceScalarToBoolean`; `Ok(None)` is text that isn't TRUE or FALSE.
fn to_boolean(value: Value) -> Result<Option<bool>, ErrorKind> {
	match value {
		Value::Boolean(flag) => Ok(Some(flag)),
		Value::Empty => Ok(Some(false)),
		Value::Number(number) => Ok(Some(number != 0.0)),
		Value::Text(text) => Ok(match text.to_uppercase().as_str() {
			"TRUE" => Some(true),
			"FALSE" | "" => Some(false),
			_ => None,
		}),
		Value::Error(kind) => Err(kind),
	}
}

const EPSILON: f64 = 1e-13;

fn add(left: f64, right: f64) -> f64 {
	let sum = left + right;
	if sum.abs() < EPSILON * left.abs() {
		0.0
	} else {
		sum
	}
}

fn subtract(left: f64, right: f64) -> f64 {
	let difference = left - right;
	if difference.abs() < EPSILON * left.abs() {
		0.0
	} else {
		difference
	}
}

/// `floatCmp`: numbers within a relative 1e-13 are equal.
fn float_compare(left: f64, right: f64) -> Ordering {
	let scale = 1.0 + EPSILON;
	if (right >= 0.0 && left * scale >= right && left <= right * scale)
		|| (right <= 0.0 && left * scale <= right && left >= right * scale)
	{
		Ordering::Equal
	} else if left > right {
		Ordering::Greater
	} else {
		Ordering::Less
	}
}

fn type_rank(value: &Value) -> u8 {
	match value {
		Value::Empty => 0,
		Value::Number(_) => 1,
		Value::Text(_) => 2,
		Value::Boolean(_) => 3,
		Value::Error(_) => 4,
	}
}

/// `ArithmeticHelper.compare`: text that reads as a date compares as one,
/// an empty side takes the other side's type, and mixed types order as
/// number < text < boolean.
fn compare(left: Value, right: Value) -> Result<Ordering, CoreError> {
	if matches!(left, Value::Text(_)) || matches!(right, Value::Text(_)) {
		let as_date = |value: &Value| match value {
			Value::Text(text) => dates::text_to_serial(text),
			Value::Number(number) => Some(*number),
			_ => None,
		};
		if let (Some(left), Some(right)) = (as_date(&left), as_date(&right)) {
			return Ok(float_compare(left, right));
		}
	}
	let empty_as = |other: &Value| match other {
		Value::Text(_) => Value::Text(String::new()),
		Value::Number(_) => Value::Number(0.0),
		Value::Boolean(_) => Value::Boolean(false),
		_ => Value::Empty,
	};
	let (left, right) = match (&left, &right) {
		(Value::Empty, _) => (empty_as(&right), right),
		(_, Value::Empty) => {
			let right = empty_as(&left);
			(left, right)
		}
		_ => (left, right),
	};
	Ok(match (&left, &right) {
		(Value::Text(left), Value::Text(right)) => js::collate(left, right)?,
		(Value::Boolean(left), Value::Boolean(right)) => left.cmp(right),
		(Value::Number(left), Value::Number(right)) => float_compare(*left, *right),
		_ => type_rank(&left).cmp(&type_rank(&right)),
	})
}

fn utf16_text(units: &[u16]) -> Result<String, CoreError> {
	String::from_utf16(units)
		.map_err(|_| unsupported("text cut inside a character JavaScript stores as two".to_owned()))
}

/// `ToIntegerOrInfinity`, clamped to `0..=length`, as `String.slice` reads a
/// count.
fn clamp_count(count: f64, length: usize) -> usize {
	let count = count.trunc();
	if count <= 0.0 {
		0
	} else if count >= length as f64 {
		length
	} else {
		count as usize
	}
}

/// A function's arguments after `runFunction`'s checks and coercions, or
/// the error it returns instead.
type Coerced = Result<Vec<Option<Value>>, ErrorKind>;

pub(super) struct Evaluator<'a> {
	pub session: &'a mut Session,
	pub today: SimpleDate,
}

impl Evaluator<'_> {
	/// `Interpreter.evaluateAst`: an infinite or NaN result is `#NUM!`, and
	/// negative zero becomes zero.
	pub fn eval(&mut self, ast: &Ast) -> Result<Value, CoreError> {
		let value = self.eval_raw(ast)?;
		Ok(match value {
			Value::Number(number) if !number.is_finite() => Value::Error(ErrorKind::Num),
			Value::Number(number) => Value::Number(if number == 0.0 { 0.0 } else { number }),
			other => other,
		})
	}

	fn eval_raw(&mut self, ast: &Ast) -> Result<Value, CoreError> {
		Ok(match ast {
			Ast::Empty => Value::Empty,
			Ast::Number(number) => Value::Number(*number),
			Ast::Text(text) => Value::Text(text.clone()),
			Ast::Error(kind) => Value::Error(*kind),
			Ast::Name(_) => Value::Error(ErrorKind::Name),
			Ast::Parenthesis(inner) | Ast::Plus(inner) => self.eval(inner)?,
			Ast::Negate(inner) => match to_number(self.eval(inner)?) {
				Ok(number) => Value::Number(-number),
				Err(kind) => Value::Error(kind),
			},
			Ast::Percent(inner) => match to_number(self.eval(inner)?) {
				Ok(number) => Value::Number(number / 100.0),
				Err(kind) => Value::Error(kind),
			},
			Ast::Binary(op, left, right) => {
				let left = self.eval(left)?;
				let right = self.eval(right)?;
				binary(*op, left, right)?
			}
			Ast::Call(name, args) => self.call(name, args)?,
			Ast::Unsupported(what) => {
				return Err(unsupported(format!("{what} in a formula card")));
			}
			Ast::Array(_) => {
				return Err(unsupported(
					"an array outside BUDGET_QUERY's categories".to_owned(),
				));
			}
		})
	}

	/// `runFunction`: evaluates every argument, checks the count, then
	/// coerces each in order and stops at the first that fails.
	fn run(&mut self, args: &[Ast], params: &[Param], repeat: bool) -> Result<Coerced, CoreError> {
		let mut values = Vec::with_capacity(args.len());
		for arg in args {
			values.push(self.eval(arg)?);
		}
		Ok(coerce(values, params, repeat))
	}

	fn call(&mut self, name: &str, args: &[Ast]) -> Result<Value, CoreError> {
		let result = match name {
			"SUM" | "AVERAGE" | "MIN" | "MAX" => return self.aggregate(name, args),
			"BUDGET_QUERY" => return self.budget_query(args),
			"QUERY_EXTRACT_CATEGORIES" => {
				return Err(unsupported(
					"QUERY_EXTRACT_CATEGORIES anywhere but BUDGET_QUERY's categories".to_owned(),
				));
			}
			"TRUE" => self.run(args, &[], false)?.map(|_| Value::Boolean(true)),
			"FALSE" => self.run(args, &[], false)?.map(|_| Value::Boolean(false)),
			"IF" => self
				.run(
					args,
					&[BOOLEAN, SCALAR, with_default(SCALAR, Value::Boolean(false))],
					false,
				)?
				.map(|mut args| {
					let otherwise = args.pop().flatten().unwrap_or(Value::Empty);
					let then = args.pop().flatten().unwrap_or(Value::Empty);
					if args.pop().flatten() == Some(Value::Boolean(true)) {
						then
					} else {
						otherwise
					}
				}),
			"AND" | "OR" => self.run(args, &[BOOLEAN], true)?.map(|args| {
				let flags = args
					.into_iter()
					.flatten()
					.map(|flag| flag == Value::Boolean(true));
				Value::Boolean(if name == "AND" {
					flags.into_iter().all(|flag| flag)
				} else {
					flags.into_iter().any(|flag| flag)
				})
			}),
			"NOT" => self.run(args, &[BOOLEAN], false)?.map(|args| {
				Value::Boolean(args.first().cloned().flatten() != Some(Value::Boolean(true)))
			}),
			"ROUND" => self
				.run(
					args,
					&[NUMBER, with_default(NUMBER, Value::Number(0.0))],
					false,
				)?
				.map(|args| {
					let [number, places] = numbers(&args);
					let scale = js::pow(10.0, places);
					Value::Number(if number < 0.0 {
						-js::round(-number * scale) / scale
					} else {
						js::round(number * scale) / scale
					})
				}),
			"ABS" => self
				.run(args, &[NUMBER], false)?
				.map(|args| Value::Number(numbers::<1>(&args)[0].abs())),
			"CONCATENATE" => self
				.run(args, &[TEXT], true)?
				.map(|args| Value::Text(texts(&args).concat())),
			"LEN" => self
				.run(args, &[TEXT], false)?
				.map(|args| Value::Number(texts(&args)[0].encode_utf16().count() as f64)),
			"UPPER" => self
				.run(args, &[TEXT], false)?
				.map(|args| Value::Text(texts(&args)[0].to_uppercase())),
			"LOWER" => self
				.run(args, &[TEXT], false)?
				.map(|args| Value::Text(texts(&args)[0].to_lowercase())),
			"LEFT" | "RIGHT" => {
				let coerced = self.run(
					args,
					&[TEXT, with_default(NUMBER, Value::Number(1.0))],
					false,
				)?;
				match coerced {
					Ok(args) => slice(name == "LEFT", &texts(&args)[0], numbers::<2>(&args)[1])?,
					Err(kind) => Err(kind),
				}
			}
			"TODAY" => self
				.run(args, &[], false)?
				.map(|_| Value::Number(dates::to_serial(self.today))),
			"DATE" => self
				.run(args, &[NUMBER, NUMBER, NUMBER], false)?
				.map(|args| date(numbers(&args))),
			"YEAR" | "MONTH" | "DAY" => self.run(args, &[NON_NEGATIVE], false)?.map(|args| {
				let date = dates::from_serial(numbers::<1>(&args)[0]);
				Value::Number(match name {
					"YEAR" => date.year,
					"MONTH" => date.month,
					_ => date.day,
				})
			}),
			"EOMONTH" => self
				.run(args, &[NON_NEGATIVE, NUMBER], false)?
				.map(|args| end_of_month(numbers(&args))),
			"QUERY" => self.run(args, &[TEXT], false)?.map(|args| {
				let name = texts(&args)[0].clone();
				let sum = self.session.query_sums.get(&name).copied().unwrap_or(0.0);
				remember(&mut self.session.query_names, name);
				Value::Number(sum)
			}),
			"QUERY_EXTRACT_TIMEFRAME_START" => self.run(args, &[TEXT], false)?.map(|args| {
				let name = texts(&args)[0].clone();
				let found = self.session.starts.get(&name).cloned().unwrap_or_default();
				remember(&mut self.session.start_names, name);
				Value::Text(found)
			}),
			"QUERY_EXTRACT_TIMEFRAME_END" => self.run(args, &[TEXT], false)?.map(|args| {
				let name = texts(&args)[0].clone();
				let found = self.session.ends.get(&name).cloned().unwrap_or_default();
				remember(&mut self.session.end_names, name);
				Value::Text(found)
			}),
			_ => Ok(Value::Error(ErrorKind::Name)),
		};
		Ok(result.unwrap_or_else(Value::Error))
	}

	/// SUM, AVERAGE, MIN and MAX (`NumericAggregationPlugin.reduce`): each
	/// argument is read only while no earlier one was an error.
	fn aggregate(&mut self, name: &str, args: &[Ast]) -> Result<Value, CoreError> {
		if args.is_empty() {
			return Ok(Value::Error(ErrorKind::Na));
		}
		let (mut total, mut count) = match name {
			"MIN" => (f64::INFINITY, 0.0),
			"MAX" => (f64::NEG_INFINITY, 0.0),
			_ => (0.0, 0.0),
		};
		for arg in args {
			let number = match to_number(self.eval(arg)?) {
				Ok(number) => number,
				Err(kind) => return Ok(Value::Error(kind)),
			};
			total = match name {
				"SUM" => add(total, number),
				"AVERAGE" => total + number,
				"MIN" => total.min(number),
				_ => total.max(number),
			};
			count += 1.0;
		}
		Ok(Value::Number(match name {
			"AVERAGE" => total / count,
			"MIN" | "MAX" if !total.is_finite() => 0.0,
			_ => total,
		}))
	}

	/// `CustomFunctionsPlugin.budgetQuery`. The categories argument is a
	/// range: `QUERY_EXTRACT_CATEGORIES`, an array of literals, or one value.
	fn budget_query(&mut self, args: &[Ast]) -> Result<Value, CoreError> {
		let mut texts = Vec::with_capacity(args.len());
		let mut categories = Err(ErrorKind::Na);
		for (index, arg) in args.iter().enumerate() {
			if index == 1 {
				categories = self.categories_argument(arg)?;
			} else {
				texts.push(self.eval(arg)?);
			}
		}
		if args.len() != 4 {
			return Ok(Value::Error(ErrorKind::Na));
		}
		let [dimension, start, end] =
			<[Value; 3]>::try_from(texts).unwrap_or([Value::Empty, Value::Empty, Value::Empty]);
		// Coerced in argument order, so the first failing one is the error.
		let request = to_text(dimension).and_then(|dimension| {
			Ok(BudgetRequest {
				dimension: dimension.to_lowercase(),
				categories: categories?,
				start: to_text(start)?,
				end: to_text(end)?,
			})
		});
		let request = match request {
			Ok(request) => request,
			Err(kind) => return Ok(Value::Error(kind)),
		};
		let failed = self.session.budget_errors.contains_key(&request);
		let found = self.session.budget_values.get(&request).copied();
		remember(&mut self.session.budget_requests, request);
		Ok(if failed {
			Value::Error(ErrorKind::Value)
		} else {
			Value::Number(found.unwrap_or(0.0))
		})
	}

	/// The category ids `categoryRangeToIds` reads from BUDGET_QUERY's
	/// second argument, or the error that argument evaluated to.
	fn categories_argument(
		&mut self,
		ast: &Ast,
	) -> Result<Result<Vec<String>, ErrorKind>, CoreError> {
		let mut inner = ast;
		while let Ast::Parenthesis(next) = inner {
			inner = next;
		}
		let literal_number = |ast: &Ast| match ast {
			Ast::Number(number) => Some(*number),
			Ast::Negate(inner) => match **inner {
				Ast::Number(number) if number != 0.0 => Some(-number),
				_ => None,
			},
			_ => None,
		};
		let cells: Vec<Value> = match inner {
			Ast::Call(name, args) if name == "QUERY_EXTRACT_CATEGORIES" => {
				match self.run(args, &[TEXT], false)? {
					Ok(args) => {
						let name = texts(&args)[0].clone();
						let ids = self
							.session
							.categories
							.get(&name)
							.cloned()
							.unwrap_or_default();
						remember(&mut self.session.category_names, name);
						ids.into_iter().map(Value::Text).collect()
					}
					Err(kind) => return Ok(Err(kind)),
				}
			}
			Ast::Array(rows) => {
				let width = rows.first().map_or(0, Vec::len);
				if rows.iter().any(|row| row.len() != width) {
					return Ok(Err(ErrorKind::Ref));
				}
				let mut cells = Vec::new();
				for cell in rows.iter().flatten() {
					cells.push(match cell {
						Ast::Text(text) => Value::Text(text.clone()),
						_ => match literal_number(cell) {
							Some(number) => Value::Number(number),
							None => {
								return Err(unsupported(
									"an array of anything but text and numbers in BUDGET_QUERY"
										.to_owned(),
								));
							}
						},
					});
				}
				cells
			}
			_ => match self.eval(ast)? {
				Value::Error(kind) => return Ok(Err(kind)),
				Value::Number(_) if literal_number(inner).is_none() => {
					return Err(unsupported(
						"a computed number as BUDGET_QUERY's categories".to_owned(),
					));
				}
				value => vec![value],
			},
		};
		let mut ids = Vec::new();
		for cell in cells {
			let text = match cell {
				Value::Text(text) => text,
				Value::Number(number) => js::number_text(number),
				_ => continue,
			};
			ids.extend(
				text.split(',')
					.map(js::trim)
					.filter(|id| !id.is_empty())
					.map(str::to_owned),
			);
		}
		Ok(Ok(ids))
	}
}

fn coerce(values: Vec<Value>, params: &[Param], repeat: bool) -> Coerced {
	let mut params = params.to_vec();
	if repeat && let Some(last) = params.last().cloned() {
		while params.len() < values.len() {
			params.push(last.clone());
		}
	}
	if values.len() > params.len() || params[values.len()..].iter().any(|p| p.default.is_none()) {
		return Err(ErrorKind::Na);
	}
	let mut values = values.into_iter();
	let mut coerced = Vec::with_capacity(params.len());
	for param in params {
		let Some(value) = values.next().or_else(|| param.default.clone()) else {
			coerced.push(None);
			continue;
		};
		coerced.push(Some(match param.kind {
			Kind::Number { min } => {
				let number = to_number(value)?;
				if min.is_some_and(|min| number < min) {
					return Err(ErrorKind::Num);
				}
				Value::Number(number)
			}
			Kind::Text => Value::Text(to_text(value)?),
			Kind::Boolean => Value::Boolean(to_boolean(value)?.ok_or(ErrorKind::Value)?),
			Kind::Scalar => value,
		}));
	}
	Ok(coerced)
}

fn numbers<const N: usize>(args: &[Option<Value>]) -> [f64; N] {
	let mut out = [0.0; N];
	for (slot, arg) in out.iter_mut().zip(args) {
		if let Some(Value::Number(number)) = arg {
			*slot = *number;
		}
	}
	out
}

fn texts(args: &[Option<Value>]) -> Vec<String> {
	args.iter()
		.map(|arg| match arg {
			Some(Value::Text(text)) => text.clone(),
			_ => String::new(),
		})
		.collect()
}

/// LEFT and RIGHT, counting UTF-16 units as `String.slice` does.
fn slice(left: bool, text: &str, count: f64) -> Result<Result<Value, ErrorKind>, CoreError> {
	if count < 0.0 {
		return Ok(Err(ErrorKind::Value));
	}
	let units: Vec<u16> = text.encode_utf16().collect();
	let kept = if left {
		&units[..clamp_count(count, units.len())]
	} else if count == 0.0 {
		&[]
	} else if count.trunc() == 0.0 {
		// `slice(-0.5)` starts at -0, which is the start of the text.
		&units[..]
	} else {
		&units[units.len() - clamp_count(count, units.len())..]
	};
	Ok(Ok(Value::Text(utf16_text(kept)?)))
}

/// DATE: years before 1900 count from 1900, and months past 12 roll into
/// the next year.
fn date([year, month, day]: [f64; 3]) -> Value {
	let (day, mut month, mut year) = (day.trunc(), month.trunc(), year.trunc());
	if year < 1900.0 {
		year += 1900.0;
	}
	let delta = ((month - 1.0) / 12.0).floor();
	year += delta;
	month -= delta * 12.0;
	let first = SimpleDate {
		year,
		month,
		day: 1.0,
	};
	if !dates::is_valid(first) {
		return Value::Error(ErrorKind::Value);
	}
	dates::within_bounds(dates::to_serial(first) + (day - 1.0))
		.map_or(Value::Error(ErrorKind::Num), Value::Number)
}

/// EOMONTH, through `offsetMonth` and `endOfMonth`.
fn end_of_month([serial, months]: [f64; 2]) -> Value {
	let date = dates::from_serial(serial);
	let total = 12.0 * date.year + date.month - 1.0 + months;
	let year = (total / 12.0).floor();
	let month = total % 12.0 + 1.0;
	let end = SimpleDate {
		year,
		month,
		day: dates::days_in_month(year, month),
	};
	dates::within_bounds(dates::to_serial(end)).map_or(Value::Error(ErrorKind::Num), Value::Number)
}

/// `binaryRangeWrapper` and the operator: an error on the left wins, then
/// one on the right, then a failed coercion.
fn binary(op: BinaryOp, left: Value, right: Value) -> Result<Value, CoreError> {
	if let Value::Error(kind) = left {
		return Ok(Value::Error(kind));
	}
	if let Value::Error(kind) = right {
		return Ok(Value::Error(kind));
	}
	let arithmetic =
		|apply: fn(f64, f64) -> Value| match (to_number(left.clone()), to_number(right.clone())) {
			(Err(kind), _) | (_, Err(kind)) => Value::Error(kind),
			(Ok(left), Ok(right)) => apply(left, right),
		};
	let comparison = |test: fn(Ordering) -> bool| -> Result<Value, CoreError> {
		Ok(Value::Boolean(test(compare(left.clone(), right.clone())?)))
	};
	Ok(match op {
		BinaryOp::Add => arithmetic(|a, b| Value::Number(add(a, b))),
		BinaryOp::Subtract => arithmetic(|a, b| Value::Number(subtract(a, b))),
		BinaryOp::Multiply => arithmetic(|a, b| Value::Number(a * b)),
		BinaryOp::Divide => arithmetic(|a, b| {
			if b == 0.0 {
				Value::Error(ErrorKind::DivByZero)
			} else {
				Value::Number(a / b)
			}
		}),
		BinaryOp::Power => arithmetic(|a, b| Value::Number(js::pow(a, b))),
		BinaryOp::Concatenate => match (to_text(left.clone()), to_text(right.clone())) {
			(Err(kind), _) | (_, Err(kind)) => Value::Error(kind),
			(Ok(left), Ok(right)) => Value::Text(left + &right),
		},
		BinaryOp::Equal => comparison(Ordering::is_eq)?,
		BinaryOp::NotEqual => comparison(Ordering::is_ne)?,
		BinaryOp::Greater => comparison(Ordering::is_gt)?,
		BinaryOp::Less => comparison(Ordering::is_lt)?,
		BinaryOp::GreaterOrEqual => comparison(Ordering::is_ge)?,
		BinaryOp::LessOrEqual => comparison(Ordering::is_le)?,
	})
}
