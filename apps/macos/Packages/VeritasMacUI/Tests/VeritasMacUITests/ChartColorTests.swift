import Testing
import VeritasCore
import VeritasDesign
@testable import VeritasMacUI

/// The money colors the balance graph and the Sankey choose, read through
/// their seams rather than from screenshots.
struct ChartColorTests {
	/// ST-9.1 and ST-2: below zero, a liability is neutral and an asset is
	/// overspent red; above zero, every account is green.
	@Test func theBalanceGraphColorsBelowZeroByTheCoresPlacement() {
		#expect(BalanceGraph.belowLine(isLiability: true) == .init(token: .textPrimary, opacity: 1))
		#expect(BalanceGraph.belowArea(isLiability: true).token == .textPrimary)
		#expect(BalanceGraph.belowLine(isLiability: false).token == ColorToken.moneyOverspentFill.chartMark)
		#expect(BalanceGraph.belowArea(isLiability: false).token == .moneyOverspentFill)
		for isLiability in [true, false] {
			#expect(BalanceGraph.belowLine(isLiability: isLiability).token != ColorToken.moneyAvailableFill.chartMark)
		}
		#expect(BalanceGraph.aboveLine().token == ColorToken.moneyAvailableFill.chartMark, "green above zero for every account")
		#expect(BalanceGraph.aboveArea().token == .moneyAvailableFill)
	}

	@Test func sankeyGroupsTakeTheCategoryPaletteAndCategoriesTheirGroups() {
		let none = ReportLabels(hidden: false, archived: false, deleted: false, closedGoal: false, needsReview: false)
		func node(_ kind: SankeyNodeKind, _ id: String?, column: UInt8) -> SankeyNode {
			SankeyNode(key: SankeyNodeKey(kind: kind, id: id), name: id ?? "", value: 100, labels: none, count: 1, column: column)
		}
		let nodes = [
			node(.incomeCategory, "paycheck", column: 0),
			node(.income, nil, column: 1),
			node(.categoryGroup, "housing", column: 2),
			node(.categoryGroup, "everyday", column: 2),
			node(.categoryGroup, "personal", column: 2),
			node(.savings, nil, column: 2),
			node(.debtPaydown, nil, column: 2),
			node(.unallocated, nil, column: 2),
			node(.category, "rent", column: 3),
			node(.category, "groceries", column: 3),
			node(.category, "gas", column: 3),
			node(.category, "gifts", column: 3)
		]
		let flows = [(0, 1), (1, 2), (1, 3), (1, 4), (1, 5), (1, 6), (1, 7), (2, 8), (3, 9), (3, 10), (4, 11)]
			.map { SankeyFlow(from: UInt32($0.0), to: UInt32($0.1), value: 100) }
		let tokens = SankeyCanvas.tokens(for: SankeyReport(first: "2026-01", last: "2026-11", nodes: nodes, flows: flows))

		#expect(tokens.count == nodes.count)
		#expect(Array(tokens[2...4]) == Array(SankeyCanvas.groupTokens.prefix(3)), "groups take the palette in order")
		for skipped in [ColorToken.category2, .category4, .category9] {
			#expect(!SankeyCanvas.groupTokens.contains(skipped), "\(skipped) belongs to income, debt paydown or the ninth series")
		}
		#expect(tokens[8] == tokens[2], "rent takes housing's color")
		#expect(tokens[9] == tokens[3] && tokens[10] == tokens[3], "groceries and gas take everyday's color")
		#expect(tokens[11] == tokens[4], "gifts take personal's color")
		#expect(tokens[0] == .moneyIncomeFill && tokens[1] == .moneyIncomeFill)
		#expect(tokens[5] == .chartSavings)
		#expect(tokens[6] == .chartDebtPaydown)
		#expect(tokens[7] == .chartUnallocated)
	}
}
