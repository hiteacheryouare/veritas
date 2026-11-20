import Foundation
import Observation
import VeritasCore

/// The Forecast screen (R-FC-1 to R-FC-4, design/mockups/forecast.html): net
/// worth over the next 13 months with the baseline and up to 3 what-ifs, the
/// four horizons with their bands, goal ETAs, the lists behind the forecast,
/// and next month's budget suggestions. Every figure is the core's.
@MainActor
@Observable
public final class ForecastModel {
	public nonisolated static let horizons: [UInt32] = [3, 6, 10, 12]
	public nonisolated static let maxCompared = 3

	public private(set) var forecast: Forecast?
	public private(set) var suggestions: SuggestionList?
	public private(set) var scenarios: [ScenarioSummary] = []
	public var horizon: UInt32 = 12

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let today: String
	@ObservationIgnored private var loadGeneration = 0

	public init(workspace: WorkspaceModel, today: String? = nil) {
		self.workspace = workspace
		self.today = today ?? WorkspaceModel.today()
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }
	public var compared: [String] { workspace.comparedScenarios }

	public func load() async {
		loadGeneration += 1
		let generation = loadGeneration
		let today = today
		guard let scenarios = await workspace.report({ try await $0.scenarios(today: today) }), generation == loadGeneration else { return }
		self.scenarios = scenarios
		let ids = workspace.comparedScenarios.filter { id in scenarios.contains { $0.id == id } }
		if ids != workspace.comparedScenarios { workspace.comparedScenarios = ids }
		let loaded = await workspace.report { session in
			async let forecast = session.forecast(today: today, scenarioIDs: ids)
			async let suggestions = session.budgetSuggestions(today: today)
			return try await (forecast, suggestions)
		}
		guard let loaded, generation == loadGeneration else { return }
		(forecast, suggestions) = loaded
	}

	// MARK: Projections

	/// The baseline first, then each compared what-if in the order chosen.
	public var projections: [ForecastProjection] {
		guard let forecast else { return [] }
		return [forecast.baseline] + forecast.scenarios
	}

	public func point(_ projection: ForecastProjection, at months: UInt32? = nil) -> ForecastHorizon? {
		projection.horizons.first { $0.months == (months ?? horizon) }
	}

	public var selected: ForecastHorizon? {
		forecast.flatMap { point($0.baseline) }
	}

	// MARK: Compare

	public func isCompared(_ id: String) -> Bool {
		compared.contains(id)
	}

	public var canCompareMore: Bool {
		compared.count < Self.maxCompared
	}

	/// What-ifs that aren't in the compare, in the order they were made.
	public var notCompared: [ScenarioSummary] {
		scenarios.filter { !isCompared($0.id) }
	}

	/// The screen reloads when the compare changes, as it does after a change.
	public func setCompared(_ id: String, _ isOn: Bool) {
		workspace.setCompared(id, isOn)
	}

	// MARK: Goals

	/// One goal's baseline ETA line and, for each compared what-if whose line
	/// differs, that what-if's.
	public struct GoalRow: Identifiable, Hashable, Sendable {
		public struct WhatIf: Hashable, Sendable {
			/// 0 for the first compared what-if.
			public let index: Int
			public let name: String
			public let text: String
		}

		public let id: String
		public let name: String
		public let text: String
		public let onTrack: Bool?
		public let whatIfs: [WhatIf]
	}

	public var goalRows: [GoalRow] {
		guard let forecast else { return [] }
		return forecast.baseline.goalEtas.map { eta in
			let whatIfs = forecast.scenarios.enumerated().compactMap { index, projection -> GoalRow.WhatIf? in
				guard let other = projection.goalEtas.first(where: { $0.goalId == eta.goalId }), other.text != eta.text else { return nil }
				return GoalRow.WhatIf(index: index, name: projection.name, text: other.text)
			}
			return GoalRow(id: eta.goalId, name: eta.name, text: eta.text, onTrack: eta.onTrack, whatIfs: whatIfs)
		}
	}

	// MARK: Suggestions

	public var canApplySuggestions: Bool {
		!isReadOnly && suggestions?.items.contains { $0.amount != nil } == true
	}

	/// Sets next month's budget to every suggested amount: one command, one undo step.
	@discardableResult
	public func applySuggestions() async -> Bool {
		guard canApplySuggestions else { return false }
		return await workspace.perform(.applySuggestions(today: today))
	}
}

public extension WorkspaceModel {
	/// Adds a what-if to the compare, up to `ForecastModel.maxCompared`, or takes it out.
	func setCompared(_ id: String, _ isOn: Bool) {
		if isOn {
			guard !comparedScenarios.contains(id), comparedScenarios.count < ForecastModel.maxCompared else { return }
			comparedScenarios.append(id)
		} else {
			comparedScenarios.removeAll { $0 == id }
		}
	}
}
