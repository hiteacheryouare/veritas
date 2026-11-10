use std::path::Path;

use veritas_domain::{Change, CoreError, Store, StoreTransaction};
use veritas_store::{Access, AuditReport, Loaded, Writer};

use crate::session::Backend;

/// veritas-store's working copy behind a session.
pub struct StoreBackend {
	store: veritas_store::Store,
}

/// One command's writes, as one SQLite transaction.
pub struct StoreWrite<'a> {
	writer: Writer<'a>,
}

impl Store for StoreBackend {
	type Transaction<'a> = StoreWrite<'a>;

	fn begin(&mut self) -> Result<StoreWrite<'_>, CoreError> {
		self.store.writer().map(|writer| StoreWrite { writer })
	}
}

impl StoreTransaction for StoreWrite<'_> {
	fn apply(&mut self, change: &Change) -> Result<(), CoreError> {
		let w = &mut self.writer;
		match change {
			Change::Account(d) => w.put_account(d.before.as_ref(), d.after.as_ref()),
			Change::AccountGroup(d) => w.put_account_group(d.before.as_ref(), d.after.as_ref()),
			Change::CategoryGroup(d) => w.put_category_group(d.before.as_ref(), d.after.as_ref()),
			Change::Category(d) => w.put_category(d.before.as_ref(), d.after.as_ref()),
			Change::Payee(d) => w.put_payee(d.before.as_ref(), d.after.as_ref()),
			Change::Transaction(d) => w.put_transaction(d.before.as_ref(), d.after.as_ref()),
			Change::Rule(d) => w.put_rule(d.before.as_ref(), d.after.as_ref()),
			Change::Schedule(d) => w.put_schedule(d.before.as_ref(), d.after.as_ref()),
			Change::Tag(d) => w.put_tag(d.before.as_ref(), d.after.as_ref()),
			Change::BudgetCell(d) => w.put_budget_cell(d.before.as_ref(), d.after.as_ref()),
			Change::Hold(d) => w.put_hold(
				d.key,
				d.before.as_ref().map(|hold| hold.amount),
				d.after.as_ref().map(|hold| hold.amount),
			),
			Change::Note(d) => w.put_note(
				&d.key,
				d.before.as_ref().map(|note| note.text.as_str()),
				d.after.as_ref().map(|note| note.text.as_str()),
			),
			Change::CategoryMapping(d) => w.put_category_mapping(
				&d.key,
				d.before.as_ref().map(|row| &row.to),
				d.after.as_ref().map(|row| &row.to),
			),
			Change::PayeeMapping(d) => w.put_payee_mapping(
				&d.key,
				d.before.as_ref().map(|row| &row.to),
				d.after.as_ref().map(|row| &row.to),
			),
			Change::CategoryMeta(d) => w.put_category_meta(
				&d.key,
				d.before.as_ref().map(|row| &row.meta),
				d.after.as_ref().map(|row| &row.meta),
			),
			Change::AccountMeta(d) => w.put_account_meta(
				&d.key,
				d.before.as_ref().map(|row| &row.meta),
				d.after.as_ref().map(|row| &row.meta),
			),
			Change::TransactionMeta(d) => w.put_transaction_meta(
				&d.key,
				d.before.as_ref().map(|row| &row.meta),
				d.after.as_ref().map(|row| &row.meta),
			),
			Change::TaxTag(_) => Err(CoreError::Invalid(
				"tax tags can't be saved yet. This is coming in a later version".to_owned(),
			)),
			Change::Goal(d) => w.put_goal(d.before.as_ref(), d.after.as_ref()),
		}
	}

	fn commit(self) -> Result<(), CoreError> {
		self.writer.commit()
	}

	fn rollback(self) -> Result<(), CoreError> {
		self.writer.rollback()
	}
}

impl Backend for StoreBackend {
	fn open(db: &Path, access: Access) -> Result<Self, CoreError> {
		veritas_store::Store::open(db, access).map(|store| StoreBackend { store })
	}

	fn migration_ids(&self) -> Result<Vec<i64>, CoreError> {
		self.store.migration_ids()
	}

	fn quick_problems(&self) -> Result<Vec<String>, CoreError> {
		let verdict = self.store.pragma_text("quick_check")?;
		Ok(if verdict == "ok" {
			Vec::new()
		} else {
			vec![verdict]
		})
	}

	fn verify(db: &Path) -> Result<AuditReport, CoreError> {
		let (_, report) = veritas_store::audit_connection(&veritas_store::open_read_only(db)?)?;
		Ok(report)
	}

	fn adopt(&mut self) -> Result<(), CoreError> {
		self.store.adopt()
	}

	fn load(&self) -> Result<Loaded, CoreError> {
		self.store.load()
	}
}
