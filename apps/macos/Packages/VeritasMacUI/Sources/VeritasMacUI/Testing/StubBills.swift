#if DEBUG
import Foundation
import VeritasCore
import VeritasKit

/// The stub budget's bills, payees and accounts, after design/mockups/bills.html.
final class StubBills: @unchecked Sendable {
	private let lock = NSLock()
	private var all: [BillSummary]
	private var created = 0

	static let payees = [
		PayeeDetail(id: "landlord", name: "Landlord", transferAccountId: nil, favorite: true, learnCategories: true, transactions: 11),
		PayeeDetail(id: "phone-co", name: "Phone company", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 11),
		PayeeDetail(id: "isp", name: "Internet provider", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 11),
		PayeeDetail(id: "car-loan-payee", name: "Car loan", transferAccountId: "car-loan", favorite: false, learnCategories: false, transactions: 10),
		PayeeDetail(id: "power-co", name: "Power company", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 11),
		PayeeDetail(id: "music", name: "Music streaming", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 9),
		PayeeDetail(id: "employer", name: "Employer", transferAccountId: nil, favorite: true, learnCategories: true, transactions: 22),
		PayeeDetail(id: "gym", name: "Gym", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 4),
		PayeeDetail(id: "amazon", name: "Amazon", transferAccountId: nil, favorite: false, learnCategories: true, transactions: 31)
	]

	static let accounts = AccountList(accounts: [
		AccountDetail(id: "checking", name: "Checking", offBudget: false, closed: false, groupId: nil, cleared: 412_000, uncleared: 0, total: 412_000, note: nil, lastReconciled: nil),
		AccountDetail(id: "card", name: "Visa", offBudget: false, closed: false, groupId: nil, cleared: -61_340, uncleared: 0, total: -61_340, note: nil, lastReconciled: nil),
		AccountDetail(id: "car-loan", name: "Car loan", offBudget: true, closed: false, groupId: nil, cleared: -1_204_000, uncleared: 0, total: -1_204_000, note: nil, lastReconciled: nil)
	], groups: [])

	init() {
		func monthly(_ start: String, weekend: BillWeekend? = nil) -> BillDate {
			.repeat(repeat: BillRepeat(start: start, interval: 1, frequency: .monthly, patterns: [], weekend: weekend, end: .never))
		}
		func bill(
			_ id: String, _ name: String, payee: String, account: String?, category: String?, amount: BillAmount, scheduled: Int64,
			date: BillDate, autoPost: Bool, next: String?, state: BillState = .active, status: BillStatus = .scheduled, readOnly: Bool = false, customRule: Bool = false
		) -> BillSummary {
			BillSummary(
				id: id, name: name, payeeId: payee, payeeName: StubBills.payees.first { $0.id == payee }?.name,
				accountId: account, accountName: StubBills.accounts.accounts.first { $0.id == account }?.name, categoryId: category,
				scheduledAmount: scheduled, isIncome: scheduled > 0, autoPost: autoPost, nextDate: next, state: state, status: status,
				draft: readOnly ? nil : BillDraft(name: name, payeeId: payee, accountId: account, amount: amount, date: date, categoryId: category, autoPost: autoPost),
				customRule: customRule
			)
		}
		all = [
			bill("car", "Car loan", payee: "car-loan-payee", account: "checking", category: nil, amount: .exact(amount: -41_200), scheduled: -41_200, date: monthly("2026-01-12"), autoPost: false, next: "2026-11-12", status: .missed),
			bill("electric", "Electric", payee: "power-co", account: "checking", category: "electric", amount: .about(amount: -9_000), scheduled: -9_000, date: monthly("2026-01-15"), autoPost: false, next: "2026-11-15", status: .due),
			bill("music", "Music streaming", payee: "music", account: "card", category: "hobbies", amount: .exact(amount: -1_099), scheduled: -1_099, date: monthly("2026-01-18"), autoPost: true, next: "2026-11-18", status: .upcoming),
			bill("paycheck", "Paycheck", payee: "employer", account: "checking", category: "paycheck", amount: .between(low: 240_000, high: 260_000), scheduled: 250_000,
				date: .repeat(repeat: BillRepeat(start: "2026-01-02", interval: 2, frequency: .weekly, patterns: [], weekend: .fridayBefore, end: .never)), autoPost: true, next: "2026-11-20", status: .upcoming),
			bill("rent", "Rent", payee: "landlord", account: "checking", category: "rent", amount: .exact(amount: -165_000), scheduled: -165_000, date: monthly("2026-01-01", weekend: .fridayBefore), autoPost: true, next: "2026-12-01", customRule: true),
			bill("phone", "Phone", payee: "phone-co", account: "card", category: "landline", amount: .exact(amount: -4_500), scheduled: -4_500, date: monthly("2026-01-02"), autoPost: true, next: "2026-12-02"),
			bill("internet", "Internet", payee: "isp", account: "checking", category: "internet", amount: .exact(amount: -7_000), scheduled: -7_000,
				date: .repeat(repeat: BillRepeat(start: "2026-01-03", interval: 1, frequency: .monthly, patterns: [.day(day: 3), .day(day: -1)], weekend: nil, end: .afterOccurrences(count: 24))), autoPost: true, next: "2026-12-03"),
			bill("insurance", "Car insurance", payee: "car-loan-payee", account: "checking", category: nil, amount: .exact(amount: -61_000), scheduled: -61_000, date: monthly("2026-03-08"), autoPost: false, next: "2027-03-08", readOnly: true),
			bill("gym", "Gym", payee: "gym", account: "card", category: "hobbies", amount: .exact(amount: -3_500), scheduled: -3_500, date: monthly("2026-01-05"), autoPost: true, next: nil, state: .paused, status: .completed)
		]
	}

	func bills() -> [BillSummary] { lock.withLock { all } }

	func detail(_ id: String) throws -> BillDetail {
		guard let bill = bills().first(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "schedule", id: id) }
		let next = bill.state == .active ? ["2026-12-15", "2027-01-15", "2027-02-15", "2027-03-15", "2027-04-15"] : []
		return BillDetail(bill: bill, nextDates: next, paidTodayCounts: true)
	}

	func upcoming(days: UInt32) -> [UpcomingBill] {
		let last = days <= 7 ? "2026-11-22" : "2027-01-14"
		return bills()
			.filter { $0.state == .active && ($0.nextDate ?? "9999") <= last }
			.sorted { ($0.nextDate ?? "") < ($1.nextDate ?? "") }
			.map { UpcomingBill(billId: $0.id, name: $0.name, payeeName: $0.payeeName, accountId: $0.accountId, date: $0.nextDate ?? "", scheduledAmount: $0.scheduledAmount, isIncome: $0.isIncome, autoPost: $0.autoPost, status: $0.status == .scheduled ? .upcoming : $0.status, paidTodayCounts: true) }
	}

	/// Applies a bill command. Returns false for a command that isn't a bill's.
	func perform(_ command: Command) throws -> Bool {
		switch command {
		case .createBill(_, let draft):
			try Self.validate(draft)
			lock.withLock {
				created += 1
				all.append(BillSummary(
					id: "new-bill-\(created)", name: draft.name, payeeId: draft.payeeId, payeeName: Self.payees.first { $0.id == draft.payeeId }?.name,
					accountId: draft.accountId, accountName: Self.accounts.accounts.first { $0.id == draft.accountId }?.name, categoryId: draft.categoryId,
					scheduledAmount: Self.scheduled(draft.amount), isIncome: Self.scheduled(draft.amount) > 0, autoPost: draft.autoPost, nextDate: Self.first(draft.date),
					state: .active, status: .scheduled, draft: draft, customRule: false
				))
			}
		case .editBill(let id, let draft):
			try Self.validate(draft)
			try update(id) { bill in
				bill.name = draft.name
				bill.draft = draft
				bill.autoPost = draft.autoPost
				bill.scheduledAmount = Self.scheduled(draft.amount)
				bill.isIncome = Self.scheduled(draft.amount) > 0
			}
		case .pauseBill(let id): try update(id) { $0.state = .paused; $0.nextDate = nil }
		case .endBill(let id): try update(id) { $0.state = .ended; $0.nextDate = nil }
		case .resumeBill(let id): try update(id) { $0.state = .active; $0.nextDate = "2026-12-15"; $0.status = .scheduled }
		case .payBill(let id, _, _), .postBillNow(let id, _): try update(id) { $0.nextDate = "2026-12-15"; $0.status = .paid }
		case .skipBill(let id): try update(id) { $0.nextDate = "2026-12-15"; $0.status = .scheduled }
		default: return false
		}
		return true
	}

	private func update(_ id: String, _ change: (inout BillSummary) -> Void) throws {
		try lock.withLock {
			guard let index = all.firstIndex(where: { $0.id == id }) else { throw FfiError.NotFound(entity: "schedule", id: id) }
			change(&all[index])
		}
	}

	private static func validate(_ draft: BillDraft) throws {
		if (draft.name ?? "").trimmingCharacters(in: .whitespaces).isEmpty { throw FfiError.Invalid(message: "Give the bill a name.") }
		if case .between(let low, let high) = draft.amount, low == high { throw FfiError.Invalid(message: "A range needs two different amounts.") }
	}

	private static func scheduled(_ amount: BillAmount) -> Int64 {
		switch amount {
		case .exact(let amount), .about(let amount): amount
		case .between(let low, let high): (low + high) / 2
		}
	}

	private static func first(_ date: BillDate) -> String {
		switch date {
		case .once(let date): date
		case .repeat(let rule): rule.start
		}
	}
}
#endif
