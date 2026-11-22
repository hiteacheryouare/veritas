import AppKit
import Foundation
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// Contract v18 on the stub the journeys read: the figures each new view
/// draws are there, and the stub answers as the core would.
struct V18StubTests {
	let session = StubSession(readOnly: nil)

	@Test func everyPickerSymbolIsOnThisMac() {
		for name in GoalIcon.symbols {
			#expect(NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil, "\(name)")
		}
	}

	@Test func aGoalIconIsASymbolOrOneEmoji() async throws {
		try await session.perform(.setGoalIcon(goalId: "laptop", icon: "car.fill"))
		try await session.perform(.setGoalIcon(goalId: "japan", icon: "🏴󠁧󠁢󠁥󠁮󠁧󠁿"))
		var goals = try await session.goals(today: WorkspaceModel.today()).goals
		#expect(goals.first { $0.id == "laptop" }?.icon == "car.fill")
		#expect(goals.first { $0.id == "japan" }?.icon == "🏴󠁧󠁢󠁥󠁮󠁧󠁿")
		for refused in ["Car", "car fill", ".car", "🚗🚗", "a🚗"] {
			await #expect(throws: FfiError.Invalid(message: "Choose a symbol or one emoji."), "\(refused)") {
				try await session.perform(.setGoalIcon(goalId: "laptop", icon: refused))
			}
		}
		try await session.perform(.setGoalIcon(goalId: "laptop", icon: nil))
		goals = try await session.goals(today: WorkspaceModel.today()).goals
		#expect(goals.first { $0.id == "laptop" }?.icon == nil)
	}

	@Test func drilledSavingsAreTransfersToSavings() async throws {
		let rows = try await session.reportTransactions(.netProfit(first: "2026-08-01", last: "2026-08-31", line: .savings, category: nil))
		#expect(!rows.isEmpty)
		#expect(rows.allSatisfy { $0.transferAccountId == "savings" && $0.amount < 0 })
		#expect(rows.map { PayeeText($0).direction } == rows.map { _ in "→" })
		#expect(rows.first.map { PayeeText($0).text } == "To Savings")
		let spending = try await session.reportTransactions(.netProfit(first: "2026-08-01", last: "2026-08-31", line: .expenses, category: nil))
		#expect(spending.allSatisfy { $0.transferAccountId == nil })
	}

	@Test func wrappedListsItsYearsAndEachHasItsMonths() async throws {
		let today = "2026-11-16"
		let years = try await session.wrappedYears(today: today)
		#expect(years == [2026, 2025])
		for year in years {
			let report = try await session.wrapped(year: year, today: today)
			#expect(report.hasData)
			let biggest = try #require(report.biggestMonth)
			let shown = try #require(report.months.first { $0.month == biggest.month })
			#expect(shown.total == biggest.total)
			#expect(shown.share == report.shares.biggestMonth)
		}
		#expect(try await session.wrapped(year: 2026, today: today).months.map(\.total).reduce(0, +) == 6_140_000)
	}

	@Test func theForecastHasHistoryABandEachMonthAndDifferences() async throws {
		let today = WorkspaceModel.today()
		let id = try await session.createScenario(
			named: "Trip", firstEvent: .oneTime(date: today, accountId: "checking", categoryId: nil, amount: -200_000), today: today
		)
		let forecast = try await session.forecast(today: today, scenarioIDs: [id])
		#expect(forecast.history.count == 12)
		#expect(forecast.baseline.months.dropFirst().allSatisfy { $0.bandLow != nil && $0.bandHigh != nil })
		for horizon in forecast.baseline.horizons {
			#expect(horizon.bandLow == forecast.baseline.months[Int(horizon.months)].bandLow)
		}
		let whatIf = try #require(forecast.scenarios.first)
		#expect(whatIf.horizons.allSatisfy { $0.netWorthDifference == -200_000 })
		#expect(whatIf.horizons.allSatisfy { $0.accounts.first { $0.accountId == "checking" }?.difference == -200_000 })
		#expect(forecast.baseline.horizons.allSatisfy { $0.netWorthDifference == nil })
	}
}
