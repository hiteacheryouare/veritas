import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// A session with fixed what-ifs, a forecast echoing the ids it was asked
/// for, and a record of every command.
final class ForecastFakeSession: BudgetSession, @unchecked Sendable {
	let canExport = true
	var readOnly: ReadOnlyKind?
	var commands: [Command] = []
	var asked: [[String]] = []
	var created: [CreatedScenario] = []
	var rateTexts: [String] = []
	var rates: [String: Int64] = ["10": 1_000, "2,5": 250]
	var scenarioList: [ScenarioSummary] = ["a", "b", "c", "d"].map { ScenarioSummary(id: $0, name: $0.uppercased(), events: []) }
	var suggestionItems = [
		BudgetSuggestion(categoryId: "rent", name: "Rent", amount: 165_000, basis: .bills, reason: "Rent $1,650, scheduled: Rent"),
		BudgetSuggestion(categoryId: "gifts", name: "Gifts", amount: nil, basis: .noHistory, reason: "Gifts: no history yet, keeping $40")
	]
	var invalidAdd: String?

	func overview() async throws -> BudgetOverview {
		BudgetOverview(name: "Household", directory: URL(filePath: "/tmp/budget"), readOnly: readOnly, accountGroups: [])
	}
	func undo() async throws {}
	func redo() async throws {}
	func maintain() async throws {}
	func exportActualZip(to destination: URL) async throws -> [AuditFinding] { [] }
	func observeChanges(_ changed: @escaping @Sendable () -> Void) async throws {}
	func close() async {}

	func perform(_ command: Command) async throws {
		commands.append(command)
		if case .deleteScenario(let id) = command { scenarioList.removeAll { $0.id == id } }
	}

	func scenarios(today: String) async throws -> [ScenarioSummary] { scenarioList }

	/// One command, as the core's v17 `createScenario`: a refused first
	/// event makes nothing.
	func createScenario(named name: String, firstEvent: ScenarioChange?, today: String) async throws -> String {
		guard !scenarioList.contains(where: { $0.name == name }) else { throw FfiError.Invalid(message: "Another what-if is already called \(name).") }
		if firstEvent != nil, let invalidAdd { throw FfiError.Invalid(message: invalidAdd) }
		created.append(CreatedScenario(name: name, firstEvent: firstEvent, today: today))
		let id = "new-\(created.count)"
		let events = firstEvent.map { [ScenarioEventItem(id: "event-\(created.count)", change: $0, inThePast: false)] } ?? []
		scenarioList.append(ScenarioSummary(id: id, name: name, events: events))
		return id
	}

	func evaluateRate(_ text: String, style: NumberStyle) async throws -> Int64 {
		rateTexts.append(text)
		guard let points = rates[text] else { throw FfiError.Invalid(message: "Can't read \(text) as a rate.") }
		return points
	}

	func forecast(today: String, scenarioIDs: [String]) async throws -> Forecast {
		asked.append(scenarioIDs)
		return ForecastSamples.forecast(today: today, scenarios: scenarioIDs.map { id in
			ForecastSamples.projection(id: id, name: id.uppercased(), shift: -10_000, laptopText: id == "b" ? "ETA January 2027" : "ETA March 2027")
		})
	}

	func budgetSuggestions(today: String) async throws -> SuggestionList {
		SuggestionList(month: Month(year: 2026, month: 12), items: suggestionItems, expectedIncome: 500_000, exceedsIncomeBy: nil, notice: nil)
	}
}

struct CreatedScenario: Equatable {
	let name: String
	let firstEvent: ScenarioChange?
	let today: String
}

final class ForecastFakeLibrary: BudgetLibrary, @unchecked Sendable {
	let session: ForecastFakeSession

	init(session: ForecastFakeSession) {
		self.session = session
	}

	func newBudgetDirectory() -> URL { URL(filePath: "/tmp/budgets/new") }
	func open(directory: URL) async throws -> any BudgetSession { session }
	func importActualZip(_ zip: URL, into directory: URL) async throws -> any BudgetSession { session }
	func createBudget(_ budget: NewBudget, into directory: URL) async throws -> any BudgetSession { session }
	func currencies() async throws -> [CurrencyOption] { FakeAmounts.currencies }
	func evaluateAmount(_ text: String, style: NumberStyle) async throws -> Int64 { try FakeAmounts.evaluate(text) }
}

enum ForecastSamples {
	static let november = Month(year: 2026, month: 11)

	static func projection(id: String?, name: String, shift: Int64, laptopText: String = "ETA March 2027") -> ForecastProjection {
		let months = (0...12).map { offset in
			ForecastMonth(month: november.adding(months: offset), knownNetIncome: 0, baselinedNetFlow: 0, scenarioNetIncome: shift, netIncome: 0, netWorth: 1_000_000 + Int64(offset) * 50_000 + shift)
		}
		let horizons = ([3, 6, 10, 12] as [UInt32]).map { count in
			let point = months[Int(count)]
			return ForecastHorizon(months: count, month: point.month, netWorth: point.netWorth, bandLow: point.netWorth - 20_000, bandHigh: point.netWorth + 10_000, accounts: [])
		}
		let laptop = GoalEtaItem(
			goalId: "laptop", name: "New laptop", status: .eta(month: Month(year: 2027, month: 3), optimistic: Month(year: 2027, month: 3), pessimistic: nil),
			remaining: 38_000, pace: 25_000, optimisticPace: 25_000, pessimisticPace: 0, historyMonths: 3, historyLabel: "3 months of history", onTrack: true, text: laptopText
		)
		return ForecastProjection(scenarioId: id, name: name, months: months, horizons: horizons, eventsInThePast: [], goalEtas: [laptop])
	}

	static func forecast(today: String, scenarios: [ForecastProjection]) -> Forecast {
		Forecast(
			today: today, netWorthToday: 1_000_000, accountsToday: [], baseline: projection(id: nil, name: "", shift: 0), scenarios: scenarios,
			bandMethod: .residuals, bandLabel: "80% band from 14 past months", baselines: [], knownFlows: [], loans: [], unforecast: [], shortfalls: [],
			possibleDoubleCounts: [], notForecastNote: nil, assetPurchaseNote: nil
		)
	}
}

@MainActor
struct ForecastModelTests {
	let session = ForecastFakeSession()

	func open() async -> ForecastModel {
		let workspace = WorkspaceModel(library: ForecastFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let model = ForecastModel(workspace: workspace, today: { "2026-11-19" })
		await model.load()
		return model
	}

	@Test func theHorizonPicksThePointEveryFigureReads() async throws {
		let model = await open()
		#expect(session.asked == [[]])
		#expect(model.projections.count == 1)
		#expect(model.selected?.months == 12)
		#expect(model.selected?.netWorth == 1_600_000)
		model.horizon = 3
		#expect(model.selected?.month == Month(year: 2027, month: 2))
		#expect(model.selected?.netWorth == 1_150_000)
	}

	@Test func theCompareHoldsThreeWhatIfsInTheOrderChosen() async throws {
		let model = await open()
		model.setCompared("c", true)
		await model.load()
		model.setCompared("a", true)
		await model.load()
		model.setCompared("b", true)
		await model.load()
		#expect(model.compared == ["c", "a", "b"])
		#expect(!model.canCompareMore)
		model.setCompared("d", true)
		await model.load()
		#expect(model.compared == ["c", "a", "b"], "a fourth is refused here, as the core refuses it")
		#expect(session.asked.last == ["c", "a", "b"])
		#expect(model.projections.map(\.scenarioId) == [nil, "c", "a", "b"])
		#expect(model.notCompared.map(\.id) == ["d"])
		model.setCompared("a", false)
		await model.load()
		#expect(model.compared == ["c", "b"])
		#expect(model.canCompareMore)
	}

	@Test func aDeletedWhatIfLeavesTheCompare() async throws {
		let model = await open()
		model.setCompared("a", true)
		await model.load()
		model.setCompared("b", true)
		await model.load()
		session.scenarioList.removeAll { $0.id == "a" }
		await model.load()
		#expect(model.compared == ["b"])
		#expect(session.asked.last == ["b"], "an id the core no longer has is never asked for")
	}

	@Test func aGoalShowsAWhatIfsLineOnlyWhenItDiffers() async throws {
		let model = await open()
		model.setCompared("a", true)
		await model.load()
		model.setCompared("b", true)
		await model.load()
		let row = try #require(model.goalRows.first)
		#expect(row.text == "ETA March 2027")
		#expect(row.whatIfs == [ForecastModel.GoalRow.WhatIf(index: 1, name: "B", text: "ETA January 2027")])
	}

	@Test func applyingSendsOneCommandWithToday() async throws {
		let model = await open()
		#expect(model.canApplySuggestions)
		#expect(await model.applySuggestions())
		#expect(session.commands == [.applySuggestions(today: "2026-11-19")])
	}

	@Test func onlyKeepingLinesOrReadOnlyLeaveNothingToApply() async throws {
		session.suggestionItems.removeFirst()
		let model = await open()
		#expect(!model.canApplySuggestions)
		#expect(!(await model.applySuggestions()))
		#expect(session.commands.isEmpty)

		let other = ForecastFakeSession()
		other.readOnly = .openElsewhere
		let workspace = WorkspaceModel(library: ForecastFakeLibrary(session: other), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let readOnly = ForecastModel(workspace: workspace, today: { "2026-11-19" })
		await readOnly.load()
		#expect(!readOnly.canApplySuggestions)
	}
}

@MainActor
struct NewWhatIfTests {
	let session = ForecastFakeSession()
	let trip = ScenarioChange.oneTime(date: "2027-03-10", accountId: "checking", categoryId: nil, amount: -200_000)

	func open() async -> WhatIfsModel {
		let workspace = WorkspaceModel(library: ForecastFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		return WhatIfsModel(workspace: workspace, today: { "2026-11-19" })
	}

	@Test func savingMakesTheWhatIfWithItsEventInOneCommandAndComparesIt() async throws {
		let model = await open()
		let new = NewWhatIf(model: model)
		new.name = " Vacation "
		let id = try #require(await new.save(trip))
		#expect(session.created == [CreatedScenario(name: "Vacation", firstEvent: trip, today: "2026-11-19")])
		#expect(session.commands.isEmpty, "no second command, so one undo step")
		#expect(model.isCompared(id))
		#expect(model.scenarios.first { $0.id == id }?.events.map(\.change) == [trip])
	}

	@Test func aRefusedEventMakesNothingAndTheRetryIsAFreshCommand() async throws {
		let model = await open()
		let new = NewWhatIf(model: model)
		new.name = "Vacation"
		session.invalidAdd = "Pick today or a later date."
		#expect(await new.save(trip) == nil)
		#expect(new.problem == "Pick today or a later date.")
		#expect(session.created.isEmpty)
		#expect(model.scenarios.isEmpty)

		session.invalidAdd = nil
		#expect(await new.save(trip) != nil)
		#expect(session.created.map(\.name) == ["Vacation"], "the same name works, because nothing was left behind")
	}

	@Test func aDuplicateNameShowsTheCoresLine() async throws {
		let model = await open()
		let new = NewWhatIf(model: model)
		new.name = "A"
		#expect(await new.save(trip) == nil)
		#expect(new.problem == "Another what-if is already called A.")
		#expect(session.created.isEmpty)
	}

	@Test func todayIsReadWhenItIsUsed() async throws {
		let workspace = WorkspaceModel(library: ForecastFakeLibrary(session: session), format: MoneyFormat())
		await workspace.reopen(directory: URL(filePath: "/tmp/budget"))
		let clock = Clock(day: "2026-11-30")
		let model = WhatIfsModel(workspace: workspace, today: { clock.day })
		#expect(model.thisMonth == Month(year: 2026, month: 11))
		clock.day = "2026-12-01"
		#expect(model.thisMonth == Month(year: 2026, month: 12), "past midnight the page works as of the new day")
		#expect(model.newDraft().date == "2026-12-01")
		_ = await model.create(named: "Later", firstEvent: trip, inline: { Issue.record("\($0)") })
		#expect(session.created.last?.today == "2026-12-01")
	}

	@Test func aPercentIsReadByTheCoreInTheBudgetsStyle() async throws {
		let model = await open()
		#expect(await model.basisPoints("2,5") == .success(250))
		#expect(session.rateTexts == ["2,5"], "Swift passes the text through as typed")
		#expect(await model.basisPoints("  ") == .failure(ScenarioDraft.Problem(message: ScenarioDraft.percentMissing)), "an empty field never reaches the core")
		#expect(await model.basisPoints("ten") == .failure(ScenarioDraft.Problem(message: "Can't read ten as a rate.")))
	}
}

final class Clock: @unchecked Sendable {
	var day: String

	init(day: String) {
		self.day = day
	}
}
