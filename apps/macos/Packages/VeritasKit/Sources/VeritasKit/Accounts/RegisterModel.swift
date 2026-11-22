import Foundation
import Observation
import VeritasCore

extension RegisterRow: Identifiable {}
extension PayeeDetail: Identifiable {}
extension AccountDetail: Identifiable {}

/// How a register row's payee reads (R-XFER-2): a transfer is "→ To Savings"
/// or "← From Savings", never a bare payee. A report's drill-down rows read
/// the same way.
public struct PayeeText: Hashable, Sendable {
	public let direction: String?
	public let text: String

	public init(_ row: RegisterRow) {
		self.init(payee: row.payeeName, amount: row.amount, isTransfer: row.transferAccountId != nil)
	}

	public init(_ row: ReportTransaction) {
		self.init(payee: row.payee, amount: row.amount, isTransfer: row.transferAccountId != nil)
	}

	init(payee: String?, amount: Int64, isTransfer: Bool) {
		let name = payee ?? ""
		if isTransfer {
			direction = amount < 0 ? "→" : "←"
			text = amount < 0 ? "To \(name)" : "From \(name)"
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
		guard parts.count == 3, let value = CivilDate.date(date) else { return date }
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
	/// The chart above one account's register, over the date filter or the last 30 days.
	public private(set) var history: BalanceHistory?
	public private(set) var tags: [TagSummary] = []
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
	/// Contract v9 keeps the running balance under a date range, so only
	/// the other filters hide it.
	public var showsBalance: Bool {
		accountID != nil && sort.key == .date && !sort.ascending && isUnfilteredButDates
	}

	private var isUnfilteredButDates: Bool {
		(filter.search ?? "").isEmpty && filter.status == nil && filter.categoryId == nil && filter.payeeId == nil
			&& !filter.uncategorized && filter.tag == nil
	}

	public var isUnfiltered: Bool {
		isUnfilteredButDates && filter.from == nil && filter.to == nil
	}

	/// Starts over from the first page, as after any change.
	public func load() async {
		generation += 1
		pages = [:]
		loading = []
		await fetch(page: 0)
		revision += 1
		await loadChart()
	}

	private func loadChart() async {
		guard let accountID, workspace.canPlanTransfers else { return }
		let today = WorkspaceModel.today()
		let monthAgo = Calendar.current.date(byAdding: .day, value: -30, to: .now).map(TransactionEditorModel.civilDate) ?? today
		let from = filter.from ?? monthAgo
		let to = filter.to ?? today
		history = await workspace.report { try await $0.balanceHistory(accountID, from: from, to: to) }
		tags = await workspace.report { try await $0.tags() } ?? []
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

	/// DEBT-6 A.5: deleting a loan's side of a payment deletes the whole
	/// split it came from, so the confirmation names what goes with it.
	public func paymentsDeleted(with rows: [RegisterRow]) async -> [RegisterRow] {
		var payments: [RegisterRow] = []
		for id in Set(rows.compactMap(\.counterpartParentId)).sorted() {
			if let payment = await workspace.report({ try await $0.registerRow(id) }) { payments.append(payment) }
		}
		return payments
	}

	/// "Checking: To Car loan 331.80, Loan interest 80.20", for a payment's confirmation.
	public func describe(_ payment: RegisterRow) -> String {
		let lines = payment.lines.map { line in
			let name = line.transferAccountId == nil ? (line.categoryName ?? "Uncategorized") : "To \(line.payeeName ?? "")"
			return "\(name) \(format.amount(abs(line.amount), on: .register))"
		}
		return "\(payment.accountName): \(lines.joined(separator: ", "))"
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
