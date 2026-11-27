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

	/// MIG-2.4's goal lines: the Goals screen, Wrapped, and the suggestion
	/// and ETA (DEBT-8.1, FC-6, D77).
	public static func goalLines(_ preview: MigrationGoalPreview, format: MoneyFormat) -> [String] {
		var lines = ["The Goals screen lists the goal."]
		if let reached = preview.reachedMonth {
			lines.append("Wrapped counts it as a goal reached in \(ReportPeriod.monthYear(reached)).")
		} else {
			lines.append("Wrapped counts it in the year it's reached.")
		}
		if preview.spendingRecorded {
			lines.append("No suggestion or ETA, because spending is recorded.")
		} else if let monthly = preview.monthly, let months = preview.monthsLeft {
			let span = months == 1 ? "1 month" : "\(months) months"
			lines.append("It suggests \(format.currency(monthly, on: .budget)) a month for \(span), and shows an ETA.")
		} else if preview.reachedMonth == nil {
			lines.append("It shows an ETA once it has a pace.")
		}
		return lines
	}

	/// `2027-06-30` as the Mac writes a date.
	public static func day(_ text: String) -> String {
		guard let date = CivilDate.date(text) else { return text }
		return date.formatted(date: .abbreviated, time: .omitted)
	}
}
