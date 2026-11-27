import Foundation
import VeritasCore

/// The upgrade sheet's sentences with their figures (MIG-2.4, MIG-5.4,
/// MIG-5.6).
public enum UpgradeWords {
	public static func kindName(_ kind: CategoryKind) -> String {
		switch kind {
		case .debtPrincipal: "debt paydown"
		case .savings: "savings"
		case .income: "income"
		case .expense: "spending"
		}
	}

	/// "3 rows will change: 3 transfers of 1,156.50 move from spending to
	/// debt paydown, and 1 purchase will be flagged as misfiled." Reverting
	/// reads it the other way.
	public static func kindChange(_ change: MigrationKindChange, format: MoneyFormat, reverting: Bool) -> String {
		let rows = change.rows == 1 ? "1 row will change" : "\(change.rows) rows will change"
		let transfers = change.transfers == 1 ? "1 transfer" : "\(change.transfers) transfers"
		let amount = format.amount(change.amount, on: .register)
		let kind = kindName(change.kind)
		let moves = reverting ? "move from \(kind) back to spending" : "move from spending to \(kind)"
		var sentence = "\(rows): \(transfers) of \(amount) \(moves)"
		if change.purchases > 0 {
			let purchases = change.purchases == 1 ? "1 purchase" : "\(change.purchases) purchases"
			let flag = reverting ? "will no longer be flagged as misfiled" : "will be flagged as misfiled"
			sentence += ", and \(purchases) \(flag)"
		}
		return sentence + "."
	}

	/// "Expenses, October 2026: 1,585.50 → 1,200.00".
	public static func effect(_ effect: MigrationEffect, format: MoneyFormat) -> String {
		let month = effect.month.map { ", \(ReportPeriod.monthYear($0))" } ?? ""
		return "\(effect.report)\(month): \(format.amount(effect.before, on: .register)) → \(format.amount(effect.after, on: .register))"
	}

	/// `2027-06-30` as the Mac writes a date.
	public static func day(_ text: String) -> String {
		guard let date = CivilDate.date(text) else { return text }
		return date.formatted(date: .abbreviated, time: .omitted)
	}
}
