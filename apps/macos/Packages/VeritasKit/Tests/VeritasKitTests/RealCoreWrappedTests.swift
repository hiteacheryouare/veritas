import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Wrapped against the real core on fixtures/synthetic/all-features
/// (R-WRAP-1, R-WRAP-2): every year in the data renders, the pages carry the
/// core's figures, and the redacted pages carry no money amount.
@MainActor
struct RealCoreWrappedTests {
	static let today = "2026-11-16"

	@Test func eachYearInTheDataRendersAndHidesItsAmounts() async throws {
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			#expect(model.years.first == 2026)
			#expect(model.years.contains(2025), "the fixture starts in August 2025: \(model.years)")
			for year in model.years {
				await model.choose(year)
				let report = try #require(model.report)
				let core = try #require(await budget.workspace.report { try await $0.wrapped(year: year, today: Self.today) })
				#expect(report == core)
				#expect(report.hasData, "\(year)")
				#expect(model.issue?.isYearToDate == (year == 2026))
				#expect(!report.budgetName.isEmpty, "v24: the cover's name")

				let shown = try #require(model.issue(redacted: false))
				#expect(shown.spreads.count > 2, "\(year)")
				let ledger = shown.pages.first { $0.kind == .ledger }
				#expect(ledger?.rows.first?.value == model.format.currency(report.totalIn, on: .register))
				if let top = report.topCategories.first {
					#expect(shown.pages.first { $0.kind == .categories }?.rows.first?.value == model.format.currency(top.total, on: .register))
				}
				#expect(shown.pages.first { $0.kind == .netWorth }?.points.count == report.monthEnds.count, "N1: a point per month end")

				let hidden = try #require(model.issue(redacted: true))
				#expect(hidden.spreads.map(\.id) == shown.spreads.map(\.id), "the same spreads either way")
				for line in hidden.text {
					#expect(WrappedIssueTests.moneyFigures(in: line, format: model.format).isEmpty, "\(year): \(line)")
				}
				if let top = report.topCategories.first {
					#expect(hidden.pages.first { $0.kind == .categories }?.rows.first?.value == WrappedWords.percent(top.share, format: model.format))
				}
			}
		}
	}

	@Test func thePickerListsTheCoresYearsAndEachDrawsItsMonths() async throws {
		// v18: wrappedYears is the picker, and the shape of the year draws
		// FC-8's monthly series by the core's shares.
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			let listed = try #require(await budget.workspace.report { try await $0.wrappedYears(today: Self.today) })
			#expect(model.years == listed)
			#expect(listed == listed.sorted(by: >), "newest first")
			#expect(!listed.contains(2024), "the fixture starts in August 2025")
			for year in model.years {
				await model.choose(year)
				let report = try #require(model.report)
				#expect(report.hasData, "a listed year never reads No data: \(year)")
				#expect(report.months.last?.month == (year == 2026 ? Month(year: 2026, month: 11) : Month(year: year, month: 12)))
				let shape = try #require(model.issue(redacted: false)?.pages.first { $0.kind == .shape }, "\(year)")
				#expect(shape.columns.map(\.share) == report.months.map(\.share))
				#expect(shape.columns.filter(\.isBiggest).count == 1)
				#expect(shape.annotations.first?.text.hasSuffix(model.format.currency(report.biggestMonth?.total ?? 0, on: .register)) == true)
				let hidden = try #require(model.issue(redacted: true)?.pages.first { $0.kind == .shape })
				#expect(hidden.columns.map(\.share) == shape.columns.map(\.share))
			}
		}
	}

	/// FC-8 W-20 and W-21 on the real core: rows added after the fixture's
	/// data, with Wrapped asked for as of the case's own today.
	private func spend(_ amount: Int64, on date: String, _ budget: RealBudget) async {
		#expect(await budget.workspace.perform(.addTransaction(transaction: TransactionDraft(
			id: nil, accountId: AllFeatures.checking, date: date, amount: amount, payeeId: nil, payeeName: "Corner store",
			categoryId: AllFeatures.groceries, notes: nil, cleared: true, splits: []
		))))
	}

	@Test func w20AndW21ARefundMonthIsDrawnBelowTheLine() async throws {
		try await RealBudget.with { budget in
			await spend(-30_000, on: "2027-01-15", budget)
			await spend(-1_000, on: "2027-02-03", budget)
			await spend(6_000, on: "2027-02-10", budget)
			let model = WrappedModel(workspace: budget.workspace, today: "2027-03-31")
			await model.load()
			await model.choose(2027)
			let report = try #require(model.report)
			#expect(report.months.map(\.total) == [30_000, -5_000, 0], "W-20: shown, not clamped")
			#expect(report.totalOut == 25_000)
			#expect(report.months.map(\.share) == [1_200, -200, 0], "W-21")
			#expect(report.biggestMonth?.month == Month(year: 2027, month: 1), "W-22")

			let hidden = try #require(model.issue(redacted: true)?.pages.first { $0.kind == .shape })
			#expect(hidden.columns.map(\.slot) == [0, 1, 2])
			#expect(hidden.columns.map(\.share) == [1_200, -200, 0])
			#expect(hidden.columns.map(\.spoken) == ["January, 120.0% of total out", "February, refund month", "March, 0.0% of total out"])
			#expect(hidden.columns.map(\.isRefund) == [false, true, false])
		}
	}

	@Test func w21WithNoTotalOutEveryBarReadsNA() async throws {
		try await RealBudget.with { budget in
			await spend(-10_000, on: "2027-01-15", budget)
			await spend(20_000, on: "2027-02-10", budget)
			let model = WrappedModel(workspace: budget.workspace, today: "2027-03-31")
			await model.load()
			await model.choose(2027)
			let report = try #require(model.report)
			#expect(report.totalOut <= 0)
			#expect(report.months.allSatisfy { $0.share == nil })
			let hidden = try #require(model.issue(redacted: true)?.pages.first { $0.kind == .shape }, "January still has spending")
			#expect(hidden.columns.count == 3)
			#expect(hidden.columns.allSatisfy { $0.share == nil })
			#expect(hidden.columns.map(\.spoken) == ["January, n/a", "February, n/a", "March, n/a"])
		}
	}

	@Test func aYearBeforeTheDataSaysSo() async throws {
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			await model.choose(2023)
			#expect(model.report?.hasData == false)
			#expect(model.issue?.pages.map(\.kind) == [.cover])
			#expect(model.title == "Wrapped 2023")
		}
	}

	@Test func theSpreadsTurnAndTheFileNamesSayWhichExport() async throws {
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			#expect(model.index == 0)
			#expect(!model.canGoBack)
			model.previous()
			#expect(model.index == 0)
			model.next()
			#expect(model.index == 1)
			model.showLast()
			#expect(!model.canGoForward)
			model.next()
			#expect(model.index == model.spreads.count - 1)
			model.hidesAmounts = true
			#expect(model.spread?.id == model.issue(redacted: true)?.spreads.last?.id)
			model.showContents()
			#expect(model.index == 0, "the contents face the cover")
			#expect(model.title == "Wrapped 2026 so far")
			#expect(model.imageFileName(spread: 2, redacted: true) == "Wrapped 2026 spread 3 without amounts.png")
			#expect(model.imageFileName(spread: 0, redacted: false) == "Wrapped 2026 spread 1.png")
			#expect(model.pdfFileName(redacted: false) == "Wrapped 2026.pdf")
			#expect(model.folderName(redacted: true) == "Wrapped 2026 without amounts")
			await model.choose(2025)
			#expect(model.index == 0, "a new year starts at its cover")
		}
	}
}
