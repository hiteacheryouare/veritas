import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Forecast, what-ifs and suggestions against the real core on
/// fixtures/synthetic/all-features (R-FC-1 to R-FC-4), as of the day the
/// core's own tests use.
@MainActor
struct RealCoreForecastTests {
	static let today = "2026-11-18"
	static let march = Month(year: 2027, month: 3)
	static let trip = ScenarioChange.oneTime(date: "2027-03-10", accountId: AllFeatures.checking, categoryId: AllFeatures.groceries, amount: -200_000)

	private func forecastModel(_ budget: RealBudget, today: String = today) async -> ForecastModel {
		let model = ForecastModel(workspace: budget.workspace, today: { today })
		await model.load()
		return model
	}

	private func whatIfs(_ budget: RealBudget, today: String = today) async -> WhatIfsModel {
		let model = WhatIfsModel(workspace: budget.workspace, today: { today })
		await model.load()
		return model
	}

	private func addTrip(_ model: WhatIfsModel, named name: String = "Vacation") async throws -> String {
		let new = NewWhatIf(model: model)
		new.name = name
		let id = await new.save(Self.trip)
		return try #require(id, "saved: \(String(describing: new.problem))")
	}

	@Test func theModelShowsTheCoresForecastAndSuggestionsAsGiven() async throws {
		try await RealBudget.with { budget in
			let model = await forecastModel(budget)
			let forecast = try #require(model.forecast)
			let core = try #require(await budget.workspace.report { try await $0.forecast(today: Self.today, scenarioIDs: []) })
			#expect(forecast == core)
			#expect(forecast.baseline.months.count == 13)
			#expect(forecast.baseline.horizons.map(\.months) == ForecastModel.horizons)
			#expect(forecast.bandMethod != .noHistory, "the fixture has more than a year of history")
			#expect(forecast.bandLabel.hasPrefix("80% band from") || forecast.bandLabel.hasPrefix("Based on"), "\(forecast.bandLabel)")
			for horizon in forecast.baseline.horizons {
				let low = try #require(horizon.bandLow), high = try #require(horizon.bandHigh)
				#expect(low <= horizon.netWorth && horizon.netWorth <= high, "\(horizon.months) months")
			}
			for months in ForecastModel.horizons {
				model.horizon = months
				#expect(model.selected == forecast.baseline.horizons.first { $0.months == months })
			}
			#expect(model.goalRows.map(\.id) == forecast.baseline.goalEtas.map(\.goalId))
			#expect(model.goalRows.map(\.text) == forecast.baseline.goalEtas.map(\.text))

			let suggestions = try #require(model.suggestions)
			#expect(suggestions == (await budget.workspace.report { try await $0.budgetSuggestions(today: Self.today) }))
			#expect(suggestions.month == AllFeatures.december)
			#expect(model.canApplySuggestions)
		}
	}

	@Test func aWhatIfMovesNetWorthFromItsMonthOnNeverTheBandAndTouchesNoRealData() async throws {
		try await RealBudget.with { budget in
			let transactions = try budget.transactionRows()
			let cells = try budget.sqlite("SELECT * FROM zero_budgets ORDER BY id")
			let whatIfs = await whatIfs(budget)
			let id = try await addTrip(whatIfs)
			#expect(budget.workspace.comparedScenarios == [id], "a new what-if joins the compare")
			#expect(whatIfs.scenarios.map(\.name) == ["Vacation"])
			#expect(whatIfs.scenarios.first?.events.map(\.change) == [Self.trip])
			#expect(whatIfs.words.sentence(Self.trip) == "Groceries: \u{2212}2,000.00 on March 10, 2027, Checking", "the fixture has no currency, so no symbol")

			let model = await forecastModel(budget)
			#expect(model.projections.map(\.scenarioId) == [nil, id])
			let forecast = try #require(model.forecast)
			let whatIf = try #require(forecast.scenarios.first)
			#expect(whatIf.name == "Vacation")
			for (base, moved) in zip(forecast.baseline.horizons, whatIf.horizons) {
				let shift: Int64 = base.month < Self.march ? 0 : -200_000
				#expect(moved.netWorth == base.netWorth + shift, "\(base.months) months")
				#expect(moved.bandHigh.map { $0 - moved.netWorth } == base.bandHigh.map { $0 - base.netWorth }, "the band's offsets never move")
				#expect(moved.bandLow.map { $0 - moved.netWorth } == base.bandLow.map { $0 - base.netWorth })
			}

			#expect(try budget.transactionRows() == transactions)
			#expect(try budget.sqlite("SELECT * FROM zero_budgets ORDER BY id") == cells)
			#expect(try budget.sqlite("SELECT COUNT(*) FROM veritas_scenario") == [["1"]])
			#expect(try budget.sqlite("SELECT COUNT(*) FROM veritas_scenario_event") == [["1"]])
		}
	}

	@Test func renameEditRemoveAndDeleteAreEachOneUndoStep() async throws {
		try await RealBudget.with { budget in
			let model = await whatIfs(budget)
			let id = try await addTrip(model)
			#expect(await model.rename(id, to: "Longer vacation", inline: { Issue.record("\($0)") }))
			#expect(model.scenarios.first?.name == "Longer vacation")

			let event = try #require(model.scenarios.first?.events.first)
			let later = ScenarioChange.recurring(start: "2027-01-05", endMonth: Month(year: 2027, month: 6), cadence: .everyMonths(count: 1), accountId: AllFeatures.checking, categoryId: AllFeatures.diningOut, amount: -5_000)
			#expect(await model.edit(event.id, in: id, to: later, inline: { Issue.record("\($0)") }))
			#expect(model.scenarios.first?.events.map(\.change) == [later])
			#expect(await model.remove(event.id, from: id))
			#expect(model.scenarios.first?.events.isEmpty == true)

			await budget.workspace.undo()
			await model.load()
			#expect(model.scenarios.first?.events.map(\.change) == [later])
			await budget.workspace.undo()
			await model.load()
			#expect(model.scenarios.first?.events.map(\.change) == [Self.trip])

			#expect(await model.delete(id))
			#expect(model.scenarios.isEmpty)
			#expect(budget.workspace.comparedScenarios.isEmpty)
			await budget.workspace.undo()
			await model.load()
			#expect(model.scenarios.map(\.name) == ["Longer vacation"])
		}
	}

	@Test func addWhatIfIsOneUndoStepAndARefusedEventLeavesNothing() async throws {
		try await RealBudget.with { budget in
			let model = await whatIfs(budget)
			_ = try await addTrip(model)
			#expect(model.scenarios.first?.events.map(\.change) == [Self.trip])
			await budget.workspace.undo()
			await model.load()
			#expect(model.scenarios.isEmpty, "one undo takes the what-if and its event (v17)")

			_ = try await addTrip(model)
			let duplicate = NewWhatIf(model: model)
			duplicate.name = "vacation"
			#expect(await duplicate.save(Self.trip) == nil)
			#expect(duplicate.problem?.isEmpty == false, "the core's line shows in the sheet")

			let past = NewWhatIf(model: model)
			past.name = "Last month"
			#expect(await past.save(.oneTime(date: "2026-10-01", accountId: AllFeatures.checking, categoryId: nil, amount: -100)) == nil)
			#expect(past.problem?.isEmpty == false)
			#expect(model.scenarios.map(\.name) == ["Vacation"], "a refused event makes no what-if")
			#expect(try budget.sqlite("SELECT COUNT(*) FROM veritas_scenario") == [["1"]])
		}
	}

	@Test func aPercentIsReadByTheCore() async throws {
		try await RealBudget.with { budget in
			let model = await whatIfs(budget)
			#expect(await model.basisPoints("10") == .success(1_000))
			#expect(await model.basisPoints("2.5%") == .success(250))
			#expect(await model.basisPoints("5+5") == .success(1_000), "arithmetic works as in an amount field")
			#expect(await model.basisPoints("-5") == .failure(ScenarioDraft.Problem(message: "A rate can't be negative.")))
			let id = try await addTrip(model)
			let points = try #require(try? await model.basisPoints("12.5").get())
			let cut = ScenarioChange.categoryPercent(categoryId: AllFeatures.groceries, percentBp: -points, startMonth: Month(year: 2027, month: 1), endMonth: nil)
			#expect(await model.add(cut, to: id, inline: { Issue.record("\($0)") }))
			#expect(model.words.sentence(cut) == "Groceries: \u{2212}12.5% from January 2027")
		}
	}

	@Test func d67AnEventThatIsNowInThePastIsFlaggedAndLeftOut() async throws {
		try await RealBudget.with { budget in
			let soon = ScenarioChange.oneTime(date: "2026-11-25", accountId: AllFeatures.checking, categoryId: nil, amount: -50_000)
			let model = await whatIfs(budget)
			let new = NewWhatIf(model: model)
			new.name = "Soon"
			let id = try #require(await new.save(soon))

			let later = await whatIfs(budget, today: "2026-12-02")
			let event = try #require(later.scenarios.first?.events.first)
			#expect(event.inThePast)
			let forecast = try #require(await forecastModel(budget, today: "2026-12-02").forecast)
			#expect(forecast.scenarios.first?.scenarioId == id)
			#expect(forecast.scenarios.first?.eventsInThePast == [event.id])
			#expect(forecast.scenarios.first?.horizons.map(\.netWorth) == forecast.baseline.horizons.map(\.netWorth))

			var problem: String?
			#expect(!(await later.edit(event.id, in: id, to: soon, inline: { problem = $0 })), "re-dating keeps the D67 rule")
			#expect(problem?.isEmpty == false)
			#expect(await later.edit(event.id, in: id, to: .oneTime(date: "2026-12-10", accountId: AllFeatures.checking, categoryId: nil, amount: -50_000), inline: { Issue.record("\($0)") }))
			#expect(later.scenarios.first?.events.first?.inThePast == false)
		}
	}

	@Test func applyingSuggestionsBudgetsDecemberAndOneUndoPutsItBack() async throws {
		try await RealBudget.with { budget in
			let model = await forecastModel(budget)
			let suggestions = try #require(model.suggestions)
			let before = await budget.screen(AllFeatures.december)
			let beforeCells = try suggestions.items.map { try #require(before.sheet).row($0.categoryId).budgeted }

			#expect(await model.applySuggestions())
			let after = try #require(await budget.screen(AllFeatures.december).sheet)
			for (item, old) in zip(suggestions.items, beforeCells) {
				#expect(try after.row(item.categoryId).budgeted == (item.amount ?? old), "\(item.reason)")
			}

			await budget.workspace.undo()
			let undone = try #require(await budget.screen(AllFeatures.december).sheet)
			#expect(try suggestions.items.map { try undone.row($0.categoryId).budgeted } == beforeCells)
		}
	}
}
