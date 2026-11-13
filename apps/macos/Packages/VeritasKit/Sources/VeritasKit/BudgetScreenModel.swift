import Foundation
import Observation
import VeritasCore

/// The Budget screen: one month of envelopes, the edit in progress, and the
/// budget actions (R-BUD-1, R-BUD-3, R-BUD-4).
@MainActor
@Observable
public final class BudgetScreenModel {
	public private(set) var month: Month
	public private(set) var sheet: BudgetMonth?
	public private(set) var goals: [GoalSummary] = []
	public var selection: String?
	public private(set) var editing: AmountFieldModel?
	public private(set) var editingID: String?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored private let amountReader: any AmountReader

	public init(workspace: WorkspaceModel, reader: any AmountReader, month: Month = BudgetScreenModel.currentMonth()) {
		self.workspace = workspace
		amountReader = reader
		self.month = month
	}

	public static func currentMonth(_ now: Date = .now, calendar: Calendar = .current) -> Month {
		let parts = calendar.dateComponents([.year, .month], from: now)
		return Month(year: Int32(parts.year ?? 2026), month: UInt32(parts.month ?? 1))
	}

	public var format: MoneyFormat { workspace.format }

	public var isReadOnly: Bool { workspace.readOnly != nil }

	public var numberStyle: NumberStyle { workspace.numberStyle }

	public var reader: any AmountReader { amountReader }

	/// The groups and envelopes on screen: hidden ones only while Show Hidden is on.
	public var groups: [EnvelopeGroup] {
		guard let sheet else { return [] }
		if workspace.showsHiddenEnvelopes { return sheet.groups }
		return sheet.groups.compactMap { group in
			guard !group.hidden else { return nil }
			var visible = group
			visible.envelopes = group.envelopes.filter { !$0.hidden }
			return visible
		}
	}

	public var hasHiddenEnvelopes: Bool {
		sheet?.groups.contains { $0.hidden || $0.envelopes.contains(where: \.hidden) } ?? false
	}

	/// Envelopes in screen order, income last, for ↑ and ↓.
	public var envelopeIDs: [String] {
		groups.flatMap { $0.envelopes.map(\.id) }
	}

	public var canGoBack: Bool {
		guard let sheet else { return false }
		return sheet.month != sheet.firstMonth
	}

	public var canGoForward: Bool {
		guard let sheet else { return false }
		return sheet.month != sheet.lastMonth
	}

	public func load() async {
		guard let loaded = await workspace.budgetMonth(month) else { return }
		sheet = loaded
		month = loaded.month
		goals = await workspace.goals().filter(\.isOpen)
		if let selection, !envelopeIDs.contains(selection) {
			self.selection = nil
		}
	}

	public func previousMonth() async {
		guard canGoBack else { return }
		month = Self.shift(month, by: -1)
		await load()
	}

	public func nextMonth() async {
		guard canGoForward else { return }
		month = Self.shift(month, by: 1)
		await load()
	}

	public func envelope(_ id: String) -> EnvelopeRow? {
		groups.lazy.flatMap(\.envelopes).first { $0.id == id }
	}

	public func group(of id: String) -> EnvelopeGroup? {
		groups.first { $0.envelopes.contains { $0.id == id } }
	}


	public func beginEditing(_ id: String) {
		guard let envelope = envelope(id), group(of: id)?.isIncome == false else { return }
		selection = id
		editingID = id
		editing = AmountFieldModel(amount: envelope.budgeted, style: workspace.numberStyle, reader: amountReader)
	}

	public func cancelEditing() {
		editing = nil
		editingID = nil
	}

	/// Saves the edit; on success moves to the next (or previous) envelope's
	/// field, as Return and Shift-Return do in Actual's budget.
	public func commitEditing(thenMove step: Int = 0) async {
		guard let editing, let id = editingID else { return }
		guard let amount = await editing.commit() else { return }
		if amount != editing.original {
			guard await workspace.perform(.setBudget(month: month, categoryId: id, amount: amount)) else { return }
			await load()
		}
		cancelEditing()
		guard step != 0, let next = neighbour(of: id, step: step, skippingIncome: true) else { return }
		beginEditing(next)
	}


	public func moveSelection(by step: Int) {
		let ids = envelopeIDs
		guard !ids.isEmpty else { return }
		guard let current = selection, ids.contains(current) else {
			selection = step > 0 ? ids.first : ids.last
			return
		}
		selection = neighbour(of: current, step: step, skippingIncome: false) ?? current
	}

	/// ⌥↑ and ⌥↓: the first envelope of the previous or next group.
	public func moveSelectionToGroup(by step: Int) {
		let groups = self.groups.filter { !$0.envelopes.isEmpty }
		guard !groups.isEmpty else { return }
		let index = selection.flatMap { id in groups.firstIndex { $0.envelopes.contains { $0.id == id } } }
		let target = index.map { min(max($0 + step, 0), groups.count - 1) } ?? (step > 0 ? 0 : groups.count - 1)
		selection = groups[target].envelopes.first?.id
	}

	private func neighbour(of id: String, step: Int, skippingIncome: Bool) -> String? {
		let ids = groups
			.filter { !skippingIncome || !$0.isIncome }
			.flatMap { $0.envelopes.map(\.id) }
		guard let index = ids.firstIndex(of: id) else { return nil }
		let next = index + step
		return ids.indices.contains(next) ? ids[next] : nil
	}


	public func moveBudget(amount: Int64, from: String?, to: String?) async -> Bool {
		let moved = await workspace.perform(.moveBudget(month: month, amount: amount, fromCategoryId: from, toCategoryId: to))
		if moved { await load() }
		return moved
	}

	public func coverOverspending(_ id: String, from: String?) async -> Bool {
		let covered = await workspace.perform(.coverOverspending(month: month, categoryId: id, fromCategoryId: from, amount: nil))
		if covered { await load() }
		return covered
	}

	public func holdForNextMonth(_ amount: Int64) async -> Bool {
		let held = await workspace.perform(.holdForNextMonth(month: month, amount: amount))
		if held { await load() }
		return held
	}

	public func resetHold() async {
		if await workspace.perform(.resetHold(month: month)) { await load() }
	}

	public func setCarryover(_ id: String, _ carryover: Bool) async {
		if await workspace.perform(.setCarryover(month: month, categoryId: id, carryover: carryover)) { await load() }
	}

	public func setNote(_ id: String, _ text: String) async {
		if await workspace.perform(.setCategoryNote(categoryId: id, text: text)) { await load() }
	}

	/// What happens when the user deletes an envelope (R-BUD-5): one with
	/// history is archived instead, and a real delete moves its history.
	public enum Removal: Hashable, Sendable {
		case delete
		case archive(transactions: UInt64, budgetedMonths: UInt64)
	}

	public func removal(for id: String) async -> Removal? {
		guard let history = await workspace.categoryHistory(id) else { return nil }
		if history.transactions == 0, history.budgetedMonths == 0 { return .delete }
		return .archive(transactions: history.transactions, budgetedMonths: history.budgetedMonths)
	}

	@discardableResult
	public func run(_ command: Command) async -> Bool {
		let done = await workspace.perform(command)
		if done { await load() }
		return done
	}

	public func createEnvelope(in groupID: String, named name: String) async -> Bool {
		await run(.createCategory(groupId: groupID, name: name))
	}

	public func renameEnvelope(_ id: String, to name: String) async -> Bool {
		await run(.renameCategory(categoryId: id, name: name))
	}

	public func setHidden(_ id: String, _ hidden: Bool) async {
		await run(.setCategoryHidden(categoryId: id, hidden: hidden))
	}

	public func archive(_ id: String) async {
		await run(.archiveCategory(categoryId: id))
	}

	public func delete(_ id: String) async {
		await run(.deleteCategory(categoryId: id))
	}

	public func delete(_ id: String, movingHistoryTo destination: String) async -> Bool {
		await run(.deleteCategoryMovingHistory(categoryId: id, toCategoryId: destination))
	}

	/// Drops `id` before `beforeID` in `groupID`, or at the end when `beforeID` is `nil`.
	public func moveEnvelope(_ id: String, to groupID: String, before beforeID: String?) async {
		guard id != beforeID else { return }
		await run(.moveCategory(categoryId: id, groupId: groupID, beforeCategoryId: beforeID))
	}

	public func createGroup(named name: String) async -> Bool {
		await run(.createCategoryGroup(name: name))
	}

	public func renameGroup(_ id: String, to name: String) async -> Bool {
		await run(.renameCategoryGroup(groupId: id, name: name))
	}

	public func setGroupHidden(_ id: String, _ hidden: Bool) async {
		await run(.setCategoryGroupHidden(groupId: id, hidden: hidden))
	}

	public func moveGroup(_ id: String, before beforeID: String?) async {
		guard id != beforeID else { return }
		await run(.moveCategoryGroup(groupId: id, beforeGroupId: beforeID))
	}

	/// Envelopes that can give budget: those with money available, in screen order.
	public var sources: [EnvelopeRow] {
		groups.filter { !$0.isIncome }.flatMap(\.envelopes).filter { $0.available > 0 }
	}

	public var spendingEnvelopes: [EnvelopeRow] {
		groups.filter { !$0.isIncome }.flatMap(\.envelopes)
	}

	static func shift(_ month: Month, by step: Int) -> Month {
		let index = Int(month.year) * 12 + Int(month.month) - 1 + step
		return Month(year: Int32(index / 12), month: UInt32(index % 12 + 1))
	}
}
