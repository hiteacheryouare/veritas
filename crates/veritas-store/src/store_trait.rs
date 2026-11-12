use veritas_domain::{Change, CoreError, StoreTransaction};

use crate::connection::Store;
use crate::write::Writer;

impl veritas_domain::Store for Store {
	type Transaction<'a> = Writer<'a>;

	fn begin(&mut self) -> Result<Writer<'_>, CoreError> {
		self.writer()
	}
}

impl StoreTransaction for Writer<'_> {
	fn apply(&mut self, change: &Change) -> Result<(), CoreError> {
		match change {
			Change::Account(delta) => self.put_account(delta.before.as_ref(), delta.after.as_ref()),
			Change::AccountGroup(delta) => {
				self.put_account_group(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::CategoryGroup(delta) => {
				self.put_category_group(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::Category(delta) => {
				self.put_category(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::Payee(delta) => self.put_payee(delta.before.as_ref(), delta.after.as_ref()),
			Change::Transaction(delta) => {
				self.put_transaction(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::Rule(delta) => self.put_rule(delta.before.as_ref(), delta.after.as_ref()),
			Change::Schedule(delta) => {
				self.put_schedule(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::Tag(delta) => self.put_tag(delta.before.as_ref(), delta.after.as_ref()),
			Change::BudgetCell(delta) => {
				self.put_budget_cell(delta.before.as_ref(), delta.after.as_ref())
			}
			Change::Hold(delta) => self.put_hold(
				delta.key,
				delta.before.as_ref().map(|hold| hold.amount),
				delta.after.as_ref().map(|hold| hold.amount),
			),
			Change::Note(delta) => self.put_note(
				&delta.key,
				delta.before.as_ref().map(|note| note.text.as_deref()),
				delta.after.as_ref().map(|note| note.text.as_deref()),
			),
			Change::CategoryMapping(delta) => self.put_category_mapping(
				&delta.key,
				delta.before.as_ref().map(|row| &row.to),
				delta.after.as_ref().map(|row| &row.to),
			),
			Change::PayeeMapping(delta) => self.put_payee_mapping(
				&delta.key,
				delta.before.as_ref().map(|row| &row.to),
				delta.after.as_ref().map(|row| &row.to),
			),
			Change::CategoryMeta(delta) => self.put_category_meta(
				&delta.key,
				delta.before.as_ref().map(|row| &row.meta),
				delta.after.as_ref().map(|row| &row.meta),
			),
			Change::AccountMeta(delta) => self.put_account_meta(
				&delta.key,
				delta.before.as_ref().map(|row| &row.meta),
				delta.after.as_ref().map(|row| &row.meta),
			),
			Change::TransactionMeta(delta) => self.put_transaction_meta(
				&delta.key,
				delta.before.as_ref().map(|row| &row.meta),
				delta.after.as_ref().map(|row| &row.meta),
			),
			Change::Goal(delta) => self.put_goal(delta.before.as_ref(), delta.after.as_ref()),
		}
	}

	fn commit(self) -> Result<(), CoreError> {
		Writer::commit(self)
	}

	fn rollback(self) -> Result<(), CoreError> {
		Writer::rollback(self)
	}
}
