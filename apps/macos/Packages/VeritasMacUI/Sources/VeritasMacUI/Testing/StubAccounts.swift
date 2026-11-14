#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// Accounts and registers for the stub budget: the register mockup's rows on
/// top of three years of made-up history, so paging and scrolling get real
/// volume. Everything is editable in memory.
final class StubAccounts: @unchecked Sendable {
	struct Txn {
		var id: String
		var account: String
		var date: String
		var amount: Int64
		var payee: String?
		var category: String?
		var notes: String?
		var cleared: Bool
		var reconciled: Bool
		var transfer: String?
		var counterpart: String?
		var lines: [RegisterLine] = []
		var startingBalance = false
	}

	private let lock = NSLock()
	private(set) var accounts: [AccountDetail] = [
		StubAccounts.account("checking", "Checking"), StubAccounts.account("savings", "Savings"), StubAccounts.account("visa", "Visa"),
		StubAccounts.account("brokerage", "Brokerage", offBudget: true), StubAccounts.account("carloan", "Car loan", offBudget: true),
		StubAccounts.account("oldcard", "Old store card", closed: true)
	]
	private var payees: [String: PayeeDetail] = [:]
	private var txns: [Txn] = []
	private var notes: [String: String] = ["checking": "Joint with Sam. Statement closes on the 9th."]
	private var reconciledAt: [String: String] = [:]

	static let categories: [String: String] = [
		"groceries": "Groceries", "dining": "Dining out", "gas": "Gas", "household": "Household", "rent": "Rent",
		"electric": "Electric", "internet": "Internet", "hobbies": "Hobbies", "gifts": "Gifts", "paycheck": "Paycheck",
		"principal": "Car loan payment", "interest": "Loan interest"
	]

	static func account(_ id: String, _ name: String, offBudget: Bool = false, closed: Bool = false) -> AccountDetail {
		AccountDetail(id: id, name: name, offBudget: offBudget, closed: closed, groupId: nil, cleared: 0, uncleared: 0, total: 0, note: nil, lastReconciled: nil)
	}

	init() {
		for (id, name) in [("fernwood", "Fernwood Market"), ("bistro", "Corner Bistro"), ("payroll", "Northwind Payroll"), ("maple", "Maple Court Apartments"), ("quickfuel", "Quickfuel"), ("harbor", "Harbor Electric"), ("pharmacy", "Lakeside Pharmacy"), ("clinic", "Northside Clinic"), ("bookshop", "Riverside Books"), ("fiber", "Fiberline")] {
			payees[id] = PayeeDetail(id: id, name: name, transferAccountId: nil, favorite: id == "fernwood", learnCategories: true, transactions: 0)
		}
		for account in accounts {
			payees["to-\(account.id)"] = PayeeDetail(id: "to-\(account.id)", name: account.name, transferAccountId: account.id, favorite: false, learnCategories: false, transactions: 0)
		}
		var next = 0
		func add(_ account: String, _ date: String, _ amount: Int64, _ payee: String?, _ category: String?, _ notes: String? = nil, cleared: Bool = true, reconciled: Bool = false) {
			next += 1
			txns.append(Txn(id: "t\(next)", account: account, date: date, amount: amount, payee: payee, category: category, notes: notes, cleared: cleared, reconciled: reconciled, transfer: nil, counterpart: nil))
		}
		func transfer(_ from: String, _ to: String, _ date: String, _ amount: Int64, category: String? = nil, notes: String? = nil, cleared: Bool = true) {
			next += 1
			let a = "t\(next)"
			next += 1
			let b = "t\(next)"
			txns.append(Txn(id: a, account: from, date: date, amount: -amount, payee: "to-\(to)", category: category, notes: notes, cleared: cleared, reconciled: false, transfer: to, counterpart: b))
			txns.append(Txn(id: b, account: to, date: date, amount: amount, payee: "to-\(from)", category: nil, notes: notes, cleared: cleared, reconciled: false, transfer: from, counterpart: a))
		}
		// Three years of history before the mockup's month and a half.
		var day = Calendar.current.date(from: DateComponents(year: 2023, month: 10, day: 1)) ?? .now
		let end = Calendar.current.date(from: DateComponents(year: 2026, month: 10, day: 9)) ?? .now
		var seed: UInt64 = 7
		func random(_ bound: Int64) -> Int64 {
			seed = seed &* 6364136223846793005 &+ 1442695040888963407
			return Int64((seed >> 33) % UInt64(bound))
		}
		let gigs: [(String, String, Int64)] = [("fernwood", "groceries", 9_000), ("bistro", "dining", 6_500), ("quickfuel", "gas", 5_000), ("bookshop", "hobbies", 3_800), ("pharmacy", "household", 3_000)]
		while day < end {
			let text = TransactionEditorModel.civilDate(day)
			let dayOfMonth = Calendar.current.component(.day, from: day)
			if dayOfMonth == 1 { add("checking", text, -165_000, "maple", "rent", reconciled: true) }
			if dayOfMonth == 3 || dayOfMonth == 20 { add("checking", text, 193_720, "payroll", "paycheck", reconciled: true) }
			if dayOfMonth == 10 { add("checking", text, -(7_000 + random(3_000)), "harbor", "electric", reconciled: true) }
			if dayOfMonth == 12 { transfer("checking", "carloan", text, 41_200, category: "principal", notes: "Car loan payment") }
			if random(3) == 0 {
				let gig = gigs[Int(random(Int64(gigs.count)))]
				add(random(4) == 0 ? "visa" : "checking", text, -(gig.2 / 2 + random(gig.2)), gig.0, gig.1, reconciled: true)
			}
			day = Calendar.current.date(byAdding: .day, value: 1, to: day) ?? end
		}
		add("checking", "2026-10-10", -8_836, "harbor", "electric")
		transfer("checking", "carloan", "2026-10-12", 41_200, category: "principal", notes: "Debt paydown 331.80, interest 80.20")
		add("checking", "2026-10-20", 193_720, "payroll", "paycheck")
		add("checking", "2026-10-28", -4_175, "quickfuel", "gas")
		add("checking", "2026-10-31", -5_820, "fernwood", "groceries", "Party supplies #gift")
		add("checking", "2026-11-01", -165_000, "maple", "rent")
		add("checking", "2026-11-03", 193_720, "payroll", "paycheck")
		transfer("savings", "checking", "2026-11-04", 15_000)
		transfer("checking", "visa", "2026-11-06", 35_000)
		add("checking", "2026-11-07", -6_430, "bistro", "dining", "Dinner with Sam #reimbursable", cleared: false)
		transfer("checking", "savings", "2026-11-08", 20_000, cleared: false)
		add("checking", "2026-11-09", -8_214, "fernwood", "groceries", "Includes prescription #medical", cleared: false)
		add("savings", "2023-09-30", 1_240_000, nil, nil, "Starting balance")
		add("brokerage", "2023-09-30", 3_180_000, nil, nil, "Starting balance")
		add("carloan", "2023-09-30", -2_480_000, nil, nil, "Starting balance")
		add("checking", "2023-09-30", 240_000, nil, nil, "Starting balance")
		recount()
	}

	private func recount() {
		for index in accounts.indices {
			let rows = txns.filter { $0.account == accounts[index].id }
			let cleared = rows.filter(\.cleared).reduce(0) { $0 + $1.amount }
			let total = rows.reduce(0) { $0 + $1.amount }
			accounts[index].cleared = cleared
			accounts[index].uncleared = total - cleared
			accounts[index].total = total
			accounts[index].note = notes[accounts[index].id]
			accounts[index].lastReconciled = reconciledAt[accounts[index].id]
		}
		for id in payees.keys {
			payees[id]?.transactions = UInt64(txns.filter { $0.payee == id }.count)
		}
	}

	func list() -> AccountList {
		lock.withLock { AccountList(accounts: accounts.filter { !$0.closed } + accounts.filter(\.closed), groups: []) }
	}

	func sidebar() -> [SidebarGroup] {
		lock.withLock {
			let open = accounts.filter { !$0.closed }
			let item = { (account: AccountDetail) in SidebarAccount(id: account.id, name: account.name, balance: account.total) }
			return [
				SidebarGroup(name: "Cash", accounts: open.filter { ["checking", "savings"].contains($0.id) || (!$0.offBudget && !["visa", "checking", "savings"].contains($0.id)) }.map(item)),
				SidebarGroup(name: "Credit", accounts: open.filter { $0.id == "visa" }.map(item)),
				SidebarGroup(name: "Investments", accounts: open.filter { $0.id == "brokerage" }.map(item)),
				SidebarGroup(name: "Loans", accounts: open.filter { $0.id == "carloan" }.map(item))
			].filter { !$0.accounts.isEmpty }
		}
	}

	private func row(_ txn: Txn, balance: Int64?) -> RegisterRow {
		let payee = txn.payee.flatMap { payees[$0] }
		let tags = (txn.notes ?? "").split(separator: " ").filter { $0.hasPrefix("#") }.map { String($0.dropFirst()) }
		return RegisterRow(
			id: txn.id, accountId: txn.account, accountName: accounts.first { $0.id == txn.account }?.name ?? "",
			date: txn.date, amount: txn.amount, payeeId: txn.payee, payeeName: payee?.name, categoryId: txn.category,
			categoryName: txn.category.flatMap { Self.categories[$0] }, notes: txn.notes, tags: tags, cleared: txn.cleared,
			reconciled: txn.reconciled, transferAccountId: txn.transfer, counterpartId: txn.counterpart, lines: txn.lines,
			splitDifference: nil, startingBalance: txn.startingBalance, scheduleId: nil, needsActual: nil, runningBalance: balance
		)
	}

	func register(_ scope: RegisterScope, filter: RegisterFilter, sort: RegisterSort, offset: UInt64, limit: UInt64) -> RegisterPage {
		lock.withLock {
			var rows = txns.filter { txn in
				switch scope {
				case .account(let id): return txn.account == id
				case .onBudget: return accounts.first { $0.id == txn.account }?.offBudget == false
				case .offBudget: return accounts.first { $0.id == txn.account }?.offBudget == true
				case .all: return true
				}
			}
			rows.sort { ($0.date, $0.id) > ($1.date, $1.id) }
			var balances: [String: Int64] = [:]
			if case .account = scope {
				var running = rows.reduce(0) { $0 + $1.amount }
				for txn in rows {
					balances[txn.id] = running
					running -= txn.amount
				}
			}
			let search = (filter.search ?? "").lowercased()
			rows = rows.filter { txn in
				if !search.isEmpty {
					let text = [txn.payee.flatMap { payees[$0]?.name }, txn.category.flatMap { Self.categories[$0] }, txn.notes].compactMap(\.self).joined(separator: " ").lowercased()
					if !text.contains(search) { return false }
				}
				if let status = filter.status {
					switch status {
					case .uncleared: if txn.cleared { return false }
					case .cleared: if !txn.cleared || txn.reconciled { return false }
					case .reconciled: if !txn.reconciled { return false }
					}
				}
				if let from = filter.from, txn.date < from { return false }
				if let to = filter.to, txn.date > to { return false }
				if let category = filter.categoryId, txn.category != category { return false }
				if filter.uncategorized, txn.category != nil || txn.transfer != nil { return false }
				return true
			}
			switch sort.key {
			case .date: if sort.ascending { rows.reverse() }
			case .amount: rows.sort { sort.ascending ? $0.amount < $1.amount : $0.amount > $1.amount }
			case .payee: rows.sort { (payees[$0.payee ?? ""]?.name ?? "") < (payees[$1.payee ?? ""]?.name ?? "") }
			case .category: rows.sort { (Self.categories[$0.category ?? ""] ?? "") < (Self.categories[$1.category ?? ""] ?? "") }
			case .notes: rows.sort { ($0.notes ?? "") < ($1.notes ?? "") }
			case .account: rows.sort { $0.account < $1.account }
			}
			let showBalance = sort.key == .date && !sort.ascending && search.isEmpty && filter.status == nil && filter.from == nil && filter.categoryId == nil
			let page = rows.dropFirst(Int(offset)).prefix(Int(limit)).map { row($0, balance: showBalance ? balances[$0.id] : nil) }
			return RegisterPage(total: UInt64(rows.count), rows: page)
		}
	}

	func registerRow(_ id: String) throws -> RegisterRow {
		try lock.withLock {
			guard let txn = txns.first(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "transaction", id: id) }
			return row(txn, balance: nil)
		}
	}

	func payeeList() -> [PayeeDetail] {
		lock.withLock { payees.values.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending } }
	}

	func memory(_ payee: String) -> PayeeMemory? {
		lock.withLock {
			guard let last = txns.filter({ $0.payee == payee && $0.transfer == nil }).max(by: { $0.date < $1.date }) else { return nil }
			return PayeeMemory(categoryId: last.category, amount: last.amount, date: last.date)
		}
	}

	func reconciliation(_ account: String, statement: Int64) -> Reconciliation {
		lock.withLock {
			let cleared = txns.filter { $0.account == account && $0.cleared }.reduce(0) { $0 + $1.amount }
			return Reconciliation(cleared: cleared, difference: statement - cleared)
		}
	}

	/// Handles the account, transaction, reconcile and payee commands, and
	/// says whether it did.
	func perform(_ command: Command) throws -> Bool {
		try lock.withLock {
			defer { recount() }
			switch command {
			case .addTransaction(let draft):
				txns.append(try make(draft, id: draft.id ?? UUID().uuidString.lowercased()))
			case .editTransaction(let id, let draft):
				guard let index = txns.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "transaction", id: id) }
				if txns[index].reconciled, !draft.cleared {
					throw FfiError.Invalid(message: "This transaction is reconciled. Unlock it before you uncheck cleared.")
				}
				var edited = try make(draft, id: id)
				edited.reconciled = txns[index].reconciled
				txns[index] = edited
			case .deleteTransaction(let id):
				let partner = txns.first { $0.id == id }?.counterpart
				txns.removeAll { $0.id == id || $0.id == partner }
			case .duplicateTransaction(let id, let newID):
				guard var copy = txns.first(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "transaction", id: id) }
				copy.id = newID ?? UUID().uuidString
				copy.cleared = false
				copy.reconciled = false
				copy.counterpart = nil
				txns.append(copy)
			case .setCleared(let ids, let cleared):
				for index in txns.indices where ids.contains(txns[index].id) && !txns[index].reconciled {
					txns[index].cleared = cleared
				}
			case .unlockReconciled(let id):
				if let index = txns.firstIndex(where: { $0.id == id }) { txns[index].reconciled = false }
			case .addReconcileAdjustment(let account, let statement):
				let cleared = txns.filter { $0.account == account && $0.cleared }.reduce(0) { $0 + $1.amount }
				txns.append(Txn(id: UUID().uuidString, account: account, date: TransactionEditorModel.civilDate(.now), amount: statement - cleared, payee: nil, category: nil, notes: "Reconciliation balance adjustment", cleared: true, reconciled: false, transfer: nil, counterpart: nil))
			case .finishReconcile(let account, let statement):
				let cleared = txns.filter { $0.account == account && $0.cleared }.reduce(0) { $0 + $1.amount }
				if cleared == statement {
					for index in txns.indices where txns[index].account == account && txns[index].cleared { txns[index].reconciled = true }
				}
				reconciledAt[account] = String(Int(Date.now.timeIntervalSince1970 * 1000))
			case .createAccount(let new):
				let id = "acct-\(accounts.count)"
				let offBudget = [.investment, .retirement, .property, .loan, .mortgage].contains(new.kind)
				accounts.append(Self.account(id, new.name, offBudget: offBudget))
				payees["to-\(id)"] = PayeeDetail(id: "to-\(id)", name: new.name, transferAccountId: id, favorite: false, learnCategories: false, transactions: 0)
				if new.startingBalance != 0 {
					let owed = [.creditCard, .lineOfCredit, .loan, .mortgage].contains(new.kind)
					txns.append(Txn(id: UUID().uuidString, account: id, date: TransactionEditorModel.civilDate(.now), amount: owed ? -new.startingBalance : new.startingBalance, payee: nil, category: nil, notes: "Starting balance", cleared: true, reconciled: false, transfer: nil, counterpart: nil, startingBalance: true))
				}
			case .renameAccount(let id, let name):
				guard let index = accounts.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "account", id: id) }
				let trimmed = name.trimmingCharacters(in: .whitespaces)
				guard !trimmed.isEmpty else { throw FfiError.Invalid(message: "Enter a name for the account.") }
				accounts[index].name = trimmed
				payees["to-\(id)"]?.name = trimmed
			case .moveAccount(let id, let before):
				guard let index = accounts.firstIndex(where: { $0.id == id }) else { return true }
				let moving = accounts.remove(at: index)
				let at = before.flatMap { b in accounts.firstIndex { $0.id == b } } ?? accounts.count
				accounts.insert(moving, at: at)
			case .setAccountOffBudget(let id, let offBudget):
				if let index = accounts.firstIndex(where: { $0.id == id }) { accounts[index].offBudget = offBudget }
			case .setAccountNote(let id, let text):
				let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
				notes[id] = trimmed.isEmpty ? nil : trimmed
			case .closeAccount(let id, let transferTo, let category):
				guard let index = accounts.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "account", id: id) }
				let balance = txns.filter { $0.account == id }.reduce(0) { $0 + $1.amount }
				if balance != 0 {
					guard let transferTo else { throw FfiError.Invalid(message: "Choose where the \(balance) left in \(accounts[index].name) goes.") }
					let a = UUID().uuidString, b = UUID().uuidString
					let today = TransactionEditorModel.civilDate(.now)
					txns.append(Txn(id: a, account: id, date: today, amount: -balance, payee: "to-\(transferTo)", category: category, notes: "Closing account", cleared: true, reconciled: false, transfer: transferTo, counterpart: b))
					txns.append(Txn(id: b, account: transferTo, date: today, amount: balance, payee: "to-\(id)", category: nil, notes: "Closing account", cleared: true, reconciled: false, transfer: id, counterpart: a))
				}
				accounts[index].closed = true
			case .reopenAccount(let id):
				if let index = accounts.firstIndex(where: { $0.id == id }) { accounts[index].closed = false }
			case .renamePayee(let id, let name):
				payees[id]?.name = name.trimmingCharacters(in: .whitespaces)
			case .mergePayees(let target, let others):
				for index in txns.indices where others.contains(txns[index].payee ?? "") { txns[index].payee = target }
				for other in others { payees[other] = nil }
			case .setPayeeFavorite(let id, let favorite):
				payees[id]?.favorite = favorite
			case .setPayeeLearnCategories(let id, let learn):
				payees[id]?.learnCategories = learn
			default:
				return false
			}
			return true
		}
	}

	private func make(_ draft: TransactionDraft, id: String) throws -> Txn {
		if !draft.splits.isEmpty, draft.splits.reduce(0, { $0 + $1.amount }) != draft.amount {
			throw FfiError.Invalid(message: "The split lines add up to \(draft.splits.reduce(0) { $0 + $1.amount }), not \(draft.amount).")
		}
		var payee = draft.payeeId
		if payee == nil, let name = draft.payeeName {
			if let existing = payees.values.first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame }) {
				payee = existing.id
			} else {
				let new = "payee-\(payees.count)"
				payees[new] = PayeeDetail(id: new, name: name, transferAccountId: nil, favorite: false, learnCategories: true, transactions: 0)
				payee = new
			}
		}
		let transfer = payee.flatMap { payees[$0]?.transferAccountId }
		return Txn(
			id: id, account: draft.accountId, date: draft.date, amount: draft.amount, payee: payee, category: draft.categoryId,
			notes: draft.notes, cleared: draft.cleared, reconciled: false, transfer: transfer, counterpart: nil,
			lines: draft.splits.map { line in
				RegisterLine(id: line.id ?? UUID().uuidString, amount: line.amount, payeeId: payee, payeeName: payee.flatMap { payees[$0]?.name }, categoryId: line.categoryId, categoryName: line.categoryId.flatMap { Self.categories[$0] }, notes: line.notes, transferAccountId: nil, counterpartId: nil)
			}
		)
	}
}
#endif
