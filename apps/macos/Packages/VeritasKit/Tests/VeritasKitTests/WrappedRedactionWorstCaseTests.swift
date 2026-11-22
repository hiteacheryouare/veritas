import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// R-WRAP-2 where an amount is hardest to spot: a budget that hides decimals
/// and names no currency writes 1,480.00 as "1,480", with no symbol and no
/// cents. No redacted card may carry any amount of 1,000 or more the report
/// holds, standing as a figure of its own, in any form the budget could write it.
@MainActor
struct WrappedRedactionWorstCaseTests {
	/// `figure` in `line` with no digit, separator or percent sign touching it.
	static func stands(_ figure: String, in line: String) -> Bool {
		var rest = line[...]
		while let range = rest.range(of: figure) {
			let before = range.lowerBound == line.startIndex ? nil : line[line.index(before: range.lowerBound)]
			let after = range.upperBound == line.endIndex ? nil : line[range.upperBound]
			let touching: (Character?) -> Bool = { $0.map { $0.isNumber || ",.%".contains($0) } ?? false }
			if !touching(before) && !touching(after) { return true }
			rest = line[range.upperBound...]
		}
		return false
	}

	@Test func aBudgetThatHidesDecimalsShowsNoAmountOnARedactedCard() async throws {
		try await RealBudget.with { budget in
			let directory = try budget.directory
			await budget.workspace.closeBudget()
			try ActualDatabase.execute(directory.appending(path: "db.sqlite"), "INSERT OR REPLACE INTO preferences (id, value) VALUES ('hideFraction', 'true')")
			await budget.workspace.reopen(directory: directory)
			try #require(budget.workspace.phase == .open)
			let format = budget.workspace.format
			try #require(format.hidesDecimals)

			let model = WrappedModel(workspace: budget.workspace, today: RealCoreWrappedTests.today)
			await model.load()
			#expect(model.years.count >= 2)
			for year in model.years {
				await model.choose(year)
				let report = try #require(model.report)
				let amounts = [report.totalIn, report.totalOut, report.net, report.debtPaydown, report.savings,
					report.netWorthStart, report.netWorthEnd, report.netWorthChange,
					report.otherCategories, report.otherGroups, report.otherPayees, report.biggestMonth?.total ?? 0]
					+ report.topCategories.map(\.total) + report.topGroups.map(\.total) + report.topPayees.map(\.total)
				let written = Set(amounts.filter { abs($0) >= 100_000 }.flatMap { amount in
					[amount, -amount].flatMap { [format.amount($0, on: .register), format.currency($0, on: .register)] }
				})
				#expect(model.cards(redacted: false).flatMap(\.text).contains { line in written.contains { Self.stands($0, in: line) } },
					"\(year): the shown cards do carry these amounts, so the check can see them")
				for line in model.cards(redacted: true).flatMap(\.text) {
					let leaked = written.filter { Self.stands($0, in: line) }
					#expect(leaked.isEmpty, "\(year): \(line) carries \(leaked)")
					#expect(line.firstMatch(of: /\d{1,3}(,\d{3})+(?![\d,]|%|\.\d+%)/) == nil, "\(year): \(line) has a grouped figure")
				}
			}
		}
	}
}
