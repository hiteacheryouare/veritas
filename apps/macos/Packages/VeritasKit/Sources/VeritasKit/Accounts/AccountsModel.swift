import Foundation
import Observation
import VeritasCore

/// The account list (R-ACC-1): on budget, off budget, then closed, each with
/// cleared, uncleared and total balances.
@MainActor
@Observable
public final class AccountsModel {
	public private(set) var list: AccountList?

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }

	public func load() async {
		if let list = await workspace.report({ try await $0.accounts() }) { self.list = list }
	}

	public var onBudget: [AccountDetail] { list?.accounts.filter { !$0.closed && !$0.offBudget } ?? [] }
	public var offBudget: [AccountDetail] { list?.accounts.filter { !$0.closed && $0.offBudget } ?? [] }
	public var closed: [AccountDetail] { list?.accounts.filter(\.closed) ?? [] }
	public var open: [AccountDetail] { list?.accounts.filter { !$0.closed } ?? [] }

	public func account(_ id: String) -> AccountDetail? {
		list?.accounts.first { $0.id == id }
	}

	@discardableResult
	public func create(name: String, kind: NewAccountKind, startingBalance: Int64) async -> Bool {
		await run(.createAccount(account: NewAccount(name: name, kind: kind, startingBalance: startingBalance)))
	}

	@discardableResult
	public func rename(_ id: String, to name: String) async -> Bool {
		await run(.renameAccount(accountId: id, name: name))
	}

	/// Up or down among the accounts it's listed with.
	public func move(_ id: String, by step: Int) async {
		guard let account = account(id) else { return }
		let siblings = account.closed ? closed : (account.offBudget ? offBudget : onBudget)
		guard let index = siblings.firstIndex(where: { $0.id == id }), siblings.indices.contains(index + step) else { return }
		let after = index + step + (step > 0 ? 1 : 0)
		let before = siblings.indices.contains(after) ? siblings[after].id : nil
		await run(.moveAccount(accountId: id, beforeAccountId: before))
	}

	public func canMove(_ id: String, by step: Int) -> Bool {
		guard let account = account(id) else { return false }
		let siblings = account.closed ? closed : (account.offBudget ? offBudget : onBudget)
		guard let index = siblings.firstIndex(where: { $0.id == id }) else { return false }
		return siblings.indices.contains(index + step)
	}

	public func setOffBudget(_ id: String, _ offBudget: Bool) async {
		await run(.setAccountOffBudget(accountId: id, offBudget: offBudget))
	}

	@discardableResult
	public func setNote(_ id: String, _ text: String) async -> Bool {
		await run(.setAccountNote(accountId: id, text: text))
	}

	public func setGroup(_ id: String, _ group: String?) async {
		await run(.setAccountGroup(accountId: id, groupId: group))
	}

	/// Actual's close (docs/actual-format.md): a balance moves to `transferTo`,
	/// categorized only when the money leaves the budget.
	@discardableResult
	public func close(_ id: String, transferTo: String?, category: String?) async -> Bool {
		await run(.closeAccount(accountId: id, transferToAccountId: transferTo, categoryId: category))
	}

	/// This month's spending envelopes, for a close that takes money off budget.
	public func envelopes() async -> [EnvelopeGroup] {
		let month = Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		return await workspace.budgetMonth(month)?.groups.filter { !$0.isIncome && !$0.hidden } ?? []
	}

	public func reopen(_ id: String) async {
		await run(.reopenAccount(accountId: id))
	}

	/// Whether closing into `destination` takes the money off budget, so the
	/// close sheet asks for a category.
	public func closingNeedsCategory(_ id: String, into destination: String?) -> Bool {
		guard let account = account(id), let destination, let target = self.account(destination) else { return false }
		return !account.offBudget && target.offBudget
	}

	@discardableResult
	private func run(_ command: Command) async -> Bool {
		let done = await workspace.perform(command)
		if done { await load() }
		return done
	}
}

/// Payees (R-ACC-5): rename, merge into one, favorite, and whether the payee
/// learns its category.
@MainActor
@Observable
public final class PayeesModel {
	public private(set) var payees: [PayeeDetail] = []
	public var selection: Set<String> = []
	public var search = ""

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var isReadOnly: Bool { workspace.readOnly != nil }

	public func load() async {
		if let payees = await workspace.report({ try await $0.payees() }) { self.payees = payees }
	}

	public var shown: [PayeeDetail] {
		let query = search.trimmingCharacters(in: .whitespaces)
		guard !query.isEmpty else { return payees }
		return payees.filter { $0.name.localizedCaseInsensitiveContains(query) }
	}

	/// Transfer payees are the accounts themselves and aren't merged or renamed.
	public var mergeable: [PayeeDetail] {
		payees.filter { selection.contains($0.id) && $0.transferAccountId == nil }
	}

	@discardableResult
	public func rename(_ id: String, to name: String) async -> Bool {
		await run(.renamePayee(payeeId: id, name: name))
	}

	public func merge(into target: String) async {
		let others = mergeable.map(\.id).filter { $0 != target }
		guard !others.isEmpty else { return }
		if await run(.mergePayees(targetPayeeId: target, payeeIds: others)) { selection = [target] }
	}

	public func setFavorite(_ id: String, _ favorite: Bool) async {
		await run(.setPayeeFavorite(payeeId: id, favorite: favorite))
	}

	public func setLearnCategories(_ id: String, _ learn: Bool) async {
		await run(.setPayeeLearnCategories(payeeId: id, learn: learn))
	}

	@discardableResult
	private func run(_ command: Command) async -> Bool {
		let done = await workspace.perform(command)
		if done { await load() }
		return done
	}
}

/// Reconcile (R-ACC-3): the statement balance against what's cleared, then
/// lock it, or add Actual's adjustment for the difference.
@MainActor
@Observable
public final class ReconcileModel {
	public let accountID: String
	public private(set) var statementBalance: Int64?
	public private(set) var result: Reconciliation?

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel, accountID: String) {
		self.workspace = workspace
		self.accountID = accountID
	}

	public var isBalanced: Bool { result?.difference == 0 }

	public func check(_ statement: Int64) async {
		statementBalance = statement
		await refresh()
	}

	public func refresh() async {
		guard let statement = statementBalance else { return }
		let id = accountID
		result = await workspace.report { try await $0.reconciliation(id, statementBalance: statement) }
	}

	@discardableResult
	public func finish() async -> Bool {
		guard let statement = statementBalance else { return false }
		return await workspace.perform(.finishReconcile(accountId: accountID, statementBalance: statement))
	}

	public func addAdjustment() async {
		guard let statement = statementBalance else { return }
		if await workspace.perform(.addReconcileAdjustment(accountId: accountID, statementBalance: statement)) {
			await refresh()
		}
	}
}
