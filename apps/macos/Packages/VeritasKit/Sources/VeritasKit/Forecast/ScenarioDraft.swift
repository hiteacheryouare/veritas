import Foundation
import VeritasCore

/// One what-if event as its editor holds it: every kind's fields at once, so
/// switching kinds keeps what was typed. `change(magnitude:)` builds the
/// `ScenarioChange` the core checks (FC-3, R-FC-2).
public struct ScenarioDraft: Hashable, Sendable {
	public enum Kind: String, CaseIterable, Hashable, Sendable {
		case oneTime
		case recurring
		case categoryPercent
		case incomeChange
		case goalContribution

		public var title: String {
			switch self {
			case .oneTime: "One time"
			case .recurring: "Repeating"
			case .categoryPercent: "Category change"
			case .incomeChange: "Income change"
			case .goalContribution: "Goal contribution"
			}
		}

		/// A goal contribution is a budget move, on the budget's scale; the
		/// others are transactions, on the register's (CLS-12.9).
		public var scale: MoneyFormat.Scale {
			self == .goalContribution ? .budget : .register
		}

	}

	/// Money out is Actual's negative sign; for a percent or income change,
	/// `.down` is a cut.
	public enum Direction: Hashable, Sendable {
		case down
		case up
	}

	public enum CadenceUnit: String, CaseIterable, Hashable, Sendable {
		case months
		case weeks
	}

	public var kind: Kind
	/// A one-time event's date, or the date a repeating one starts on, `YYYY-MM-DD`.
	public var date: String
	public var accountID: String?
	public var categoryID: String?
	public var goalID: String?
	public var direction: Direction
	public var cadenceUnit: CadenceUnit
	public var cadenceCount: UInt32
	public var startMonth: Month
	public var endMonth: Month?
	public var incomeByPercent: Bool
	/// What's typed for a percent, such as `10` or `2.5`.
	public var percentText: String
	/// The magnitude the amount field opens on, on `kind.scale`.
	public var magnitude: Int64

	public init(today: String, accountID: String? = nil) {
		kind = .oneTime
		date = today
		self.accountID = accountID
		categoryID = nil
		goalID = nil
		direction = .down
		cadenceUnit = .months
		cadenceCount = 1
		startMonth = Month(date: today) ?? Month(year: 2026, month: 1)
		endMonth = nil
		incomeByPercent = false
		percentText = ""
		magnitude = 0
	}

	/// The editor for an event that exists. A percent opens in the budget's
	/// decimal separator, so the core reads it back the same way.
	public init(_ change: ScenarioChange, today: String, decimalSeparator: String = ".") {
		self.init(today: today)
		switch change {
		case .oneTime(let date, let accountID, let categoryID, let amount):
			kind = .oneTime
			self.date = date
			self.accountID = accountID
			self.categoryID = categoryID
			setSigned(amount)
		case .recurring(let start, let endMonth, let cadence, let accountID, let categoryID, let amount):
			kind = .recurring
			date = start
			self.endMonth = endMonth
			switch cadence {
			case .everyMonths(let count): (cadenceUnit, cadenceCount) = (.months, count)
			case .everyWeeks(let count): (cadenceUnit, cadenceCount) = (.weeks, count)
			}
			self.accountID = accountID
			self.categoryID = categoryID
			setSigned(amount)
		case .categoryPercent(let categoryID, let percentBp, let startMonth, let endMonth):
			kind = .categoryPercent
			self.categoryID = categoryID
			setPercent(percentBp, decimalSeparator)
			self.startMonth = startMonth
			self.endMonth = endMonth
		case .incomeChange(let accountID, let categoryID, let change, let startMonth, let endMonth):
			kind = .incomeChange
			self.accountID = accountID
			self.categoryID = categoryID
			switch change {
			case .amount(let amount):
				incomeByPercent = false
				setSigned(amount)
			case .percent(let percentBp):
				incomeByPercent = true
				setPercent(percentBp, decimalSeparator)
			}
			self.startMonth = startMonth
			self.endMonth = endMonth
		case .goalContribution(let goalID, let amount, let startMonth, let endMonth):
			kind = .goalContribution
			self.goalID = goalID
			setSigned(amount)
			self.startMonth = startMonth
			self.endMonth = endMonth
		}
	}

	public var usesPercent: Bool {
		kind == .categoryPercent || (kind == .incomeChange && incomeByPercent)
	}

	public var usesAmountField: Bool {
		!usesPercent
	}

	public struct Problem: Error, Hashable, Sendable {
		public let message: String

		public init(message: String) {
			self.message = message
		}
	}

	public static let percentMissing = "Enter a percent, such as 10 or 2.5."

	/// The change to send, with `magnitude` on `kind.scale` when the kind takes
	/// an amount, and `percent` in basis points, as the core read the percent
	/// field, when it takes one. Throws the line to show when a choice is missing.
	public func change(magnitude: Int64, percent: Int64? = nil) throws(Problem) -> ScenarioChange {
		let signed = direction == .down ? -magnitude : magnitude
		switch kind {
		case .oneTime:
			return .oneTime(date: date, accountId: try account(), categoryId: categoryID, amount: signed)
		case .recurring:
			let cadence: ScenarioCadence = cadenceUnit == .months ? .everyMonths(count: cadenceCount) : .everyWeeks(count: cadenceCount)
			return .recurring(start: date, endMonth: endMonth, cadence: cadence, accountId: try account(), categoryId: categoryID, amount: signed)
		case .categoryPercent:
			guard let categoryID else { throw Problem(message: "Choose a category.") }
			return .categoryPercent(categoryId: categoryID, percentBp: try signedPercent(percent), startMonth: startMonth, endMonth: endMonth)
		case .incomeChange:
			let change: IncomeChangeAmount = incomeByPercent ? .percent(percentBp: try signedPercent(percent)) : .amount(amount: signed)
			return .incomeChange(accountId: try account(), categoryId: categoryID, change: change, startMonth: startMonth, endMonth: endMonth)
		case .goalContribution:
			guard let goalID else { throw Problem(message: "Choose a goal.") }
			return .goalContribution(goalId: goalID, amount: magnitude, startMonth: startMonth, endMonth: endMonth)
		}
	}

	private func account() throws(Problem) -> String {
		guard let accountID else { throw Problem(message: "Choose an account.") }
		return accountID
	}

	private func signedPercent(_ percent: Int64?) throws(Problem) -> Int64 {
		guard let percent else { throw Problem(message: Self.percentMissing) }
		return direction == .down ? -percent : percent
	}

	private mutating func setSigned(_ amount: Int64) {
		direction = amount < 0 ? .down : .up
		magnitude = Int64(amount.magnitude)
	}

	private mutating func setPercent(_ basisPoints: Int64, _ decimalSeparator: String) {
		direction = basisPoints < 0 ? .down : .up
		percentText = ForecastWords.percentDigits(basisPoints, decimalSeparator: decimalSeparator)
	}
}
