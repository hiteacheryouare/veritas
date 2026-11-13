import Foundation
import Observation
import VeritasCore

/// The Goals screen: open goals as cards, closed ones in the archive, and the
/// create, close, reopen and re-target flows (R-GOAL-1/3/4, D66).
@MainActor
@Observable
public final class GoalsScreenModel {
	public private(set) var goals: [GoalSummary] = []

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored public let reader: any AmountReader

	public init(workspace: WorkspaceModel, reader: any AmountReader) {
		self.workspace = workspace
		self.reader = reader
	}

	public var open: [GoalSummary] { goals.filter(\.isOpen) }
	public var archive: [GoalSummary] { goals.filter { !$0.isOpen } }
	public var format: MoneyFormat { workspace.format }
	public var numberStyle: NumberStyle { workspace.numberStyle }
	public var isReadOnly: Bool { workspace.readOnly != nil }

	public func load() async {
		goals = await workspace.goals()
	}

	public func create(name: String, target: Int64, date: Date?, icon: String?, color: String?) async -> Bool {
		let done = await workspace.perform(.createGoal(
			name: name, target: target, targetDate: date.map(Self.civilDate), icon: icon, color: color
		))
		if done { await load() }
		return done
	}

	public func preview(close goal: GoalSummary, releaseTo destination: String?) async -> GoalClosePreview? {
		await workspace.goalClosePreview(goal.id, releaseTo: destination)
	}

	public func close(_ goal: GoalSummary, releaseTo destination: String?, alsoMoveLater: Bool) async -> Bool {
		let done = await workspace.perform(.closeGoal(goalId: goal.id, releaseToCategoryId: destination, alsoMoveLater: alsoMoveLater))
		if done { await load() }
		return done
	}

	public func reopen(_ goal: GoalSummary) async {
		if await workspace.perform(.reopenGoal(goalId: goal.id)) { await load() }
	}

	public func retarget(_ goal: GoalSummary, target: Int64, date: Date?) async -> Bool {
		guard goal.canRetarget else { return false }
		let done = await workspace.perform(.retargetGoal(goalId: goal.id, target: target, targetDate: date.map(Self.civilDate)))
		if done { await load() }
		return done
	}

	/// Envelopes a goal's leftover can go to, from this month's budget.
	public func destinations() async -> [EnvelopeRow] {
		let month = BudgetScreenModel.currentMonth()
		let goalCategories = Set(goals.map(\.categoryId))
		return await workspace.budgetMonth(month)?.groups
			.filter { !$0.isIncome }
			.flatMap(\.envelopes)
			.filter { !$0.hidden && !goalCategories.contains($0.id) } ?? []
	}

	static func civilDate(_ date: Date) -> String {
		WorkspaceModel.today(date)
	}
}
