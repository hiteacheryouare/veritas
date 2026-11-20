#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The stub budget's forecast, suggestions and what-ifs, in memory. The
/// baseline grows by a fixed amount a month from whatever today is, and a
/// what-if moves it by its events' amounts, so a UI test sees a compare
/// change. The figures are made up; only the core computes real ones.
final class StubForecast: @unchecked Sendable {
	static let netWorthToday: Int64 = 2_418_000
	static let monthlyGrowth: Int64 = 64_000
	/// The 80% band's offsets at 3, 6, 10 and 12 months.
	static let bands: [UInt32: (low: Int64, high: Int64)] = [3: (-91_000, 48_000), 6: (-153_000, 81_000), 10: (-225_000, 119_000), 12: (-257_000, 136_000)]
	static let bandLabel = "80% band from 14 past months"

	private let lock = NSLock()
	private var scenarioList: [ScenarioSummary] = []
	private var made = 0

	// MARK: What-ifs

	func scenarios(today: String) -> [ScenarioSummary] {
		lock.withLock {
			scenarioList.map { scenario in
				ScenarioSummary(id: scenario.id, name: scenario.name, events: scenario.events.map {
					ScenarioEventItem(id: $0.id, change: $0.change, inThePast: Self.isInThePast($0.change, today: today))
				})
			}
		}
	}

	func create(named name: String, firstEvent: ScenarioChange?, today: String) throws -> String {
		try lock.withLock {
			let trimmed = name.trimmingCharacters(in: .whitespaces)
			guard !trimmed.isEmpty else { throw FfiError.Invalid(message: "Give the what-if a name.") }
			guard !scenarioList.contains(where: { $0.name.lowercased() == trimmed.lowercased() }) else {
				throw FfiError.Invalid(message: "Another what-if is already called \(trimmed). Pick another name.")
			}
			if let firstEvent { try Self.check(firstEvent, today: today) }
			made += 1
			let id = "what-if-\(made)"
			var events: [ScenarioEventItem] = []
			if let firstEvent {
				made += 1
				events.append(ScenarioEventItem(id: "event-\(made)", change: firstEvent, inThePast: false))
			}
			scenarioList.append(ScenarioSummary(id: id, name: trimmed, events: events))
			return id
		}
	}

	/// True when it handled the command.
	func perform(_ command: Command) throws -> Bool {
		try lock.withLock {
			switch command {
			case .renameScenario(let id, let name):
				try update(id) { $0.name = name }
			case .deleteScenario(let id):
				scenarioList.removeAll { $0.id == id }
			case .addScenarioEvent(let id, let change, let today):
				try Self.check(change, today: today)
				made += 1
				let event = ScenarioEventItem(id: "event-\(made)", change: change, inThePast: false)
				try update(id) { $0.events.append(event) }
			case .editScenarioEvent(let id, let eventID, let change, let today):
				try Self.check(change, today: today)
				try update(id) { scenario in
					guard let index = scenario.events.firstIndex(where: { $0.id == eventID }) else { throw FfiError.NotFound(entity: "scenario_event", id: eventID) }
					scenario.events[index].change = change
				}
			case .removeScenarioEvent(let id, let eventID):
				try update(id) { $0.events.removeAll { $0.id == eventID } }
			default:
				return false
			}
			return true
		}
	}

	private func update(_ id: String, _ change: (inout ScenarioSummary) throws -> Void) throws {
		guard let index = scenarioList.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "scenario", id: id) }
		try change(&scenarioList[index])
	}

	private static func check(_ change: ScenarioChange, today: String) throws {
		switch change {
		case .oneTime(let date, _, _, _), .recurring(let date, _, _, _, _, _):
			if date < today { throw FfiError.Invalid(message: "Pick today or a later date.") }
		default:
			break
		}
	}

	private static func isInThePast(_ change: ScenarioChange, today: String) -> Bool {
		guard case .oneTime(let date, _, _, _) = change else { return false }
		return date < today
	}

	// MARK: Forecast

	func forecast(today: String, scenarioIDs: [String]) throws -> Forecast {
		let thisMonth = Month(date: today) ?? StubBudget.month
		let scenarios = try scenarioIDs.map { id in
			guard let scenario = lock.withLock({ scenarioList.first { $0.id == id } }) else { throw FfiError.NotFound(entity: "scenario", id: id) }
			return scenario
		}
		let baseline = Self.projection(thisMonth: thisMonth, scenario: nil, today: today)
		return Forecast(
			today: today,
			netWorthToday: Self.netWorthToday,
			accountsToday: Self.accounts(growth: 0),
			baseline: baseline,
			scenarios: scenarios.map { Self.projection(thisMonth: thisMonth, scenario: $0, today: today) },
			bandMethod: .residuals,
			bandLabel: Self.bandLabel,
			baselines: [
				ForecastBaseline(categoryId: "paycheck", name: "Paycheck", isIncome: true, level: 512_640, label: "6-month median", seasonal: false),
				ForecastBaseline(categoryId: "groceries", name: "Groceries", isIncome: false, level: 43_000, label: "6-month median", seasonal: false),
				ForecastBaseline(categoryId: "dining", name: "Dining out", isIncome: false, level: 19_850, label: "6-month median", seasonal: false),
				ForecastBaseline(categoryId: "gas", name: "Gas", isIncome: false, level: 11_200, label: "6-month median", seasonal: true),
				ForecastBaseline(categoryId: "gifts", name: "Gifts", isIncome: false, level: 0, label: "No history yet", seasonal: false)
			],
			knownFlows: Self.flows(thisMonth: thisMonth, today: today),
			loans: [LoanForecast(accountId: "carloan", name: "Car loan", estimatedFromAmounts: true, interestAssumedZero: false, notes: ["Estimated from the amounts", "Not repeated: Fees $25.00"])],
			unforecast: [UnforecastItem(billId: "gym", loanAccountId: nil, name: "Gym", reason: "its rule matches any condition, not all of them")],
			shortfalls: [],
			possibleDoubleCounts: [ForecastCategory(categoryId: "internet", name: "Internet")],
			notForecastNote: "Not forecast: $42 a month in uncategorized or hidden spending",
			assetPurchaseNote: nil
		)
	}

	private static func accounts(growth: Int64) -> [ForecastAccount] {
		[
			ForecastAccount(accountId: "checking", name: "Checking", offBudget: false, balance: 412_000 + growth),
			ForecastAccount(accountId: "visa", name: "Visa", offBudget: false, balance: -61_340),
			ForecastAccount(accountId: "savings", name: "Savings", offBudget: false, balance: 3_271_340),
			ForecastAccount(accountId: "carloan", name: "Car loan", offBudget: true, balance: -1_204_000)
		]
	}

	private static func projection(thisMonth: Month, scenario: ScenarioSummary?, today: String) -> ForecastProjection {
		let live = scenario?.events.filter { !isInThePast($0.change, today: today) } ?? []
		var netWorth = netWorthToday
		var months: [ForecastMonth] = []
		for offset in 0...12 {
			let month = thisMonth.adding(months: offset)
			let known: Int64 = offset == 0 ? 20_000 : 52_000
			let baselined: Int64 = offset == 0 ? -6_000 : 12_000
			let extra = live.reduce(Int64(0)) { $0 + effect(of: $1.change, in: month) }
			let net = known + baselined + extra
			netWorth += net
			months.append(ForecastMonth(month: month, knownNetIncome: known, baselinedNetFlow: baselined, scenarioNetIncome: extra, netIncome: net, netWorth: netWorth))
		}
		let horizons = ForecastModel.horizons.map { count in
			let point = months[Int(count)]
			let band = bands[count]
			return ForecastHorizon(
				months: count, month: point.month, netWorth: point.netWorth,
				bandLow: band.map { point.netWorth + $0.low }, bandHigh: band.map { point.netWorth + $0.high },
				accounts: accounts(growth: point.netWorth - netWorthToday)
			)
		}
		return ForecastProjection(
			scenarioId: scenario?.id, name: scenario?.name ?? "", months: months, horizons: horizons,
			eventsInThePast: scenario?.events.filter { isInThePast($0.change, today: today) }.map(\.id) ?? [],
			goalEtas: goalEtas(thisMonth: thisMonth, contributing: live.contains { if case .goalContribution = $0.change { true } else { false } })
		)
	}

	private static func effect(of change: ScenarioChange, in month: Month) -> Int64 {
		switch change {
		case .oneTime(let date, _, _, let amount):
			Month(date: date) == month ? amount : 0
		case .recurring(let start, let end, let cadence, _, _, let amount):
			Month(date: start).map { first in
				guard first <= month, end.map({ month <= $0 }) ?? true else { return 0 }
				switch cadence {
				case .everyMonths(let count): return first.months(through: month) % Int(max(count, 1)) == 0 ? amount : 0
				case .everyWeeks(let count): return amount * Int64(4 / max(count, 1))
				}
			} ?? 0
		case .categoryPercent(_, let percentBp, let start, let end):
			active(month, start, end) ? -43_000 * percentBp / 10_000 : 0
		case .incomeChange(_, _, let change, let start, let end):
			switch change {
			case .amount(let amount): active(month, start, end) ? amount : 0
			case .percent(let percentBp): active(month, start, end) ? 512_640 * percentBp / 10_000 : 0
			}
		case .goalContribution:
			0
		}
	}

	private static func active(_ month: Month, _ start: Month, _ end: Month?) -> Bool {
		start <= month && (end.map { month <= $0 } ?? true)
	}

	private static func goalEtas(thisMonth: Month, contributing: Bool) -> [GoalEtaItem] {
		let laptop = contributing ? thisMonth.adding(months: 2) : thisMonth.adding(months: 4)
		let japan = thisMonth.adding(months: 14)
		return [
			GoalEtaItem(
				goalId: "laptop", name: "New laptop", status: .eta(month: laptop, optimistic: laptop, pessimistic: laptop.adding(months: 2)),
				remaining: 38_000, pace: 9_500, optimisticPace: 12_000, pessimisticPace: 7_600, historyMonths: 3, historyLabel: "3 months of history",
				onTrack: true, text: "ETA \(ReportPeriod.monthYear(laptop)), range \(ReportPeriod.monthYear(laptop)) to \(ReportPeriod.monthYear(laptop.adding(months: 2)))"
			),
			GoalEtaItem(
				goalId: "japan", name: "Japan trip", status: .eta(month: japan, optimistic: thisMonth.adding(months: 11), pessimistic: nil),
				remaining: 369_000, pace: 25_000, optimisticPace: 41_000, pessimisticPace: 0, historyMonths: 3, historyLabel: "3 months of history",
				onTrack: false, text: "ETA \(ReportPeriod.monthYear(japan)), no ETA at the slowest pace"
			)
		]
	}

	private static func flows(thisMonth: Month, today: String) -> [ForecastFlow] {
		let next = thisMonth.adding(months: 1)
		func day(_ month: Month, _ day: Int) -> String { String(format: "%04d-%02d-%02d", month.year, month.month, day) }
		return [
			ForecastFlow(date: day(next, 1), dueNow: false, accountId: "checking", accountName: "Checking", amount: -165_000, kind: .expense, counterpartAccountId: nil, categoryId: "rent", categoryName: "Rent", billId: "rent", name: "Rent", certainty: .known),
			ForecastFlow(date: day(next, 1), dueNow: false, accountId: "checking", accountName: "Checking", amount: 256_320, kind: .income, counterpartAccountId: nil, categoryId: "paycheck", categoryName: "Paycheck", billId: "pay", name: "Paycheck", certainty: .known),
			ForecastFlow(date: day(next, 12), dueNow: false, accountId: "checking", accountName: "Checking", amount: -9_050, kind: .expense, counterpartAccountId: nil, categoryId: "electric", categoryName: "Electric", billId: "electric", name: "Electric", certainty: .estimated),
			ForecastFlow(date: day(next, 15), dueNow: false, accountId: "checking", accountName: "Checking", amount: -28_000, kind: .transfer, counterpartAccountId: "carloan", categoryId: nil, categoryName: nil, billId: "car", name: "Car loan", certainty: .estimated)
		]
	}

	// MARK: Suggestions

	func suggestions(today: String) -> SuggestionList {
		let next = (Month(date: today) ?? StubBudget.month).adding(months: 1)
		return SuggestionList(
			month: next,
			items: [
				BudgetSuggestion(categoryId: "groceries", name: "Groceries", amount: 43_000, basis: .baseline, reason: "Groceries $430, 6-month median"),
				BudgetSuggestion(categoryId: "dining", name: "Dining out", amount: 19_900, basis: .baseline, reason: "Dining out $199, 6-month median"),
				BudgetSuggestion(categoryId: "gas", name: "Gas", amount: 12_300, basis: .seasonal, reason: "Gas $123, 6-month median, adjusted +10% for \(Self.monthName(next))"),
				BudgetSuggestion(categoryId: "rent", name: "Rent", amount: 165_000, basis: .bills, reason: "Rent $1,650, scheduled: Rent"),
				BudgetSuggestion(categoryId: "electric", name: "Electric", amount: 9_100, basis: .billsAndBaseline, reason: "Electric $91: $91 scheduled + $0 6-month median"),
				BudgetSuggestion(categoryId: "gifts", name: "Gifts", amount: nil, basis: .noHistory, reason: "Gifts: no history yet, keeping $40")
			],
			expectedIncome: 512_640,
			exceedsIncomeBy: nil,
			notice: nil
		)
	}

	private static func monthName(_ month: Month) -> String {
		month.date()?.formatted(.dateTime.month(.wide)) ?? ""
	}
}
#endif
