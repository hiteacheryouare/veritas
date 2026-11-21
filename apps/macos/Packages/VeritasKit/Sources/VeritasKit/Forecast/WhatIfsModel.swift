import Foundation
import Observation
import VeritasCore

/// The What-ifs page (R-FC-2): named overlays and their events. Each change
/// is one command and one undo step, and writes only `veritas_scenario*`.
@MainActor
@Observable
public final class WhatIfsModel {
	public private(set) var scenarios: [ScenarioSummary] = []
	public private(set) var names = BudgetNames.empty
	public private(set) var goals: [GoalSummary] = []

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored private let clock: @Sendable () -> String
	@ObservationIgnored private var loadGeneration = 0

	/// `today` is read each time it's used, so a page left open past
	/// midnight works as of the new day. Tests pass a fixed clock.
	public init(workspace: WorkspaceModel, today: @escaping @Sendable () -> String = { WorkspaceModel.today() }) {
		self.workspace = workspace
		clock = today
	}

	public var today: String { clock() }
	public var thisMonth: Month { Month(date: today) ?? Month(year: 2026, month: 1) }

	public var format: MoneyFormat { workspace.format }
	public var numberStyle: NumberStyle { workspace.numberStyle }
	public var isReadOnly: Bool { workspace.readOnly != nil }
	public var words: ForecastWords { ForecastWords(names: names, goals: goals, format: format) }

	/// Register amounts read at 2 places; a goal contribution reads at the budget's (D63).
	public func reader(_ scale: MoneyFormat.Scale) -> any AmountReader {
		scale == .budget ? workspace.budgetAmountReader : CoreAmountReader()
	}

	public func load() async {
		loadGeneration += 1
		let generation = loadGeneration
		let today = today
		async let names = workspace.budgetNames()
		async let goals = workspace.goals()
		let scenarios = await workspace.report { try await $0.scenarios(today: today) }
		let (loadedNames, loadedGoals) = await (names, goals)
		guard generation == loadGeneration else { return }
		self.names = loadedNames
		self.goals = loadedGoals
		if let scenarios { self.scenarios = scenarios }
	}

	// MARK: Choices

	public var accounts: [AccountDetail] { names.openAccounts }

	public var expenseCategories: [EnvelopeGroup] { categoryGroups(income: false) }
	public var incomeCategories: [EnvelopeGroup] { categoryGroups(income: true) }

	private func categoryGroups(income: Bool) -> [EnvelopeGroup] {
		names.categoryGroups
			.filter { $0.isIncome == income && !$0.hidden }
			.map { group in
				var group = group
				group.envelopes = group.envelopes.filter { !$0.hidden }
				return group
			}
			.filter { !$0.envelopes.isEmpty }
	}

	public var activeGoals: [GoalSummary] { goals.filter { $0.state == .active } }

	/// This month and the 24 after it, the months a what-if can start or end in.
	public var months: [Month] {
		let thisMonth = thisMonth
		return (0...24).map { thisMonth.adding(months: $0) }
	}

	/// What's typed in a percent field, in basis points, read by the core
	/// with the budget's number style. `nil` with the reason to show.
	public func basisPoints(_ text: String) async -> Result<Int64, ScenarioDraft.Problem> {
		guard !text.trimmingCharacters(in: .whitespaces).isEmpty else {
			return .failure(ScenarioDraft.Problem(message: ScenarioDraft.percentMissing))
		}
		let style = numberStyle
		switch await workspace.attempt({ try await $0.evaluateRate(text, style: style) }) {
		case .success(let points): return .success(points)
		case .failure(let error):
			if case .inline(let message) = ErrorPresentation(error) { return .failure(ScenarioDraft.Problem(message: message)) }
			return .failure(ScenarioDraft.Problem(message: ScenarioDraft.percentMissing))
		case nil: return .failure(ScenarioDraft.Problem(message: ScenarioDraft.percentMissing))
		}
	}

	public func newDraft() -> ScenarioDraft {
		ScenarioDraft(today: today, accountID: accounts.first { !$0.offBudget }?.id)
	}

	// MARK: Compare

	public func isCompared(_ id: String) -> Bool {
		workspace.comparedScenarios.contains(id)
	}

	public var canCompareMore: Bool {
		workspace.comparedScenarios.count < ForecastModel.maxCompared
	}

	public func setCompared(_ id: String, _ isOn: Bool) {
		workspace.setCompared(id, isOn)
	}

	// MARK: Commands

	/// The new what-if's id, or `nil` with the core's reason passed to
	/// `inline`. With `firstEvent` it is one undo step, and a refused event
	/// makes nothing (v17).
	public func create(named name: String, firstEvent: ScenarioChange? = nil, inline: (String) -> Void) async -> String? {
		let today = today
		let id = await workspace.change({ try await $0.createScenario(named: name, firstEvent: firstEvent, today: today) }, inline: inline)
		if id != nil { await load() }
		return id
	}

	public func rename(_ id: String, to name: String, inline: (String) -> Void) async -> Bool {
		await run(.renameScenario(scenarioId: id, name: name), inline: inline)
	}

	public func delete(_ id: String) async -> Bool {
		workspace.setCompared(id, false)
		return await run(.deleteScenario(scenarioId: id))
	}

	public func add(_ change: ScenarioChange, to scenarioID: String, inline: (String) -> Void) async -> Bool {
		await run(.addScenarioEvent(scenarioId: scenarioID, change: change, today: today), inline: inline)
	}

	public func edit(_ eventID: String, in scenarioID: String, to change: ScenarioChange, inline: (String) -> Void) async -> Bool {
		await run(.editScenarioEvent(scenarioId: scenarioID, eventId: eventID, change: change, today: today), inline: inline)
	}

	public func remove(_ eventID: String, from scenarioID: String) async -> Bool {
		await run(.removeScenarioEvent(scenarioId: scenarioID, eventId: eventID))
	}

	private func run(_ command: Command, inline: (String) -> Void) async -> Bool {
		let done = await workspace.perform(command, inline: inline)
		if done { await load() }
		return done
	}

	private func run(_ command: Command) async -> Bool {
		let done = await workspace.perform(command)
		if done { await load() }
		return done
	}
}

/// "Add what-if": a name and its first event, made in one command (v17),
/// so one ⌘Z takes both away and a refused event leaves nothing behind.
@MainActor
@Observable
public final class NewWhatIf {
	public var name = ""
	public private(set) var problem: String?

	@ObservationIgnored let model: WhatIfsModel

	public init(model: WhatIfsModel) {
		self.model = model
	}

	public var trimmedName: String { name.trimmingCharacters(in: .whitespaces) }

	public func show(_ problem: String?) {
		self.problem = problem
	}

	/// The what-if's id once it and its event are saved. It joins the compare.
	public func save(_ change: ScenarioChange) async -> String? {
		problem = nil
		guard let id = await model.create(named: trimmedName, firstEvent: change, inline: show) else { return nil }
		model.setCompared(id, true)
		return id
	}
}
