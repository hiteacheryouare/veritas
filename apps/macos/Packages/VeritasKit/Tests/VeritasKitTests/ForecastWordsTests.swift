import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

struct ForecastWordsTests {
	static let names = BudgetNames(
		payees: [],
		accounts: [AccountDetail(id: "checking", name: "Checking", offBudget: false, closed: false, groupId: nil, cleared: 0, uncleared: 0, total: 0, note: nil, lastReconciled: nil)],
		categoryGroups: [
			EnvelopeGroup(id: "g", name: "Everyday", isIncome: false, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
				EnvelopeRow(id: "groceries", name: "Groceries", hidden: false, budgeted: 0, spent: 0, available: 0, carryover: false, tone: .neutral, fill: 0, note: nil)
			]),
			EnvelopeGroup(id: "i", name: "Income", isIncome: true, hidden: false, budgeted: 0, spent: 0, available: 0, envelopes: [
				EnvelopeRow(id: "salary", name: "Salary", hidden: false, budgeted: 0, spent: 0, available: 0, carryover: false, tone: .neutral, fill: 0, note: nil)
			])
		]
	)

	let words = ForecastWords(names: Self.names, goals: [], format: MoneyFormat())
	let june = Month(year: 2027, month: 6)
	let december = Month(year: 2027, month: 12)

	@Test func eachKindReadsAsOneLine() {
		#expect(words.sentence(.oneTime(date: "2027-03-10", accountId: "checking", categoryId: "groceries", amount: -200_000))
			== "Groceries: \u{2212}$2,000.00 on March 10, 2027, Checking")
		#expect(words.sentence(.oneTime(date: "2027-03-10", accountId: "checking", categoryId: nil, amount: 50_000))
			== "+$500.00 on March 10, 2027, Checking")
		#expect(words.sentence(.recurring(start: "2027-06-01", endMonth: december, cadence: .everyMonths(count: 1), accountId: "checking", categoryId: "groceries", amount: -30_000))
			== "Groceries: \u{2212}$300.00 every month from June 1, 2027 through December 2027, Checking")
		#expect(words.sentence(.recurring(start: "2027-06-01", endMonth: nil, cadence: .everyWeeks(count: 2), accountId: "checking", categoryId: nil, amount: 120_000))
			== "+$1,200.00 every 2 weeks from June 1, 2027, Checking")
		#expect(words.sentence(.categoryPercent(categoryId: "groceries", percentBp: -1_000, startMonth: june, endMonth: nil))
			== "Groceries: \u{2212}10% from June 2027")
		#expect(words.sentence(.incomeChange(accountId: "checking", categoryId: nil, change: .percent(percentBp: 250), startMonth: june, endMonth: december))
			== "All income: +2.5% from June 2027 through December 2027, Checking")
		#expect(words.sentence(.incomeChange(accountId: "checking", categoryId: "salary", change: .amount(amount: 50_000), startMonth: june, endMonth: nil))
			== "Salary: +$500.00 a month from June 2027, Checking")
	}

	@MainActor @Test func aGoalContributionReadsOnTheBudgetScale() {
		var goal = BudgetScreenTests.goal("laptop", state: .active, available: 0, target: 150_000)
		goal.name = "New laptop"
		let yen = MoneyFormat(currencySymbol: "¥", decimalPlaces: 0, hidesDecimals: true)
		let words = ForecastWords(names: Self.names, goals: [goal], format: yen)
		#expect(words.sentence(.goalContribution(goalId: "laptop", amount: 25_000, startMonth: june, endMonth: nil)) == "New laptop: +¥25,000 a month from June 2027")
		#expect(words.sentence(.oneTime(date: "2027-03-10", accountId: "checking", categoryId: nil, amount: -50_000)) == "\u{2212}¥500 on March 10, 2027, Checking", "a transaction amount stays on the register scale")
	}

	@Test func somethingNoLongerInTheBudgetSaysSo() {
		#expect(words.sentence(.oneTime(date: "2027-03-10", accountId: "gone", categoryId: "gone", amount: -100)) == "Missing category: \u{2212}$1.00 on March 10, 2027, Missing account")
		#expect(words.sentence(.goalContribution(goalId: "gone", amount: 100, startMonth: june, endMonth: nil)) == "Missing goal: +$1.00 a month from June 2027")
	}

	@Test func aFlowReadsFromItsAccountAndIncomeReadsTheOtherWay() {
		let accounts = [ForecastAccount(accountId: "loan", name: "Car loan", offBudget: true, balance: 0)]
		func flow(_ kind: ForecastFlowKind, category: String?, counterpart: String? = nil) -> ForecastFlow {
			ForecastFlow(date: "2026-12-01", dueNow: false, accountId: "checking", accountName: "Checking", amount: 0, kind: kind, counterpartAccountId: counterpart, categoryId: nil, categoryName: category, billId: nil, name: "", certainty: .known)
		}
		#expect(words.route(flow(.expense, category: "Rent"), accounts: accounts) == "Checking → Rent")
		#expect(words.route(flow(.income, category: "Salary"), accounts: accounts) == "Salary → Checking")
		#expect(words.route(flow(.transfer, category: nil, counterpart: "loan"), accounts: accounts) == "Checking → Car loan")
		#expect(words.route(flow(.transfer, category: nil), accounts: accounts) == "Checking")
	}

	@Test func theBandIsWholeUnitsAndAbsentWithNoHistory() {
		let banded = ForecastHorizon(months: 3, month: Month(year: 2027, month: 2), netWorth: 3_000_000, bandLow: 2_909_049, bandHigh: 3_048_000, accounts: [])
		#expect(words.band(banded) == "$29,090 to $30,480")
		let none = ForecastHorizon(months: 3, month: Month(year: 2027, month: 2), netWorth: 3_000_000, bandLow: nil, bandHigh: nil, accounts: [])
		#expect(words.band(none) == nil)
	}

	@Test func aShortfallNamesTheLoanTheDateAndTheAmount() {
		let item = ForecastShortfall(date: "2027-01-01", loanAccountId: "loan", loanName: "Mortgage", uncovered: 2_000)
		#expect(words.shortfall(item) == "Mortgage, January 1, 2027: $20.00 short of interest and escrow, not recorded.")
	}

	@Test func theMethodNotesFollowTheBandMethod() {
		let residuals = ForecastWords.method(.residuals)
		#expect(residuals.first?.contains("10th to the 90th percentile") == true)
		#expect(ForecastWords.method(.range).first?.hasPrefix("With fewer than 6 past forecasts") == true)
		#expect(ForecastWords.method(.range).first?.contains("lowest to the highest month so far, each measured against the median month") == true, "FC-4.3")
		let assumptions = [
			"Revaluations, adjustments and starting balances are taken as zero.",
			"No market return or inflation is assumed unless a what-if adds it."
		]
		#expect(ForecastWords.method(.noHistory) == assumptions, "no range, so nothing about one")
		for band in [ForecastBandMethod.residuals, .range] {
			#expect(ForecastWords.method(band).contains { $0.hasPrefix("It covers category baselines only") })
			#expect(Array(ForecastWords.method(band).suffix(2)) == assumptions)
		}
	}

	@Test func aPercentShowsInTheBudgetsDecimalSeparator() {
		#expect(words.percent(1_000) == "+10%")
		#expect(words.percent(-250) == "\u{2212}2.5%")
		#expect(ForecastWords(format: MoneyFormat(groupingSeparator: ".", decimalSeparator: ",")).percent(1_234) == "+12,34%")
		#expect(ForecastWords.percentDigits(5, decimalSeparator: ".") == "0.05")
		#expect(ForecastWords.percentDigits(-250, decimalSeparator: ",") == "2,5")
	}
}

struct ScenarioDraftTests {
	let today = "2026-11-19"

	@Test func everyKindRoundTripsThroughTheEditor() throws {
		let june = Month(year: 2027, month: 6)
		let changes: [ScenarioChange] = [
			.oneTime(date: "2027-03-10", accountId: "checking", categoryId: "groceries", amount: -200_000),
			.recurring(start: "2027-06-01", endMonth: Month(year: 2027, month: 12), cadence: .everyWeeks(count: 2), accountId: "checking", categoryId: nil, amount: 120_000),
			.categoryPercent(categoryId: "groceries", percentBp: -1_050, startMonth: june, endMonth: nil),
			.incomeChange(accountId: "checking", categoryId: nil, change: .percent(percentBp: 250), startMonth: june, endMonth: nil),
			.incomeChange(accountId: "checking", categoryId: "salary", change: .amount(amount: -50_000), startMonth: june, endMonth: june),
			.goalContribution(goalId: "laptop", amount: 25_000, startMonth: june, endMonth: nil)
		]
		let percents: [Int64?] = [nil, nil, 1_050, 250, nil, nil]
		for (change, percent) in zip(changes, percents) {
			let draft = ScenarioDraft(change, today: today)
			#expect(try draft.change(magnitude: draft.magnitude, percent: percent) == change, "\(change)")
		}
		#expect(ScenarioDraft(changes[2], today: today, decimalSeparator: ",").percentText == "10,5", "a percent opens in the budget's separator")
	}

	@Test func aMissingChoiceSaysWhatToChoose() throws {
		var draft = ScenarioDraft(today: today)
		#expect(throws: ScenarioDraft.Problem(message: "Choose an account.")) { try draft.change(magnitude: 100) }
		draft.accountID = "checking"
		#expect(try draft.change(magnitude: 100) == .oneTime(date: today, accountId: "checking", categoryId: nil, amount: -100), "money out by default")
		draft.kind = .categoryPercent
		#expect(throws: ScenarioDraft.Problem(message: "Choose a category.")) { try draft.change(magnitude: 0) }
		draft.categoryID = "groceries"
		#expect(throws: ScenarioDraft.Problem(message: "Enter a percent, such as 10 or 2.5.")) { try draft.change(magnitude: 0) }
		draft.kind = .goalContribution
		#expect(throws: ScenarioDraft.Problem(message: "Choose a goal.")) { try draft.change(magnitude: 100) }
	}

	@Test func aPercentOrAGoalNeedsNoAmountFieldOrDirectionForTheGoal() throws {
		var draft = ScenarioDraft(today: today)
		draft.kind = .incomeChange
		#expect(draft.usesAmountField)
		draft.incomeByPercent = true
		#expect(!draft.usesAmountField)
		draft.kind = .goalContribution
		draft.goalID = "laptop"
		#expect(draft.kind.scale == .budget)
		#expect(try draft.change(magnitude: 2_500) == .goalContribution(goalId: "laptop", amount: 2_500, startMonth: Month(year: 2026, month: 11), endMonth: nil), "a contribution adds, whatever the direction left from another kind")
	}
}
