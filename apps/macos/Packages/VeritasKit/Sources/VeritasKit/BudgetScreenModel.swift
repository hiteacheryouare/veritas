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
	public var selection: String?
	public private(set) var editing: AmountFieldModel?
	public private(set) var editingID: String?

	@ObservationIgnored private let workspace: WorkspaceModel
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

	/// Envelopes in screen order, income last, for ↑ and ↓.
	public var envelopeIDs: [String] {
		sheet?.groups.flatMap { $0.envelopes.map(\.id) } ?? []
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
		sheet?.groups.lazy.flatMap(\.envelopes).first { $0.id == id }
	}

	public func group(of id: String) -> EnvelopeGroup? {
		sheet?.groups.first { $0.envelopes.contains { $0.id == id } }
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
		guard let groups = sheet?.groups.filter({ !$0.envelopes.isEmpty }), !groups.isEmpty else { return }
		let index = selection.flatMap { id in groups.firstIndex { $0.envelopes.contains { $0.id == id } } }
		let target = index.map { min(max($0 + step, 0), groups.count - 1) } ?? (step > 0 ? 0 : groups.count - 1)
		selection = groups[target].envelopes.first?.id
	}

	private func neighbour(of id: String, step: Int, skippingIncome: Bool) -> String? {
		let ids = sheet?.groups
			.filter { !skippingIncome || !$0.isIncome }
			.flatMap { $0.envelopes.map(\.id) } ?? []
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

	/// Envelopes that can give budget: those with money available, in screen order.
	public var sources: [EnvelopeRow] {
		sheet?.groups.filter { !$0.isIncome }.flatMap(\.envelopes).filter { $0.available > 0 } ?? []
	}

	public var spendingEnvelopes: [EnvelopeRow] {
		sheet?.groups.filter { !$0.isIncome }.flatMap(\.envelopes) ?? []
	}

	static func shift(_ month: Month, by step: Int) -> Month {
		let index = Int(month.year) * 12 + Int(month.month) - 1 + step
		return Month(year: Int32(index / 12), month: UInt32(index % 12 + 1))
	}
}
