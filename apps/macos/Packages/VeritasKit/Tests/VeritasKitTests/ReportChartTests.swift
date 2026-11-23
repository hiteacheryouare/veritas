import CoreGraphics
import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

struct ReportSeriesTests {
	let balances = CustomReportBalances(totalAssets: 0, totalDebts: 0, netAssets: 0, netDebts: 0, totalTotals: 0)

	private func group(_ id: String, _ name: String, _ amounts: [Int64], uncategorized: UncategorizedKind? = nil) -> CustomReportGroup {
		CustomReportGroup(
			id: id, name: name, uncategorized: uncategorized, amount: amounts.reduce(0, +), balances: balances,
			intervals: amounts.enumerated().map { index, amount in
				CustomReportCell(interval: "2026-0\(index + 1)", start: "", end: "", amount: amount, balances: balances, change: 0)
			}
		)
	}

	/// Ryan's report: the stacked amounts hold every category, the empty and
	/// hidden ones at zero, keyed by id. The graph draws the groups only,
	/// under the core's names, so no id is ever a series name.
	@Test func theGraphDrawsTheGroupsUnderTheCoresNamesAndNeverAStackedKey() {
		let groups = [group("rent-id", "Rent", [-1000, -1000]), group("", "Transfers", [-200, 0], uncategorized: .transfers)]
		let empty = "5b9c3f1e-0000-4000-8000-000000000000"
		let intervals = (0..<2).map { index in
			CustomReportInterval(
				interval: CustomReportCell(interval: "2026-0\(index + 1)", start: "", end: "", amount: 0, balances: balances, change: 0),
				stacked: [StackedAmount(key: "rent-id", amount: 1000), StackedAmount(key: empty, amount: 0), StackedAmount(key: "Transfers", amount: index == 0 ? 200 : 0)]
			)
		}
		let data = CustomReportData(start: "2026-01-01", end: "2026-02-28", groups: groups, intervals: intervals, total: -2200, totals: balances)
		let series = ReportSeries.all(data, balance: .payment)
		#expect(series.map(\.name) == ["Rent", "Transfers"])
		#expect(series.map(\.id) == ["rent-id", "Transfers"], "Actual's dataKey: the id, or the name for an Uncategorized entry")
		#expect(!series.contains { $0.id == empty || $0.name == empty })
		#expect(series[0].points.map(\.amount) == [1000, 1000], "payments draw upward, as the stacked amounts are")
		#expect(series[1].total == 200)
		#expect(series.map(\.color) == [0, 1])
	}

	@Test func colorsCycleThroughActualsNinePlacesAndNetsKeepTheirSign() {
		let groups = (0..<11).map { group("g\($0)", "Group \($0)", [Int64($0) - 5]) }
		let data = CustomReportData(start: "", end: "", groups: groups, intervals: [], total: 0, totals: balances)
		let series = ReportSeries.all(data, balance: .net)
		#expect(series.map(\.color) == [0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 1])
		#expect(series[0].total == -5)
		#expect(ReportSeries.drawnUpward(.netPayment))
		#expect(!ReportSeries.drawnUpward(.deposit))
	}
}

struct ZeroSplitTests {
	@Test func theToneTurnsWhereTheValueCrossesZero() {
		#expect(ZeroSplit.stop(low: 100, high: 300) == 1, "all positive")
		#expect(ZeroSplit.stop(low: -300, high: -100) == 0, "all debt")
		#expect(ZeroSplit.stop(low: -100, high: 300) == 0.75)
		#expect(ZeroSplit.lineStop([300, -100, 50]) == 0.75)
	}

	@Test func anAreaFillsToZeroWhenZeroIsInView() {
		let domain = ZeroSplit.domain([-100, 300], includesZero: false)
		#expect(domain.lowerBound < -100 && domain.upperBound > 300)
		#expect(ZeroSplit.baseline(in: domain) == 0)
		#expect(ZeroSplit.areaStop([-100, 300], baseline: 0) == 0.75)
		let high = ZeroSplit.domain([900, 1000], includesZero: false)
		#expect(ZeroSplit.baseline(in: high) == high.lowerBound, "an all-positive trend fills to the bottom of the plot")
		#expect(ZeroSplit.areaStop([900, 1000], baseline: high.lowerBound) == 1)
		#expect(ZeroSplit.domain([0, 0], includesZero: true) == 0...1)
		#expect(ZeroSplit.domain([0, 50], includesZero: true).lowerBound == 0)
	}
}

struct SpendingPointTests {
	@Test func runningTotalsRiseFromZeroAndTheLastDayIs28Plus() {
		let days: [SpendingCardDay] = (1...28).map { (day: Int) -> SpendingCardDay in
			let amount = Int64(day)
			let compare: Int64? = day <= 3 ? amount * -1000 : nil
			return SpendingCardDay(day: UInt32(day), compare: compare, compareTo: amount * -1500, average: amount * -1200, budget: Double(day) * -1100.5)
		}
		let single = SpendingPoint.all(days, mode: .singleMonth)
		#expect(single[2].compare == 3000)
		#expect(single[3].compare == nil, "nothing after today")
		#expect(single[0].comparison == 1500)
		#expect(single.last?.label == "28+")
		#expect(single[26].label == "27")
		#expect(SpendingPoint.all(days, mode: .average)[1].comparison == 2400)
		#expect(SpendingPoint.all(days, mode: .budget)[1].comparison == 2201)
		var broken = days
		broken[0].budget = Double.nan
		#expect(SpendingPoint.all(broken, mode: .budget)[0].comparison == nil)
	}
}

struct SankeyHighlightTests {
	let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)

	/// Paycheck and Interest into Income; Income to Housing and Everyday;
	/// Housing to Rent; Everyday to Groceries and Dining.
	private var report: SankeyReport {
		func node(_ kind: SankeyNodeKind, _ id: String?, _ name: String, _ value: Int64, _ column: UInt8) -> SankeyNode {
			SankeyNode(key: SankeyNodeKey(kind: kind, id: id), name: name, value: value, labels: none, count: 3, column: column)
		}
		let nodes = [
			node(.incomeCategory, "paycheck", "Paycheck", 900, 0),
			node(.incomeCategory, "interest", "Interest", 100, 0),
			node(.income, nil, "", 1000, 1),
			node(.categoryGroup, "housing", "Housing", 600, 2),
			node(.categoryGroup, "everyday", "Everyday", 400, 2),
			node(.category, "rent", "Rent", 600, 3),
			node(.category, "groceries", "Groceries", 300, 3),
			node(.category, "dining", "Dining out", 100, 3)
		]
		let flows: [(UInt32, UInt32, Int64)] = [(0, 2, 900), (1, 2, 100), (2, 3, 600), (2, 4, 400), (3, 5, 600), (4, 6, 300), (4, 7, 100)]
		return SankeyReport(first: "2026-10-01", last: "2026-10-31", nodes: nodes, flows: flows.map { SankeyFlow(from: $0.0, to: $0.1, value: $0.2) })
	}

	private var links: [(source: Int, target: Int?)] {
		report.flows.map { (Int($0.from), Int($0.to)) }
	}

	@Test func aCategoryLightsItsGroupAndTheGroupsFlowFromIncome() {
		let lit = SankeyHighlight(.node(6), links: links)
		#expect(lit.nodes == [6, 4, 2])
		#expect(lit.flows == [5, 3])
	}

	@Test func aGroupLightsItsCategoriesAndIncomeLightsOnlyItsOwnFlows() {
		let group = SankeyHighlight(.node(4), links: links)
		#expect(group.nodes == [2, 4, 6, 7])
		#expect(group.flows == [3, 5, 6])
		let income = SankeyHighlight(.node(2), links: links)
		#expect(income.flows == [0, 1, 2, 3, 4], "Income's own flows, and on through Housing, whose only flow is to Rent; Everyday has two, so the path stops there")
	}

	@Test func aFlowLightsItselfItsEndsAndTheSingleFlowChainsBeyond() {
		let rent = SankeyHighlight(.flow(4), links: links)
		#expect(rent.flows == [4, 2])
		#expect(rent.nodes == [3, 5, 2])
		#expect(SankeyHighlight(.flow(0), links: links).flows == [0])
		#expect(SankeyHighlight(.flow(0), links: [(0, nil)]).flows.isEmpty, "a cut flow never lights")
	}

	@Test func theReadoutNamesTheNodeOrBothEndsWithTheAmount() {
		#expect(report.readout(.node(7)) == SankeyReadout(title: "Dining out", amount: 100))
		#expect(report.readout(.flow(3)) == SankeyReadout(title: "Income → Everyday", amount: 400))
		#expect(report.readout(.node(99)) == nil)
	}

	@Test func clickingAFlowOpensTheNodeItCarriesMoneyFor() {
		#expect(report.drillNode(.flow(0))?.name == "Paycheck", "a source's flow into Income opens the source")
		#expect(report.drillNode(.flow(6))?.name == "Dining out")
		#expect(report.drillNode(.flow(2))?.name == "Housing")
		#expect(report.drillNode(.node(5))?.name == "Rent")
	}

	@Test func anywhereOnARibbonOrNearANodeIsAHit() {
		let layout = SankeyLayout(report: report, size: CGSize(width: 960, height: 600))
		let rent = layout.nodes[5].rect
		#expect(layout.target(at: CGPoint(x: rent.midX, y: rent.midY)) == .node(5))
		#expect(layout.target(at: CGPoint(x: rent.minX - 3, y: rent.midY)) == .node(5), "a little outside a thin node still counts")
		let ribbon = layout.ribbons[4]
		let middle = CGPoint(x: (ribbon.sourceX + ribbon.targetX) / 2, y: (ribbon.source.lowerBound + ribbon.source.upperBound + ribbon.target.lowerBound + ribbon.target.upperBound) / 4)
		#expect(layout.target(at: middle) == .flow(4))
		#expect(layout.target(at: CGPoint(x: 5, y: 595)) == nil)
	}
}

struct CardOpeningTests {
	@Test func cardsWithAReportPageOpenFullSize() {
		func card(_ content: WidgetView) -> DashboardCard {
			DashboardCard(id: "c", kind: .summary, rawKind: "summary-card", x: 0, y: 0, width: 4, height: 2, config: .empty, content: content)
		}
		#expect(card(.cashFlow(range: WidgetRange(start: "2026-11", end: "2026-11", mode: "sliding-window"), income: 1, expense: -1, change: 0)).opensFullSize)
		#expect(!card(.markdown(text: "## Notes", align: nil)).opensFullSize)
		#expect(!card(.missingReport).opensFullSize)
		#expect(!card(.unknown(kind: "future")).opensFullSize)
		#expect(!card(.problem(message: "No budget")).opensFullSize)
	}
}

struct SankeyCardReadoutTests {
	/// v18: a node shows the core's `value`, the amount Actual prints under
	/// it, and nothing summed on the Mac; a flow shows the core's link value.
	@Test func aCardNodeShowsTheCoresValueAndAFlowItsValue() {
		let nodes = [
			SankeyCardNode(key: "accounts", label: .allAccounts, percentage: "100%", color: .blue, value: 10_000),
			SankeyCardNode(key: "housing", label: .name(name: "Housing"), percentage: "55.1%", color: .palette(index: 1), value: 5_510),
			SankeyCardNode(key: "spacer", label: .hidden, percentage: "", color: .gray, value: 0),
			SankeyCardNode(key: "empty", label: .name(name: "Empty"), percentage: "0%", color: .gray, value: 0)
		]
		let links = [
			SankeyCardLink(source: 0, target: 1, value: 5510, folded: [], color: nil),
			SankeyCardLink(source: 0, target: 2, value: -1, folded: [], color: nil)
		]
		#expect(SankeyCardNode.readout(.node(1), nodes: nodes, links: links) == SankeyReadout(title: "Housing", amount: 5_510))
		#expect(SankeyCardNode.readout(.node(0), nodes: nodes, links: links) == SankeyReadout(title: "All accounts", amount: 10_000))
		#expect(SankeyCardNode.readout(.node(3), nodes: nodes, links: links) == SankeyReadout(title: "Empty", amount: nil), "Actual draws no amount at 0 or less")
		#expect(SankeyCardNode.readout(.flow(0), nodes: nodes, links: links) == SankeyReadout(title: "All accounts → Housing", amount: 5510))
		#expect(SankeyCardNode.readout(.node(2), nodes: nodes, links: links) == nil, "a hidden node has no words")
	}
}
