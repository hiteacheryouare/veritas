//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! D75's functions, and Actual's own QUERY, BUDGET_QUERY and
//! QUERY_EXTRACT_* (`shared/formulas/customFunctions.ts`). Any other name
//! is `#NAME?`, as an unknown function is in Actual.

use super::FormulaErrorKind as Kind;
use super::datetime::{self, MAX_SERIAL};
use super::engine::BudgetRequest;
use super::evaluator::{Evaluator, is_category_list};
use super::parser::Node;
use super::value::{self, Outcome, Stop, Value, finite};
use crate::filter::is_js_space;
use crate::report_query::unsupported;

/// Each function's fewest and most arguments; a call outside them is
/// `#N/A` before any argument is evaluated.
fn arity(name: &str) -> Option<(usize, usize)> {
	const ANY: usize = usize::MAX;
	Some(match name {
		"TRUE" | "FALSE" | "TODAY" => (0, 0),
		"NOT"
		| "ABS"
		| "LEN"
		| "UPPER"
		| "LOWER"
		| "YEAR"
		| "MONTH"
		| "DAY"
		| "QUERY"
		| "QUERY_EXTRACT_CATEGORIES"
		| "QUERY_EXTRACT_TIMEFRAME_START"
		| "QUERY_EXTRACT_TIMEFRAME_END" => (1, 1),
		"ROUND" | "LEFT" | "RIGHT" => (1, 2),
		"EOMONTH" => (2, 2),
		"IF" => (2, 3),
		"DATE" => (3, 3),
		"BUDGET_QUERY" => (4, 4),
		"AND" | "OR" | "SUM" | "AVERAGE" | "MIN" | "MAX" | "CONCATENATE" => (1, ANY),
		_ => return None,
	})
}

/// Serials past this are where HyperFormula's own date arithmetic stops
/// matching the calendar.
const LARGEST_SERIAL_READ: f64 = 1e15;

fn push_new<T: PartialEq>(list: &mut Vec<T>, item: T) {
	if !list.contains(&item) {
		list.push(item);
	}
}

/// A call's arguments, every one already evaluated. Most functions look at
/// them in order, so the first error, or the first value that won't
/// convert, is the result.
struct Arguments {
	values: Vec<Outcome<Value>>,
	/// BUDGET_QUERY's categories, which stand in for its second value.
	categories: Option<Outcome<Vec<String>>>,
}

impl Arguments {
	fn len(&self) -> usize {
		self.values.len()
	}

	fn take(&mut self, index: usize) -> Outcome<Value> {
		match self.values.get_mut(index) {
			Some(slot) => std::mem::replace(slot, Ok(Value::Empty)),
			None => Ok(Value::Empty),
		}
	}

	fn number(&mut self, index: usize) -> Outcome<f64> {
		value::to_number(&self.take(index)?)
	}

	fn text(&mut self, index: usize) -> Outcome<String> {
		Ok(value::to_text(&self.take(index)?))
	}

	fn boolean(&mut self, index: usize) -> Outcome<bool> {
		value::to_bool(&self.take(index)?)
	}

	fn number_or(&mut self, index: usize, default: f64) -> Outcome<f64> {
		if index < self.len() {
			self.number(index)
		} else {
			Ok(default)
		}
	}
}

impl Evaluator<'_> {
	/// HyperFormula evaluates every argument of a call before it looks at
	/// any, even when the count is wrong or IF won't use a branch, so a
	/// QUERY anywhere in the formula is fetched. SUM, AVERAGE, MIN and MAX
	/// stop at the first error instead, and an unknown function evaluates
	/// nothing.
	pub(super) fn call(&mut self, name: &str, arguments: &[Option<Node>]) -> Outcome<Value> {
		let name = name.to_uppercase();
		let (fewest, most) = arity(&name).ok_or(Kind::Name)?;
		let counted = |count: usize| -> Outcome<()> {
			if (fewest..=most).contains(&count) {
				Ok(())
			} else {
				Err(Kind::Na.into())
			}
		};
		if matches!(name.as_str(), "SUM" | "AVERAGE" | "MIN" | "MAX") {
			counted(arguments.len())?;
			return self.aggregate(&name, arguments);
		}
		let mut evaluated = Arguments {
			values: Vec::with_capacity(arguments.len()),
			categories: None,
		};
		for (index, argument) in arguments.iter().enumerate() {
			if name == "BUDGET_QUERY" && index == 1 {
				evaluated.categories = Some(self.categories(argument));
				evaluated.values.push(Ok(Value::Empty));
			} else {
				evaluated.values.push(self.argument(argument));
			}
		}
		counted(evaluated.len())?;
		self.apply(&name, evaluated)
	}

	fn aggregate(&mut self, name: &str, arguments: &[Option<Node>]) -> Outcome<Value> {
		let mut numbers = Vec::with_capacity(arguments.len());
		for argument in arguments {
			numbers.push(value::to_number(&self.argument(argument)?)?);
		}
		let total = || {
			numbers
				.iter()
				.fold(0.0, |total, number| value::add(total, *number))
		};
		let result = match name {
			"SUM" => total(),
			"AVERAGE" => total() / numbers.len() as f64,
			"MIN" => numbers.iter().copied().fold(f64::INFINITY, f64::min),
			_ => numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max),
		};
		Ok(Value::Number(finite(result)?))
	}

	fn apply(&mut self, name: &str, mut arguments: Arguments) -> Outcome<Value> {
		match name {
			"TRUE" => Ok(Value::Bool(true)),
			"FALSE" => Ok(Value::Bool(false)),
			"IF" => {
				let branch = if arguments.boolean(0)? { 1 } else { 2 };
				if branch < arguments.len() {
					arguments.take(branch)
				} else {
					Ok(Value::Bool(false))
				}
			}
			"AND" | "OR" => {
				let mut flags = Vec::with_capacity(arguments.len());
				for index in 0..arguments.len() {
					flags.push(arguments.boolean(index)?);
				}
				let and = name == "AND";
				Ok(Value::Bool(if and {
					flags.iter().all(|flag| *flag)
				} else {
					flags.iter().any(|flag| *flag)
				}))
			}
			"NOT" => Ok(Value::Bool(!arguments.boolean(0)?)),
			"ROUND" => {
				let number = arguments.number(0)?;
				let scale = 10f64.powf(arguments.number_or(1, 0.0)?);
				let rounded = value::js_round(number.abs() * scale) / scale;
				Ok(Value::Number(finite(rounded.copysign(number))?))
			}
			"ABS" => Ok(Value::Number(arguments.number(0)?.abs())),
			"CONCATENATE" => {
				let mut text = String::new();
				for index in 0..arguments.len() {
					text.push_str(&arguments.text(index)?);
				}
				Ok(Value::Text(text))
			}
			"LEN" => {
				let units = arguments.text(0)?.encode_utf16().count();
				Ok(Value::Number(units as f64))
			}
			"LEFT" | "RIGHT" => left_or_right(name, &mut arguments),
			"UPPER" => Ok(Value::Text(arguments.text(0)?.to_uppercase())),
			"LOWER" => Ok(Value::Text(arguments.text(0)?.to_lowercase())),
			"TODAY" => Ok(Value::Number(datetime::today_serial(self.today))),
			"DATE" => date(&mut arguments),
			"YEAR" | "MONTH" | "DAY" => {
				let serial = arguments.number(0)?;
				if serial < 0.0 {
					return Err(Kind::Num.into());
				}
				if serial > LARGEST_SERIAL_READ {
					return Err(unsupported(format!("{name} of the serial {serial}")).into());
				}
				let (year, month, day) = datetime::civil(serial.floor() as i128);
				let part = match name {
					"YEAR" => year,
					"MONTH" => month,
					_ => day,
				};
				Ok(Value::Number(part as f64))
			}
			"EOMONTH" => end_of_month(&mut arguments),
			"QUERY" => {
				let query = arguments.text(0)?;
				let sum = self.names.query_sums.get(&query).copied().unwrap_or(0.0);
				push_new(&mut self.names.query_names, query);
				Ok(Value::Number(sum))
			}
			"QUERY_EXTRACT_TIMEFRAME_START" => {
				let query = arguments.text(0)?;
				let start = self.names.starts.get(&query).cloned().unwrap_or_default();
				push_new(&mut self.names.start_names, query);
				Ok(Value::Text(start))
			}
			"QUERY_EXTRACT_TIMEFRAME_END" => {
				let query = arguments.text(0)?;
				let end = self.names.ends.get(&query).cloned().unwrap_or_default();
				push_new(&mut self.names.end_names, query);
				Ok(Value::Text(end))
			}
			"BUDGET_QUERY" => self.budget_query(arguments),
			_ => Err(unsupported(format!("{name} outside BUDGET_QUERY's categories")).into()),
		}
	}

	fn budget_query(&mut self, mut arguments: Arguments) -> Outcome<Value> {
		let dimension = arguments.text(0)?;
		let categories = arguments.categories.take().unwrap_or(Ok(Vec::new()))?;
		let start = arguments.text(2)?;
		let end = arguments.text(3)?;
		let request = BudgetRequest {
			dimension: dimension.to_lowercase(),
			categories,
			start,
			end,
		};
		let failed = self.names.budget_errors.contains_key(&request);
		let value = self.names.budget_values.get(&request).copied();
		push_new(&mut self.names.budget_requests, request);
		if failed {
			return Err(Kind::Value.into());
		}
		Ok(Value::Number(value.unwrap_or(0.0)))
	}

	/// `categoryRangeToIds`: the text and numbers among the values, split
	/// at commas. An error inside a larger array is skipped; a lone error
	/// is the result.
	fn categories(&mut self, argument: &Option<Node>) -> Outcome<Vec<String>> {
		let values = match argument {
			Some(Node::Array(rows)) if rows.len() == 1 && rows[0].len() == 1 => {
				vec![self.evaluate(&rows[0][0])?]
			}
			Some(Node::Array(rows)) => {
				let mut values = Vec::new();
				for element in rows.iter().flatten() {
					match self.evaluate(element) {
						Ok(value) => values.push(value),
						Err(Stop::Fault(_)) => {}
						Err(refusal) => return Err(refusal),
					}
				}
				values
			}
			Some(node @ Node::Call(_, inner)) if is_category_list(node) => {
				let mut names: Vec<Outcome<Value>> = inner
					.iter()
					.map(|argument| self.argument(argument))
					.collect();
				if names.len() != 1 {
					return Err(Kind::Na.into());
				}
				let query = value::to_text(&names.remove(0)?);
				let ids = self
					.names
					.categories
					.get(&query)
					.cloned()
					.unwrap_or_default();
				push_new(&mut self.names.category_names, query);
				ids.into_iter().map(Value::Text).collect()
			}
			_ => vec![self.argument(argument)?],
		};
		Ok(values
			.iter()
			.filter(|value| matches!(value, Value::Number(_) | Value::Text(_)))
			.flat_map(|value| {
				value::to_text(value)
					.split(',')
					.map(|id| id.trim_matches(is_js_space).to_owned())
					.collect::<Vec<_>>()
			})
			.filter(|id| !id.is_empty())
			.collect())
	}
}

/// LEFT and RIGHT count UTF-16 units, as JavaScript strings do.
fn left_or_right(name: &str, arguments: &mut Arguments) -> Outcome<Value> {
	let text = arguments.text(0)?;
	let count = arguments.number_or(1, 1.0)?;
	if count < 0.0 {
		return Err(Kind::Value.into());
	}
	let units: Vec<u16> = text.encode_utf16().collect();
	let wanted = count.trunc().min(units.len() as f64) as usize;
	// JavaScript's `slice(-n)` reads -0.5 as 0, so RIGHT of a count under 1
	// is the whole text, but a count of exactly 0 is empty.
	let slice = if name == "LEFT" {
		&units[..wanted]
	} else if count == 0.0 {
		&[]
	} else if wanted == 0 {
		&units[..]
	} else {
		&units[units.len() - wanted..]
	};
	String::from_utf16(slice)
		.map(Value::Text)
		.map_err(|_| unsupported(format!("{name} cutting a character of {text:?} in half")).into())
}

fn date(arguments: &mut Arguments) -> Outcome<Value> {
	let mut parts = [0i128; 3];
	for (index, part) in parts.iter_mut().enumerate() {
		*part = arguments.number(index)?.trunc().clamp(-1e18, 1e18) as i128;
	}
	let [year, month, day] = parts;
	let year = if year < 1900 { year + 1900 } else { year };
	let months = year * 12 + month - 1;
	let year = months.div_euclid(12);
	// A year before the Gregorian calendar is #VALUE!, any other date
	// outside 1899-12-30 to 9999-12-31 #NUM!.
	if year < 1582 {
		return Err(Kind::Value.into());
	}
	in_range(datetime::serial(year, months.rem_euclid(12) + 1, day))
}

/// EOMONTH takes only a whole number of months, to within
/// `precisionEpsilon`.
fn end_of_month(arguments: &mut Arguments) -> Outcome<Value> {
	let start = arguments.number(0)?;
	if start < 0.0 {
		return Err(Kind::Num.into());
	}
	let months = arguments.number(1)?;
	if !value::approximately_equal(months, months.round()) {
		return Err(Kind::Num.into());
	}
	// Past these no month count brings the end back before 9999.
	if start > LARGEST_SERIAL_READ || months.abs() > 1e15 {
		return Err(Kind::Num.into());
	}
	let (year, month, _) = datetime::civil(start.floor() as i128);
	let index = year * 12 + month - 1 + months.round() as i128;
	let (year, month) = (index.div_euclid(12), index.rem_euclid(12) + 1);
	in_range(datetime::serial(
		year,
		month,
		datetime::days_in_month(year, month),
	))
}

fn in_range(serial: i128) -> Outcome<Value> {
	if (0..=MAX_SERIAL).contains(&serial) {
		Ok(Value::Number(serial as f64))
	} else {
		Err(Kind::Num.into())
	}
}
