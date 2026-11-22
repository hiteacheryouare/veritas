import CoreGraphics
import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// ST-10.10's words for the v14 cards, and the filter editor's choices.
struct CardWordsTests {
	@Test func groupAccountsNodeIsAllAccountsNeverIncome() {
		// Fred: with Group accounts on, the single account node is "All accounts".
		#expect(CardWords.sankey(.allAccounts) == "All accounts")
		#expect(CardWords.sankey(.hidden) == nil, "a spacer node is never drawn")
		#expect(CardWords.sankey(.name(name: "Gifts")) == "Gifts")
		#expect(CardWords.sankey(.other) == "Other")
		#expect(CardWords.sankey(.fromMonth(month: Month(year: 2026, month: 10))) == "From October 2026")
		#expect(CardWords.sankey(.forMonth(month: Month(year: 2026, month: 12))) == "For December 2026")
		#expect(CardWords.sankey(.fromCategory(category: "Groceries")) == "From Groceries")
		let words = [SankeyLabel.budgeted, .availableFunds, .toBudget, .overbudgeted, .allAccounts, .overspent, .other].compactMap(CardWords.sankey)
		#expect(words == ["Budgeted", "Available funds", "To budget", "Overbudgeted", "All accounts", "Overspent", "Other"])
		#expect(!words.contains { ["Income", "Expenses", "Spending", "Savings", "Debt paydown"].contains($0) })
	}

	@Test func monteCarloShowsTheCoresSuccessRateWithoutRoundingAgain() {
		#expect(CardWords.successRate(85) == "85%")
		#expect(CardWords.successRate(85.3) == "85.3%")
		#expect(CardWords.successRate(99.9) == "99.9%", "not rounded up to 100")
		#expect(CardWords.successRate(.nan) == "")
		#expect(CardWords.successRateLabel(endAge: 90) == "Success rate to age 90")
		#expect(CardWords.successRateLabel(endAge: 90.5) == "Success rate to age 90.5")
	}

	@Test func theMonteCarloCaptionCountsTheLifetimesActuallyRun() {
		let card = DashboardCard(
			id: "m", kind: .monteCarlo, rawKind: "monte-carlo-card", x: 0, y: 0, width: 4, height: 2, config: .empty,
			content: .monteCarlo(successPercent: 85, startAge: 45, endAge: 90, simulations: 5_000, chart: [])
		)
		let caption = CardWords.caption(card) ?? ""
		#expect(caption.hasPrefix("A projection, not a promise. It runs 5,000 simulated lifetimes"))
		#expect(caption.hasSuffix("the money lasts to age 90."))
		#expect(card.title == "Monte Carlo analysis")
	}

	@Test func balanceForecastFootersFollowActualsWordsAndST1010() {
		#expect(CardWords.forecastFooter(source: .schedules, scheduled: 3, filtered: false) == "3 scheduled transactions included")
		#expect(CardWords.forecastFooter(source: .schedules, scheduled: 1, filtered: false) == "1 scheduled transaction included")
		#expect(CardWords.forecastFooter(source: .schedules, scheduled: 2, filtered: true) == "2 scheduled transactions included (filtered running total)")
		#expect(CardWords.forecastFooter(source: .schedules, scheduled: 0, filtered: false) == "No scheduled transactions in this range")
		#expect(CardWords.forecastFooter(source: .schedules, scheduled: 0, filtered: true) == "Filtered running total only; no scheduled occurrences in this range")
		#expect(CardWords.forecastFooter(source: .trackingBudget, scheduled: 4, filtered: false) == "Budgeted income and expenses included")
		#expect(CardWords.endingBalance == "Ending balance" && CardWords.lowestBalance == "Lowest balance")
	}

	@Test func theForecastCaptionNamesTheRangesEnd() {
		let view = WidgetView.balanceForecast(
			range: WidgetRange(start: "2026-11-01", end: "2026-11-15", mode: "static"), source: .schedules, granularity: .daily,
			zeroBased: false, hasForecast: true, points: [], low: nil, scheduled: 0, filtered: false, today: nil
		)
		let card = DashboardCard(id: "b", kind: .balanceForecast, rawKind: "balance-forecast-card", x: 0, y: 0, width: 4, height: 2, config: .empty, content: view)
		#expect(card.title == "Balance with scheduled transactions")
		#expect(CardWords.caption(card)?.contains("through November 15, 2026.") == true)
	}

	@Test func theFormulaCardShowsWhatTheFormulaReturns() {
		let format = MoneyFormat(currencySymbol: "")
		#expect(CardWords.formula(.number(value: 1500.5), format: format) == ("1,500.50", false))
		#expect(CardWords.formula(.number(value: -0.004), format: format) == ("0.00", false), "half up, as Actual's amountToInteger rounds")
		#expect(CardWords.formula(.number(value: .infinity), format: format) == ("", false), "never NaN, infinity or 0")
		#expect(CardWords.formula(.text(text: "Hello"), format: format) == ("Hello", false))
		#expect(CardWords.formula(.boolean(value: true), format: format) == ("true", false))
		#expect(CardWords.formula(.empty, format: format) == ("", false))
		let error = FormulaError(kind: .divByZero, message: "Formula error: DIV_BY_ZERO")
		#expect(CardWords.formula(.error(error: error), format: format) == ("Formula error: DIV_BY_ZERO", true))
		let yen = MoneyFormat(currencySymbol: "", decimalPlaces: 0)
		#expect(CardWords.formula(.number(value: 33333), format: yen) == ("33,333", false), "a yen result shows at the currency's places")
	}

	@Test func netWorthAndSpendingLabelsChangeWithFilters() {
		#expect(CardWords.netWorth(filtered: false) == "Net worth")
		#expect(CardWords.netWorth(filtered: true) == "Balance of matching rows")
		#expect((CardWords.assets(filtered: false), CardWords.liabilities(filtered: false)) == ("Assets", "Liabilities"))
		#expect((CardWords.assets(filtered: true), CardWords.liabilities(filtered: true)) == ("Positive balances", "Negative balances"))
		#expect(CardWords.spending(filtered: false) == "Everything except income categories")
		#expect(CardWords.spending(filtered: true) == "Selected rows")
	}

	@Test func theSpendingComparisonSaysMoreLessOrSame() {
		// Fred on M8 (ST-10.10): a size, more or less, and what it is compared with.
		let format = MoneyFormat(currencySymbol: "")
		let october = Month(year: 2026, month: 10)
		func line(_ difference: Int64, _ mode: SpendingCardMode) -> String {
			CardWords.comparison(difference, mode: mode, compareTo: october, averageFirst: Month(year: 2026, month: 8), averageLast: october, format: format)
		}
		#expect(line(150_000 - 438_784, .singleMonth) == "2,887.84 less than October")
		#expect(line(12_500, .singleMonth) == "125.00 more than October")
		#expect(line(0, .singleMonth) == "Same as October")
		#expect(line(-2_500, .budget) == "25.00 less than budgeted so far")
		#expect(line(0, .budget) == "Same as budgeted so far")
		#expect(line(4_000, .average) == "40.00 more than the 3-month average")
		#expect(line(0, .average) == "Same as the 3-month average")
		for mode in [SpendingCardMode.singleMonth, .budget, .average] {
			for difference: Int64 in [-1, 0, 1] {
				let words = line(difference, mode).lowercased()
				for reserved in ["spent", "spending", "income", "expenses", "over budget", "under budget"] {
					#expect(!words.contains(reserved), "\(words)")
				}
			}
		}
	}
}

struct FilterChoicesTests {
	@Test func theEditorOffersActualsFilterFieldsWithoutIsBetween() {
		let fields = FilterChoices.all.map(\.field)
		#expect(fields == [.date, .account, .payee, .notes, .category, .categoryGroup, .amount, .amountInflow, .amountOutflow, .cleared, .reconciled, .transfer])
		#expect(!FilterChoices.all.flatMap(\.ops).contains { $0.op == .isBetween })
		#expect(FilterChoices.choice(for: .reconciled)?.ops.map(\.op) == [.is])
		#expect(FilterChoices.choice(for: .date)?.valueKind(for: .is) == .dateMonthOrYear)
		#expect(FilterChoices.choice(for: .payee)?.valueKind(for: .oneOf) == .ids)
	}

	@Test func changingTheOpKeepsAValueOfTheSameKind() {
		let notes = RuleCondition(field: .notes, op: .contains, value: .text(text: "rent"))
		#expect(FilterChoices.setting(.doesNotContain, of: notes, today: "2026-11-18").value == .text(text: "rent"))
		let payee = RuleCondition(field: .payee, op: .is, value: .id(id: "p1"))
		#expect(FilterChoices.setting(.oneOf, of: payee, today: "2026-11-18").value == .ids(ids: []))
		let month = RuleCondition(field: .date, op: .is, value: .month(month: "2026-10"))
		#expect(FilterChoices.setting(.gt, of: month, today: "2026-11-18").value == .date(date: "2026-11-18"), "only is takes a month")
		let moved = FilterChoices.setting(.amount, of: notes, today: "2026-11-18")
		#expect(moved.field == .amount && moved.value == .amount(amount: 0))
	}

	@Test func aSavedFilterOrARawValueStaysAsStored() {
		#expect(!FilterChoices.isEditable(RuleCondition(field: .saved, op: .is, value: .id(id: "f1"))))
		#expect(!FilterChoices.isEditable(RuleCondition(field: .date, op: .is, value: .raw(json: "{}"))))
		#expect(FilterChoices.isEditable(FilterChoices.starter))
		#expect(FilterWords.stored(RuleCondition(field: .saved, op: .is, value: .id(id: "f1"))) == "Saved filter")
	}
}

struct SankeyCardLayoutTests {
	static func node(_ key: String, _ label: SankeyLabel = .other) -> SankeyCardNode {
		SankeyCardNode(key: key, label: label, percentage: "", color: .gray)
	}

	@Test func sourcesStartLeftSinksEndRightAndSpacersAreNeverDrawn() {
		let nodes = [Self.node("payee"), Self.node("account", .allAccounts), Self.node("food"), Self.node("rent"), Self.node("spacer", .hidden)]
		let links = [
			SankeyCardLink(source: 0, target: 1, value: 300, folded: [], color: nil),
			SankeyCardLink(source: 1, target: 2, value: 100, folded: [], color: nil),
			SankeyCardLink(source: 1, target: 3, value: 200, folded: [], color: nil),
			SankeyCardLink(source: 1, target: 4, value: -1, folded: [], color: nil),
			SankeyCardLink(source: 2, target: nil, value: 50, folded: [], color: nil)
		]
		let layout = SankeyCardLayout(nodes: nodes, links: links, size: CGSize(width: 300, height: 200))
		#expect(layout.nodes.map(\.index) == [0, 1, 2, 3])
		#expect(layout.columns == 3)
		func rect(_ index: Int) -> CGRect { layout.nodes.first { $0.index == index }!.rect }
		#expect(rect(0).minX == 0 && rect(2).maxX == 300 && rect(3).maxX == 300)
		#expect(abs(rect(1).height - (rect(2).height + rect(3).height)) < 0.5, "a node is as tall as what flows through it")
		#expect(layout.ribbons.map(\.link) == [0, 1, 2], "no ribbon into a spacer or past the layer range")
		#expect(layout.nodes.allSatisfy { $0.rect.maxY <= 200 })
	}
}
