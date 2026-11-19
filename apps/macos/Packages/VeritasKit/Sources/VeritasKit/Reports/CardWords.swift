import Foundation
import VeritasCore

/// The words on dashboard cards (docs/finance/statements.md ST-10.10). The
/// core sends data only; these name it. Captions live in each card's info
/// popover, never under the title.
public enum CardWords {
	public static func caption(_ card: DashboardCard) -> String? {
		switch card.content {
		case .netWorth(let range, let interval, _, _, _, _, _, _, _, _):
			if card.isFiltered { return "Only rows that match the card's filters. Not your whole net worth." }
			let through = switch interval {
			case .daily, .weekly: "today"
			case .monthly, .yearly: "the end of \(Month(date: range.end).map(ReportPeriod.monthYear) ?? range.end)"
			}
			return "Every account's balance, assets minus liabilities. Includes closed accounts and everything dated through \(through). A transfer counts on the date of its later side."
		case .sankey(_, _, _, .budgeted, _, _, _):
			return "Budgeted amounts by category and month, with what was left or overspent."
		case .sankey:
			return "Money moving through your categories. Only rows with a category are shown. Savings and debt categories appear like any other category. Transfers between budget accounts appear only when Show transfers is on. Not the Sankey report."
		case .balanceForecast(_, .trackingBudget, _, _, _, _, _, _, _, _):
			return "Your balance plus budgeted income and expenses. Amounts that aren't budgeted are not included."
		case .balanceForecast(let range, .schedules, _, _, _, _, _, _, _, _):
			return "Your balance from the start of the range, with every transaction entered in the range and scheduled transactions from today on, through \(day(range.end)). Spending and income that aren't entered or scheduled are not included, so the real balance will differ. Not Veritas's forecast."
		case .monteCarlo(_, _, let endAge, let simulations, _):
			let runs = Int(simulations).formatted(.number.grouping(.automatic))
			let age = number(endAge)
			return "A projection, not a promise. It runs \(runs) simulated lifetimes from a fixed starting point, so the same settings give the same answer. Each year's return comes from the model you choose: random draws, random historical years, blocks of consecutive historical years, or replays of actual history. With random draws, inflation averages 2.5% a year with 2% variation unless you change it. With the historical models each simulated year uses that year's actual inflation. Inflation can be turned off, and then spending stays flat. The success rate is the share of simulated runs in which the money lasts to age \(age)."
		default:
			return caption(card.kind)
		}
	}

	public static func caption(_ kind: WidgetKind?) -> String? {
		switch kind {
		case .calendar: "All money in and out of every account. Both sides of transfers and card payments count. Includes refunds. Not Net Profit."
		case .ageOfMoney: "How old your money is when you spend it. Every deposit counts as new money, and every payment or move out of your budget accounts counts as spending."
		case .crossover: "Every category outside the income group, including categories Veritas treats as savings or debt paydown. Not Net Profit's Expenses."
		case .cashFlow: "Deposits and payments in your budget accounts, up to today. Leaves out transfers between accounts, including card payments and moves to savings or loans. Includes refunds, starting balances and reconciliation adjustments."
		case .spending: "Every row in your budget accounts that is not in an income category, including rows with no category and transfers out of your budget. Refunds reduce it. Not Net Profit's Expenses."
		default: nil
		}
	}

	// MARK: Net worth

	public static func netWorth(filtered: Bool) -> String {
		filtered ? "Balance of matching rows" : "Net worth"
	}

	public static func assets(filtered: Bool) -> String {
		filtered ? "Positive balances" : "Assets"
	}

	public static func liabilities(filtered: Bool) -> String {
		filtered ? "Negative balances" : "Liabilities"
	}

	// MARK: Spending

	public static func spending(filtered: Bool) -> String {
		filtered ? "Selected rows" : "Everything except income categories"
	}

	// MARK: Sankey

	/// A node's words, or `nil` for a hidden spacer node, which is never drawn.
	public static func sankey(_ label: SankeyLabel) -> String? {
		switch label {
		case .name(let name): name
		case .budgeted: "Budgeted"
		case .availableFunds: "Available funds"
		case .fromCategory(let category): "From \(category)"
		case .toBudget: "To budget"
		case .overbudgeted: "Overbudgeted"
		case .allAccounts: "All accounts"
		case .fromMonth(let month): "From \(ReportPeriod.monthYear(month))"
		case .forMonth(let month): "For \(ReportPeriod.monthYear(month))"
		case .overspent: "Overspent"
		case .other: "Other"
		case .hidden: nil
		}
	}

	public static func sankeyMode(_ mode: SankeyCardMode) -> String {
		switch mode {
		case .categorized: "Categorized"
		case .budgeted: "Budgeted"
		}
	}

	// MARK: Balance forecast

	public static let endingBalance = "Ending balance"
	public static let lowestBalance = "Lowest balance"
	public static let noForecast = "No transactions are included in this card. Adjust your filters, accounts or date range to see a projection."

	/// The line under the chart, worded as Actual words it except for the
	/// tracking budget's, which ST-10.10 replaces.
	public static func forecastFooter(source: ForecastCardSource, scheduled: UInt32, filtered: Bool) -> String {
		if source == .trackingBudget { return "Budgeted income and expenses included" }
		if scheduled == 0 {
			return filtered ? "Filtered running total only; no scheduled occurrences in this range" : "No scheduled transactions in this range"
		}
		let count = scheduled == 1 ? "1 scheduled transaction included" : "\(Int(scheduled).formatted(.number.grouping(.automatic))) scheduled transactions included"
		return filtered ? "\(count) (filtered running total)" : count
	}

	// MARK: Formula

	/// What the formula card shows: the result, or Actual's own error text.
	/// A number shows at the currency's places, as Actual's card does; a NaN
	/// or infinite one shows nothing.
	public static func formula(_ value: FormulaValue, format: MoneyFormat) -> (text: String, isError: Bool) {
		switch value {
		case .number(let number): (format.number(number) ?? "", false)
		case .text(let text): (text, false)
		case .boolean(let flag): (flag ? "true" : "false", false)
		case .empty: ("", false)
		case .error(let error): (error.message, true)
		}
	}

	// MARK: Monte Carlo

	/// "85%" or "85.3%": the core's figure, already to one decimal, as Actual
	/// writes a number into text, without rounding it again.
	public static func successRate(_ percent: Double) -> String {
		percent.isFinite ? "\(number(percent))%" : ""
	}

	public static func successRateLabel(endAge: Double) -> String {
		"Success rate to age \(number(endAge))"
	}

	/// A number as JavaScript writes it into a string: no trailing `.0`.
	static func number(_ value: Double) -> String {
		guard value.isFinite else { return "" }
		if value == value.rounded(), abs(value) < 1e15 { return String(Int64(value)) }
		return "\(value)"
	}

	/// "November 30, 2026" for a day, "November 2026" for a month.
	public static func day(_ text: String) -> String {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		if parts.count == 3, let date = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) {
			return date.formatted(.dateTime.month(.wide).day().year())
		}
		return Month(date: text).map(ReportPeriod.monthYear) ?? text
	}
}
