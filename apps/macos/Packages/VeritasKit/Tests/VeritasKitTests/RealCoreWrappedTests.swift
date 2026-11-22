import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Wrapped against the real core on fixtures/synthetic/all-features
/// (R-WRAP-1, R-WRAP-2): every year in the data renders, the cards carry the
/// core's figures, and the redacted cards carry no money amount.
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
				#expect(model.story?.isYearToDate == (year == 2026))

				let shown = model.cards(redacted: false)
				#expect(shown.count > 1, "\(year)")
				#expect(shown.first?.rows.first?.value == model.format.currency(report.totalIn, on: .register))
				if let top = report.topCategories.first {
					#expect(shown.first { $0.kind == .categories }?.figure == model.format.currency(top.total, on: .register))
				}

				let hidden = model.cards(redacted: true)
				#expect(hidden.map(\.kind) == shown.map(\.kind), "the same cards either way")
				for line in hidden.flatMap(\.text) {
					#expect(WrappedStoryTests.moneyFigures(in: line, format: model.format).isEmpty, "\(year): \(line)")
				}
				if let top = report.topCategories.first {
					#expect(hidden.first { $0.kind == .categories }?.figure == WrappedWords.percent(top.share, format: model.format))
				}
			}
		}
	}

	@Test func thePickerListsTheCoresYearsAndEachDrawsItsMonths() async throws {
		// v18: wrappedYears is the picker, and the biggest-month card draws
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
				let card = try #require(model.cards(redacted: false).first { $0.kind == .biggestMonth }, "\(year)")
				let drawn = report.months.filter { $0.share != nil }
				#expect(card.bars.map(\.share) == drawn.compactMap(\.share))
				#expect(card.bars.filter(\.isBiggest).count == 1)
				#expect(card.bars.first(where: \.isBiggest)?.value == model.format.wholeCurrency(report.biggestMonth?.total ?? 0, on: .register))
				let hidden = try #require(model.cards(redacted: true).first { $0.kind == .biggestMonth })
				#expect(hidden.bars.map(\.share) == card.bars.map(\.share))
				#expect(hidden.bars.allSatisfy { $0.value == nil })
			}
		}
	}

	@Test func aYearBeforeTheDataSaysSo() async throws {
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			await model.choose(2023)
			#expect(model.report?.hasData == false)
			#expect(model.cards.map(\.kind) == [.noData])
			#expect(model.title == "Wrapped 2023")
		}
	}

	@Test func theCardsStepAndTheFileNameSaysWhichImage() async throws {
		try await RealBudget.with { budget in
			let model = WrappedModel(workspace: budget.workspace, today: Self.today)
			await model.load()
			#expect(model.index == 0)
			#expect(!model.canGoBack)
			model.previous()
			#expect(model.index == 0)
			model.next()
			#expect(model.index == 1)
			model.show(model.cards.count - 1)
			#expect(!model.canGoForward)
			model.next()
			#expect(model.index == model.cards.count - 1)
			model.hidesAmounts = true
			#expect(model.card?.kind == model.cards(redacted: true).last?.kind)
			#expect(model.title == "Wrapped 2026 so far")
			#expect(model.imageFileName(redacted: true) == "Wrapped 2026 without amounts.png")
			#expect(model.imageFileName(redacted: false) == "Wrapped 2026.png")
			await model.choose(2025)
			#expect(model.index == 0, "a new year starts at its first card")
		}
	}
}
