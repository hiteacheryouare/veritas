import Foundation
import VeritasCore

/// The sentences the Forecast and What-ifs pages build from the core's
/// records. Amounts are formatted, never computed.
public struct ForecastWords: Sendable {
	public let names: BudgetNames
	public let goals: [GoalSummary]
	public let format: MoneyFormat

	public init(names: BudgetNames = .empty, goals: [GoalSummary] = [], format: MoneyFormat) {
		self.names = names
		self.goals = goals
		self.format = format
	}

	// MARK: What-if events

	/// "Vacation: −$2,000.00 on March 10, 2027, Checking".
	public func sentence(_ change: ScenarioChange) -> String {
		switch change {
		case .oneTime(let date, let accountID, let categoryID, let amount):
			return labeled(category(categoryID), "\(signed(amount, on: .register)) on \(Self.day(date)), \(account(accountID))")
		case .recurring(let start, let endMonth, let cadence, let accountID, let categoryID, let amount):
			let span = endMonth.map { "from \(Self.day(start)) through \(ReportPeriod.monthYear($0))" } ?? "from \(Self.day(start))"
			return labeled(category(categoryID), "\(signed(amount, on: .register)) \(Self.cadence(cadence)) \(span), \(account(accountID))")
		case .categoryPercent(let categoryID, let percentBp, let startMonth, let endMonth):
			return labeled(category(categoryID) ?? Self.missingCategory, "\(percent(percentBp)) \(Self.span(startMonth, endMonth))")
		case .incomeChange(let accountID, let categoryID, let change, let startMonth, let endMonth):
			let size = switch change {
			case .amount(let amount): "\(signed(amount, on: .register)) a month"
			case .percent(let percentBp): percent(percentBp)
			}
			let name = categoryID == nil ? "All income" : category(categoryID) ?? Self.missingCategory
			return labeled(name, "\(size) \(Self.span(startMonth, endMonth)), \(account(accountID))")
		case .goalContribution(let goalID, let amount, let startMonth, let endMonth):
			let name = goals.first { $0.id == goalID }?.name ?? "Missing goal"
			return labeled(name, "\(signed(amount, on: .budget)) a month \(Self.span(startMonth, endMonth))")
		}
	}

	static let missingCategory = "Missing category"

	private func labeled(_ name: String?, _ rest: String) -> String {
		name.map { "\($0): \(rest)" } ?? rest
	}

	private func account(_ id: String) -> String {
		names.account(id) ?? "Missing account"
	}

	private func category(_ id: String?) -> String? {
		guard let id else { return nil }
		return names.category(id) ?? Self.missingCategory
	}

	/// `+$500.00`, `−$2,000.00`: a what-if names its direction with its sign.
	public func signed(_ amount: Int64, on scale: MoneyFormat.Scale) -> String {
		let text = format.currency(amount, on: scale)
		return amount > 0 ? "+" + text : text
	}

	/// "every month", "every 2 weeks".
	static func cadence(_ cadence: ScenarioCadence) -> String {
		switch cadence {
		case .everyMonths(let count): count == 1 ? "every month" : "every \(count) months"
		case .everyWeeks(let count): count == 1 ? "every week" : "every \(count) weeks"
		}
	}

	static func span(_ start: Month, _ end: Month?) -> String {
		guard let end else { return "from \(ReportPeriod.monthYear(start))" }
		return "from \(ReportPeriod.monthYear(start)) through \(ReportPeriod.monthYear(end))"
	}

	/// `+10%`, `−2.5%`, `+2,5%` in a comma-decimal budget.
	public func percent(_ basisPoints: Int64) -> String {
		let sign = basisPoints < 0 ? MoneyFormat.minus : "+"
		return "\(sign)\(Self.percentDigits(basisPoints, decimalSeparator: format.decimalSeparator))%"
	}

	/// `1000` reads `10` and `250` reads `2.5`, unsigned, with no trailing
	/// zeros: the digits a percent shows and its field opens on.
	public static func percentDigits(_ basisPoints: Int64, decimalSeparator: String) -> String {
		let magnitude = basisPoints.magnitude
		let whole = magnitude / 100
		let fraction = magnitude % 100
		guard fraction > 0 else { return "\(whole)" }
		let digits = fraction % 10 == 0 ? "\(fraction / 10)" : String(format: "%02d", fraction)
		return "\(whole)\(decimalSeparator)\(digits)"
	}

	/// "March 10, 2027".
	public static func day(_ text: String) -> String {
		CivilDate.date(text)?.formatted(.dateTime.month(.wide).day().year()) ?? text
	}

	/// "Mar 10, 2027".
	public static func shortDay(_ text: String) -> String {
		CivilDate.date(text)?.formatted(.dateTime.month(.abbreviated).day().year()) ?? text
	}

	// MARK: Forecast lists

	/// "Checking → Rent": the account, then the category or the other
	/// account. Income reads the other way, as on the Bills screen.
	public func route(_ flow: ForecastFlow, accounts: [ForecastAccount]) -> String {
		let other = flow.categoryName ?? flow.counterpartAccountId.flatMap { id in accounts.first { $0.accountId == id }?.name }
		guard let other else { return flow.accountName }
		return flow.kind == .income ? "\(other) → \(flow.accountName)" : "\(flow.accountName) → \(other)"
	}

	/// DEBT-2.2 and DEBT-2.6: the payment covers less than its interest and
	/// escrow, and the difference isn't recorded.
	public func shortfall(_ item: ForecastShortfall) -> String {
		"\(item.loanName), \(Self.day(item.date)): \(format.currencyMagnitude(item.uncovered, on: .register)) short of interest and escrow, not recorded."
	}

	/// FC-4's method detail and FC-3's non-cash assumption, shown on request
	/// beside the band label rather than as a caption.
	public static func method(_ band: ForecastBandMethod) -> [String] {
		let range: [String] = switch band {
		case .residuals: [
			"The range comes from how far past months' forecasts missed, from the 10th to the 90th percentile.",
			"It widens faster than it would if each month's miss were independent, because a miss partly repeats."
		]
		case .range: [
			"With fewer than 6 past forecasts to check, the range runs from the lowest to the highest month so far, each measured against the median month.",
			"It widens faster than it would if each month's miss were independent, because a miss partly repeats."
		]
		case .noHistory: []
		}
		let covers: [String] = band == .noHistory ? [] : [
			"It covers category baselines only. Bills and income schedules count at their expected amount, About and Between amounts included.",
			"Yearly costs such as gifts or insurance can fall outside it unless a yearly bill makes them known."
		]
		return range + covers + [
			"Revaluations, adjustments and starting balances are taken as zero.",
			"No market return or inflation is assumed unless a what-if adds it."
		]
	}

	/// "$30,100 to $33,900", the band at a horizon. `nil` with no history.
	public func band(_ horizon: ForecastHorizon) -> String? {
		guard let low = horizon.bandLow, let high = horizon.bandHigh else { return nil }
		return "\(format.wholeCurrency(low, on: .register)) to \(format.wholeCurrency(high, on: .register))"
	}
}
