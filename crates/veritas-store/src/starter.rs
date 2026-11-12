use veritas_domain::{CategoryGroupId, CategoryId, CategoryKind};

/// A category group to create in a new budget. `seed` reuses a row of
/// Actual's template by id instead of inserting a new one.
#[derive(Clone, Debug, PartialEq)]
pub struct StarterGroup {
	pub seed: Option<CategoryGroupId>,
	pub name: String,
	pub is_income: bool,
	pub categories: Vec<StarterCategory>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StarterCategory {
	pub seed: Option<CategoryId>,
	pub name: String,
	pub kind: CategoryKind,
	pub tax_bucket: Option<String>,
}

const INCOME_GROUP: &str = "2E1F5BDB-209B-43F9-AF2C-3CE28E380C00";
const USUAL_EXPENSES_GROUP: &str = "fc3825fd-b982-4b72-b768-5b30844cf832";
const INVESTMENTS_GROUP: &str = "a137772f-cf2f-4089-9432-822d2ddc1466";
const STARTING_BALANCES: &str = "506e8d9d-7ed0-4397-84e4-07a9185dc6b2";
const INCOME: &str = "3c1699a5-522a-435e-86dc-93d900a14f0e";
const BILLS: &str = "d4b0f075-3343-4408-91ed-fae94f74e5bf";
const BILLS_FLEXIBLE: &str = "29ec2c58-8cd3-42fe-9187-7b5dfe0f70a6";
const FOOD: &str = "541836f1-e756-4473-a5d0-6c1d3f06c7fa";
const GENERAL: &str = "af375fd4-d759-46b3-bffe-74a856151d57";
const SAVINGS: &str = "6bbd8472-25d4-4cee-8a11-5bd9f7e83d61";

type Row = (
	&'static str,
	Option<&'static str>,
	CategoryKind,
	Option<&'static str>,
);

fn group(seed: Option<&str>, name: &str, is_income: bool, rows: &[Row]) -> StarterGroup {
	StarterGroup {
		seed: seed.map(CategoryGroupId::new),
		name: name.to_owned(),
		is_income,
		categories: rows
			.iter()
			.map(|&(name, seed, kind, bucket)| StarterCategory {
				seed: seed.map(CategoryId::new),
				name: name.to_owned(),
				kind,
				tax_bucket: bucket.map(str::to_owned),
			})
			.collect(),
	}
}

/// Veritas's starter categories, docs/finance/starter-categories.md SC-3 and
/// SC-4: 35 categories in 9 groups, reusing all of Actual's seed rows.
pub fn veritas_starter_set() -> Vec<StarterGroup> {
	use CategoryKind::{DebtPrincipal, Expense, Income, Savings};
	vec![
		group(
			Some(USUAL_EXPENSES_GROUP),
			"Housing",
			false,
			&[
				("Rent", Some(BILLS), Expense, None),
				("Mortgage Escrow", None, Expense, None),
				("Home Insurance", None, Expense, None),
				("Home Maintenance", None, Expense, None),
			],
		),
		group(
			None,
			"Utilities",
			false,
			&[
				("Electricity and Gas", Some(BILLS_FLEXIBLE), Expense, None),
				("Water and Trash", None, Expense, None),
				("Internet and Phone", None, Expense, None),
			],
		),
		group(
			None,
			"Food",
			false,
			&[
				("Groceries", Some(FOOD), Expense, None),
				("Dining Out", None, Expense, None),
			],
		),
		group(
			None,
			"Transportation",
			false,
			&[
				("Fuel and Transit", None, Expense, None),
				("Car Insurance", None, Expense, None),
				("Car Maintenance", None, Expense, None),
			],
		),
		group(
			None,
			"Health",
			false,
			&[
				("Health Insurance", None, Expense, None),
				("Medical", None, Expense, Some("medical")),
			],
		),
		group(
			None,
			"Personal and Family",
			false,
			&[
				("Clothing and Personal Care", None, Expense, None),
				("Childcare", None, Expense, None),
				("Education", None, Expense, Some("education")),
				("Entertainment", None, Expense, None),
				("Gifts", None, Expense, None),
				("Charity", None, Expense, Some("charitable")),
				("Miscellaneous", Some(GENERAL), Expense, None),
			],
		),
		group(
			None,
			"Paycheck Deductions",
			false,
			&[
				("Federal Income Tax", None, Expense, Some("taxes_withheld")),
				("State Income Tax", None, Expense, Some("taxes_withheld")),
				(
					"Social Security and Medicare",
					None,
					Expense,
					Some("taxes_withheld"),
				),
				(
					"Retirement Contribution",
					None,
					Savings,
					Some("retirement_contribution"),
				),
				("HSA Contribution", None, Savings, Some("hsa")),
			],
		),
		group(
			Some(INVESTMENTS_GROUP),
			"Savings and Debt",
			false,
			&[
				("Savings", Some(SAVINGS), Savings, None),
				("Investments", None, Savings, None),
				("Loan Principal", None, DebtPrincipal, None),
				("Interest and Fees", None, Expense, None),
			],
		),
		group(
			Some(INCOME_GROUP),
			"Income",
			true,
			&[
				("Wages", Some(INCOME), Income, Some("w2_income")),
				("Interest", None, Income, Some("interest_income")),
				("Business Income", None, Income, Some("business_income")),
				("Other Income", None, Income, None),
				("Starting Balances", Some(STARTING_BALANCES), Income, None),
			],
		),
	]
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_starter_set_follows_its_rules() {
		let set = veritas_starter_set();
		assert_eq!(set.len(), 9);
		assert_eq!(set.iter().map(|g| g.categories.len()).sum::<usize>(), 35);
		let income: Vec<_> = set.iter().filter(|g| g.is_income).collect();
		assert_eq!(income.len(), 1, "SC-2.2");
		assert_eq!(income[0].categories.len(), 5);
		for group in &set {
			for category in &group.categories {
				assert_eq!(
					category.kind == CategoryKind::Income,
					group.is_income,
					"SC-2.1: {}",
					category.name
				);
			}
		}
		let seeds = set
			.iter()
			.flat_map(|g| g.categories.iter())
			.filter(|c| c.seed.is_some())
			.count();
		assert_eq!(seeds, 7, "SC-4 reuses every seed category");
		assert_eq!(set.iter().filter(|g| g.seed.is_some()).count(), 3);
	}
}
