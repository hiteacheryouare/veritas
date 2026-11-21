import Foundation
import Observation
import VeritasCore

/// A bill open in the editor. Amounts are kept as magnitudes while editing;
/// `isIncome` gives them their sign when saved (R-BILL-5).
public struct BillEditing: Hashable, Sendable {
	/// Nil for a new bill.
	public let id: String?
	public var draft: BillDraft
	public var isIncome: Bool
	/// The next dates and Paid's rule, for a saved bill.
	public var detail: BillDetail?
	/// The core's `Invalid` message, shown beside the fields.
	public var problem: String?

	public var isNew: Bool { id == nil }

	/// The draft with the sign the core expects: negative for a bill, positive for income.
	public var signedDraft: BillDraft {
		var draft = draft
		draft.amount = draft.amount.signed(isIncome: isIncome)
		return draft
	}
}

/// The editor's fields over the draft's enums: switching once to repeating
/// keeps the date as the start, and back again keeps the start.
public extension BillEditing {
	var repeats: Bool {
		get { rule != nil }
		set {
			switch (newValue, draft.date) {
			case (true, .once(let date)):
				draft.date = .repeat(repeat: BillRepeat(start: date, interval: 1, frequency: .monthly, patterns: [], weekend: nil, end: .never))
			case (false, .repeat(let rule)):
				draft.date = .once(date: rule.start)
			default:
				break
			}
		}
	}

	/// Nil for a one-time bill. Setting it on a one-time bill does nothing.
	var rule: BillRepeat? {
		get {
			if case .repeat(let rule) = draft.date { return rule }
			return nil
		}
		set {
			guard let newValue, repeats else { return }
			draft.date = .repeat(repeat: newValue)
		}
	}

	/// The one-time date, or the repeating bill's start.
	var firstDate: String {
		get {
			switch draft.date {
			case .once(let date): date
			case .repeat(let rule): rule.start
			}
		}
		set {
			switch draft.date {
			case .once: draft.date = .once(date: newValue)
			case .repeat(var rule):
				rule.start = newValue
				draft.date = .repeat(repeat: rule)
			}
		}
	}
}

public extension BillAmount {
	init(kind: BillAmountKind, low: Int64, high: Int64) {
		switch kind {
		case .exact: self = .exact(amount: low)
		case .about: self = .about(amount: low)
		case .between: self = .between(low: low, high: high)
		}
	}

	var kind: BillAmountKind {
		switch self {
		case .exact: .exact
		case .about: .about
		case .between: .between
		}
	}

	/// The single amount, or a range's low end.
	var low: Int64 {
		switch self {
		case .exact(let amount), .about(let amount): amount
		case .between(let low, _): low
		}
	}

	/// A range's high end, or the single amount.
	var high: Int64 {
		switch self {
		case .exact(let amount), .about(let amount): amount
		case .between(_, let high): high
		}
	}

	var magnitudes: BillAmount {
		switch self {
		case .exact(let amount): .exact(amount: abs(amount))
		case .about(let amount): .about(amount: abs(amount))
		case .between(let low, let high): .between(low: abs(low), high: abs(high))
		}
	}

	func signed(isIncome: Bool) -> BillAmount {
		let sign: Int64 = isIncome ? 1 : -1
		switch magnitudes {
		case .exact(let amount): return .exact(amount: sign * amount)
		case .about(let amount): return .about(amount: sign * amount)
		case .between(let low, let high): return .between(low: sign * low, high: sign * high)
		}
	}

	/// "About" and "between" amounts are estimates: auto-posting one books a guess.
	var isEstimate: Bool {
		if case .exact = self { return false }
		return true
	}
}

/// The Bills screen: the upcoming tray, the timeline, every bill, and the
/// editor (R-BILL-1 to R-BILL-5).
@MainActor
@Observable
public final class BillsModel {
	/// Actual's upcoming tray shows a week.
	public static let trayDays: UInt32 = 7
	public static let timelineDays: UInt32 = 60

	public private(set) var bills: [BillSummary] = []
	public private(set) var tray: [UpcomingBill] = []
	public private(set) var timeline: [UpcomingBill] = []
	public private(set) var names = BudgetNames.empty
	public var editing: BillEditing?
	/// The core's message for the last Paid, Skip or Post now that failed inline.
	public private(set) var actionProblem: (billId: String, message: String)?

	@ObservationIgnored let workspace: WorkspaceModel

	public init(workspace: WorkspaceModel) {
		self.workspace = workspace
	}

	public var format: MoneyFormat { workspace.format }
	public var isReadOnly: Bool { workspace.readOnly != nil }
	/// Bill amounts are transaction amounts: cents, read as the register reads them.
	public var reader: any AmountReader { CoreAmountReader() }
	public var numberStyle: NumberStyle { workspace.numberStyle }
	public var today: String { WorkspaceModel.today() }

	public func load() async {
		let today = today
		async let names = workspace.budgetNames()
		let loaded = await workspace.report { session in
			async let bills = session.bills(today: today)
			async let tray = session.upcoming(days: Self.trayDays, today: today)
			async let timeline = session.upcoming(days: Self.timelineDays, today: today)
			return try await (bills, tray, timeline)
		}
		self.names = await names
		guard let loaded else { return }
		(bills, tray, timeline) = loaded
		if let pending = workspace.pendingBillID {
			workspace.pendingBillID = nil
			if let bill = bill(pending) { await edit(bill) }
		}
	}

	public func bill(_ id: String) -> BillSummary? {
		bills.first { $0.id == id }
	}

	public var activeBills: [BillSummary] { bills.filter { $0.state == .active } }
	public var stoppedBills: [BillSummary] { bills.filter { $0.state != .active } }

	/// The timeline in the core's order, split at each month.
	public var timelineMonths: [(month: String, bills: [UpcomingBill])] {
		var months: [(month: String, bills: [UpcomingBill])] = []
		for item in timeline {
			let month = String(item.date.prefix(7))
			if months.last?.month == month {
				months[months.count - 1].bills.append(item)
			} else {
				months.append((month, [item]))
			}
		}
		return months
	}

	/// "Checking → Rent": the account, then the category, or the payee when
	/// there's no category, such as a transfer. Income reads the other way.
	public func route(_ bill: BillSummary) -> String {
		let account = bill.accountName ?? names.account(bill.accountId) ?? "No account"
		guard let other = names.category(bill.categoryId) ?? bill.payeeName ?? names.payee(bill.payeeId) else { return account }
		return bill.isIncome ? "\(other) → \(account)" : "\(account) → \(other)"
	}

	public func startNew(isIncome: Bool = false) {
		let start = BillRepeat(start: today, interval: 1, frequency: .monthly, patterns: [], weekend: nil, end: .never)
		let draft = BillDraft(name: nil, payeeId: nil, accountId: nil, amount: .exact(amount: 0), date: .repeat(repeat: start), categoryId: nil, autoPost: false)
		editing = BillEditing(id: nil, draft: draft, isIncome: isIncome)
	}

	/// A bill Actual holds in a shape Veritas can't show has no draft and stays read-only.
	public func edit(_ bill: BillSummary) async {
		guard var draft = bill.draft else { return }
		draft.amount = draft.amount.magnitudes
		let today = today
		let detail = await workspace.report { try await $0.bill(bill.id, today: today) }
		editing = BillEditing(id: bill.id, draft: draft, isIncome: bill.isIncome, detail: detail)
	}

	public func cancelEditing() {
		editing = nil
	}

	@discardableResult
	public func save() async -> Bool {
		guard let editing else { return false }
		let command: Command = if let id = editing.id {
			.editBill(billId: id, bill: editing.signedDraft)
		} else {
			.createBill(billId: nil, bill: editing.signedDraft)
		}
		self.editing?.problem = nil
		let saved = await workspace.perform(command) { self.editing?.problem = $0 }
		if saved {
			self.editing = nil
			await load()
		}
		return saved
	}

	/// Paid, with the amount and date typed. `magnitude` takes the bill's sign.
	@discardableResult
	public func pay(_ bill: UpcomingBill, magnitude: Int64, date: String) async -> Bool {
		await act(bill.billId, .payBill(billId: bill.billId, amount: bill.isIncome ? magnitude : -magnitude, date: date))
	}

	@discardableResult
	public func skip(_ billId: String) async -> Bool { await act(billId, .skipBill(billId: billId)) }
	@discardableResult
	public func postNow(_ billId: String, datedToday: Bool) async -> Bool { await act(billId, .postBillNow(billId: billId, datedToday: datedToday)) }
	@discardableResult
	public func pause(_ billId: String) async -> Bool { await act(billId, .pauseBill(billId: billId)) }
	@discardableResult
	public func end(_ billId: String) async -> Bool { await act(billId, .endBill(billId: billId)) }
	@discardableResult
	public func resume(_ billId: String) async -> Bool { await act(billId, .resumeBill(billId: billId)) }

	private func act(_ billId: String, _ command: Command) async -> Bool {
		actionProblem = nil
		let done = await workspace.perform(command) { self.actionProblem = (billId, $0) }
		if done { await load() }
		return done
	}
}
