import Foundation
import Observation
import VeritasCore

extension RegisterRow: Identifiable {}
extension PayeeDetail: Identifiable {}
extension AccountDetail: Identifiable {}

/// How a register row's payee reads (R-XFER-2): a transfer is "→ To Savings"
/// or "← From Savings", never a bare payee.
public struct PayeeText: Hashable, Sendable {
	public let direction: String?
	public let text: String

	public init(_ row: RegisterRow) {
		let name = row.payeeName ?? ""
		if row.transferAccountId != nil {
			direction = row.amount < 0 ? "→" : "←"
			text = row.amount < 0 ? "To \(name)" : "From \(name)"
		} else {
			direction = nil
			text = name
		}
	}

	public var spoken: String { text }
}

public extension RegisterRow {
	var isTransfer: Bool { transferAccountId != nil }
	var isSplit: Bool { !lines.isEmpty }

	/// The Envelope column: a split names its lines' count.
	var envelopeText: String {
		if isSplit { return lines.count == 1 ? "Split, 1 line" : "Split, \(lines.count) lines" }
		return categoryName ?? ""
	}

	/// "Nov 9" this year, "Nov 9, 2025" otherwise.
	func shortDate(thisYear: Int32) -> String {
		let parts = date.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3, let value = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return date }
		return parts[0] == Int(thisYear)
			? value.formatted(.dateTime.month(.abbreviated).day())
			: value.formatted(.dateTime.month(.abbreviated).day().year())
	}
}

/// A register: one account, the on- or off-budget accounts, or all of them,
/// fetched a page at a time so scrolling costs only the rows shown (R-PERF-3).
@MainActor
@Observable
public final class RegisterModel {
	public static let pageSize = 250

	public let scope: RegisterScope
	public var filter = RegisterFilter(search: nil, status: nil, from: nil, to: nil, categoryId: nil, payeeId: nil, uncategorized: false)
	public var sort = RegisterSort(key: .date, ascending: false)
	public private(set) var total = 0
	/// Goes up whenever the rows change, so the table reloads.
	public private(set) var revision = 0
	public var selection: Set<String> = []
	public var isReconciling = false

	@ObservationIgnored private var pages: [Int: [RegisterRow]] = [:]
	@ObservationIgnored private var loading: Set<Int> = []
	@ObservationIgnored private var generation = 0
	/// Called with the row range a late page filled, so the table reloads just those.
	@ObservationIgnored public var rowsArrived: ((Range<Int>) -> Void)?
	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, scope: RegisterScope) {
		self.workspace = workspace
		self.scope = scope
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }

	public var accountID: String? {
		if case .account(let id) = scope { return id }
		return nil
	}

	/// The running balance is the account's, so it shows only in date order with no filter.
	public var showsBalance: Bool {
		accountID != nil && sort.key == .date && !sort.ascending && isUnfiltered
	}

	public var isUnfiltered: Bool {
		(filter.search ?? "").isEmpty && filter.status == nil && filter.from == nil && filter.to == nil
			&& filter.categoryId == nil && filter.payeeId == nil && !filter.uncategorized
	}

	/// Starts over from the first page, as after any change.
	public func load() async {
		generation += 1
		pages = [:]
		loading = []
		await fetch(page: 0)
		revision += 1
	}

	public func row(at index: Int) -> RegisterRow? {
		let page = index / Self.pageSize
		if let rows = pages[page] {
			let offset = index - page * Self.pageSize
			return rows.indices.contains(offset) ? rows[offset] : nil
		}
		if !loading.contains(page) {
			Task { await fetch(page: page) }
		}
		return nil
	}

	public func index(of id: String) -> Int? {
		for (page, rows) in pages {
			if let offset = rows.firstIndex(where: { $0.id == id }) { return page * Self.pageSize + offset }
		}
		return nil
	}

	public var loadedRows: [RegisterRow] {
		pages.keys.sorted().flatMap { pages[$0] ?? [] }
	}

	private func fetch(page: Int) async {
		guard !loading.contains(page) else { return }
		loading.insert(page)
		let generation = generation
		let (scope, filter, sort) = (scope, filter, sort)
		let offset = UInt64(page * Self.pageSize)
		let result = await workspace.report { try await $0.register(scope, filter: filter, sort: sort, offset: offset, limit: UInt64(Self.pageSize)) }
		guard generation == self.generation else { return }
		loading.remove(page)
		guard let result else { return }
		total = Int(result.total)
		pages[page] = result.rows
		let start = page * Self.pageSize
		rowsArrived?(start..<(start + result.rows.count))
	}

	public func setSort(_ key: RegisterSortKey, ascending: Bool) async {
		sort = RegisterSort(key: key, ascending: ascending)
		await load()
	}

	public func setFilter(_ change: (inout RegisterFilter) -> Void) async {
		change(&filter)
		await load()
	}

	public var selectedRows: [RegisterRow] {
		loadedRows.filter { selection.contains($0.id) }
	}

	@discardableResult
	public func setCleared(_ ids: [String], _ cleared: Bool) async -> Bool {
		guard !ids.isEmpty else { return false }
		return await workspace.perform(.setCleared(transactionIds: ids, cleared: cleared))
	}

	public func toggleCleared(_ row: RegisterRow) async {
		guard !row.reconciled else { return }
		await setCleared([row.id], !row.cleared)
	}

	public func delete(_ ids: [String]) async {
		for id in ids {
			guard await workspace.perform(.deleteTransaction(transactionId: id)) else { return }
		}
		selection.subtract(ids)
	}

	/// The copy's id is picked here so it can be selected.
	public func duplicate(_ id: String) async {
		let copy = UUID().uuidString.lowercased()
		if await workspace.perform(.duplicateTransaction(transactionId: id, newId: copy)) {
			selection = [copy]
		}
	}

	public func unlock(_ id: String) async {
		await workspace.perform(.unlockReconciled(transactionId: id))
	}

	public var thisYear: Int32 {
		Month(date: WorkspaceModel.today())?.year ?? 2026
	}
}
