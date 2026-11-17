//! D74: cards show register-scale figures at their true amount in any
//! currency. Actual's cards add budget cells, which carry the currency's
//! places, to transaction amounts, which always carry 2 (D63), so in a
//! 0-place budget its budget-derived figures come out 100 times too small
//! beside the register. Veritas computes every card on the register's scale
//! instead, so each money figure a card returns is a register-scale integer.

use veritas_domain::{BudgetData, CoreError, Money};

/// How many register units one budget-cell unit is: `10^(2 - places)`.
pub fn register_factor(budget: &BudgetData) -> f64 {
	10_f64.powi(2 - i32::from(budget.format.decimal_places()))
}

fn rescale(amount: Money, places: u8) -> Result<Money, CoreError> {
	let minor = i128::from(amount.minor());
	let scaled = if places <= 2 {
		minor * 10_i128.pow(u32::from(2 - places))
	} else {
		let divisor = 10_i128.pow(u32::from(places - 2));
		let half = divisor / 2;
		if minor >= 0 {
			(minor + half) / divisor
		} else {
			(minor - half) / divisor
		}
	};
	i64::try_from(scaled)
		.map(Money::from_minor)
		.map_err(|_| CoreError::Overflow)
}

/// The budget with its budget cells, goals and holds moved to the
/// register's scale, or `None` when the currency already has 2 places. A
/// 3-place currency's budget amounts round to the register's 2 places.
pub fn register_scale(budget: &BudgetData) -> Result<Option<BudgetData>, CoreError> {
	let places = budget.format.decimal_places();
	if places == 2 {
		return Ok(None);
	}
	let mut scaled = budget.clone();
	for cell in scaled.budget_cells.values_mut() {
		cell.budgeted = rescale(cell.budgeted, places)?;
		cell.goal = cell.goal.map(|goal| rescale(goal, places)).transpose()?;
	}
	for hold in scaled.holds.values_mut() {
		*hold = hold.map(|amount| rescale(amount, places)).transpose()?;
	}
	Ok(Some(scaled))
}

#[cfg(test)]
mod tests {
	use veritas_domain::BudgetType;

	use super::*;

	#[test]
	fn budget_amounts_move_to_the_registers_two_places() {
		assert_eq!(rescale(Money::from_minor(500), 0).unwrap().minor(), 50_000);
		assert_eq!(rescale(Money::from_minor(-7), 1).unwrap().minor(), -70);
		assert_eq!(rescale(Money::from_minor(1_235), 3).unwrap().minor(), 124);
		assert_eq!(rescale(Money::from_minor(-1_235), 3).unwrap().minor(), -124);
		let mut budget = BudgetData::empty(BudgetType::Envelope);
		assert!(register_scale(&budget).unwrap().is_none());
		budget.format.currency_code = Some("JPY".to_owned());
		assert!((register_factor(&budget) - 100.0).abs() < f64::EPSILON);
		assert!(register_scale(&budget).unwrap().is_some());
	}
}
