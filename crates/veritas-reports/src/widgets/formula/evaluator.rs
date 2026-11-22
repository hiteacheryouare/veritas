//! Written clean-room from HyperFormula's public docs and black-box tests
//! against Actual's formula card. No HyperFormula source was read.
//!
//! Walks a formula's tree. HyperFormula evaluates nearly everything, and
//! the first error in reading order is the result: the left operand's
//! before the right's, and each argument's error, or its failure to
//! convert, before the next argument's. Evaluating everything matters
//! because QUERY and its kin record what to fetch as they run.

use std::cmp::Ordering;

use chrono::NaiveDate;

use super::FormulaErrorKind as Kind;
use super::engine::Names;
use super::parser::{Comparison, Node, Operator};
use super::value::{self, Outcome, Value, finite};
use crate::report_query::unsupported;

pub(super) struct Evaluator<'a> {
	pub names: &'a mut Names,
	pub today: NaiveDate,
}

/// The one place Veritas evaluates an array: the categories of
/// BUDGET_QUERY, written as an array or as QUERY_EXTRACT_CATEGORIES.
pub(super) fn is_category_list(node: &Node) -> bool {
	match node {
		Node::Array(_) => true,
		Node::Call(name, _) => name.eq_ignore_ascii_case("QUERY_EXTRACT_CATEGORIES"),
		_ => false,
	}
}

/// What Veritas refuses before evaluating anything: references, which the
/// card has no cells for (D75), and arrays outside BUDGET_QUERY, which
/// HyperFormula spills into cells the card doesn't have.
pub(super) fn check(node: &Node) -> Outcome<()> {
	match node {
		Node::Reference(text) => Err(unsupported(format!("the cell reference {text}")).into()),
		Node::Array(_) => {
			Err(unsupported("an array outside BUDGET_QUERY's categories".to_owned()).into())
		}
		Node::Call(name, _) if is_category_list(node) => {
			Err(unsupported(format!("{name} outside BUDGET_QUERY's categories")).into())
		}
		Node::Call(name, arguments) => {
			let budget_query = name.eq_ignore_ascii_case("BUDGET_QUERY");
			for (index, argument) in arguments.iter().enumerate() {
				let Some(argument) = argument else {
					continue;
				};
				match argument {
					Node::Array(rows) if budget_query && index == 1 => {
						rows.iter().flatten().try_for_each(check)?;
					}
					Node::Call(_, inner)
						if budget_query && index == 1 && is_category_list(argument) =>
					{
						inner.iter().flatten().try_for_each(check)?;
					}
					_ => check(argument)?,
				}
			}
			Ok(())
		}
		Node::Plus(inner) | Node::Minus(inner) | Node::Percent(inner) => check(inner),
		Node::Chain(first, rest) => {
			check(first)?;
			rest.iter().try_for_each(|(_, operand)| check(operand))
		}
		Node::Number(_) | Node::Text(_) | Node::Fault(_) | Node::Name(_) => Ok(()),
	}
}

impl Evaluator<'_> {
	pub(super) fn evaluate(&mut self, node: &Node) -> Outcome<Value> {
		match node {
			Node::Number(number) => Ok(Value::Number(finite(*number)?)),
			Node::Text(text) => Ok(Value::Text(text.clone())),
			Node::Fault(kind) => Err((*kind).into()),
			Node::Name(_) => Err(Kind::Name.into()),
			Node::Reference(text) => Err(unsupported(format!("the cell reference {text}")).into()),
			Node::Array(_) => {
				Err(unsupported("an array outside BUDGET_QUERY's categories".to_owned()).into())
			}
			Node::Plus(inner) => self.evaluate(inner),
			Node::Minus(inner) => {
				let number = value::to_number(&self.evaluate(inner)?)?;
				Ok(Value::Number(-number))
			}
			Node::Percent(inner) => {
				let number = value::to_number(&self.evaluate(inner)?)?;
				Ok(Value::Number(number / 100.0))
			}
			Node::Chain(first, rest) => {
				// Left to right, as nested binary operators would run: each
				// operand is evaluated before the error so far counts, and
				// an error on the left wins over one on the right.
				let mut result = self.evaluate(first);
				for (operator, operand) in rest {
					let right = self.evaluate(operand);
					result = result.and_then(|left| binary(*operator, &left, &right?));
				}
				result
			}
			Node::Call(name, arguments) => self.call(name, arguments),
		}
	}

	/// An argument's value; one left empty is an empty value.
	pub(super) fn argument(&mut self, argument: &Option<Node>) -> Outcome<Value> {
		match argument {
			Some(node) => self.evaluate(node),
			None => Ok(Value::Empty),
		}
	}
}

fn binary(operator: Operator, left: &Value, right: &Value) -> Outcome<Value> {
	if let Operator::Compare(comparison) = operator {
		let order = value::compare(left, right)?;
		return Ok(Value::Bool(match comparison {
			Comparison::Equal => order == Ordering::Equal,
			Comparison::NotEqual => order != Ordering::Equal,
			Comparison::Less => order == Ordering::Less,
			Comparison::LessOrEqual => order != Ordering::Greater,
			Comparison::Greater => order == Ordering::Greater,
			Comparison::GreaterOrEqual => order != Ordering::Less,
		}));
	}
	if operator == Operator::Concat {
		return Ok(Value::Text(value::to_text(left) + &value::to_text(right)));
	}
	let a = value::to_number(left)?;
	let b = value::to_number(right)?;
	let result = match operator {
		Operator::Add => value::add(a, b),
		Operator::Subtract => value::subtract(a, b),
		Operator::Multiply => a * b,
		Operator::Divide if b == 0.0 => return Err(Kind::DivByZero.into()),
		Operator::Divide => a / b,
		Operator::Power => a.powf(b),
		Operator::Concat | Operator::Compare(_) => return Err(Kind::Error.into()),
	};
	Ok(Value::Number(finite(result)?))
}
