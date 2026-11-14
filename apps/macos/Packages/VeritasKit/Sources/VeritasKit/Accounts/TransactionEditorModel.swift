import Foundation
import Observation
import VeritasCore

/// One line of a split being edited.
@MainActor
@Observable
public final class SplitLineEditor: Identifiable {
	public let id: String
	/// `nil` for a line added in this edit.
	public let lineID: String?
	public let amount: AmountFieldModel
	public var categoryID: String?
	public var notes: String

	init(lineID: String?, amount: Int64, categoryID: String?, notes: String, style: NumberStyle, reader: any AmountReader) {
		id = lineID ?? UUID().uuidString
		self.lineID = lineID
		self.amount = AmountFieldModel(amount: amount, style: style, reader: reader)
		self.categoryID = categoryID
		self.notes = notes
	}
}

/// Quick entry and the transaction editor (R-ACC-2, R-ACC-4): payee with
/// memory, amount, envelope, account, date, notes, cleared, and splits that
/// must add up (edge case 6).
@MainActor
@Observable
public final class TransactionEditorModel: Identifiable {
	public let id = UUID()
	public let editing: RegisterRow?
	public var payeeText: String
	public var payeeID: String?
	public let amount: AmountFieldModel
	/// Money in. Off means money out, the usual case.
	public var isDeposit: Bool
	public var categoryID: String?
	public var accountID: String
	public var date: Date
	public var notes: String
	public var cleared: Bool
	public private(set) var splits: [SplitLineEditor] = []
	public private(set) var payees: [PayeeDetail] = []
	public private(set) var envelopes: [EnvelopeGroup] = []
	public private(set) var accounts: [AccountDetail] = []
	public private(set) var problem: String?
	/// Filled from the payee's last transaction, so the sheet can say so.
	public private(set) var remembered = false

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored let reader: any AmountReader
	@ObservationIgnored private var amountTouched = false

	public init(workspace: WorkspaceModel, accountID: String, editing: RegisterRow? = nil, reader: any AmountReader = CoreAmountReader()) {
		self.workspace = workspace
		self.reader = reader
		self.editing = editing
		self.accountID = editing?.accountId ?? accountID
		payeeText = editing?.payeeName ?? ""
		payeeID = editing?.payeeId
		amount = AmountFieldModel(amount: abs(editing?.amount ?? 0), style: workspace.numberStyle, reader: reader)
		isDeposit = (editing?.amount ?? -1) > 0
		categoryID = editing?.categoryId
		date = editing.flatMap { Self.date($0.date) } ?? .now
		notes = editing?.notes ?? ""
		cleared = editing?.cleared ?? false
		if let editing {
			splits = editing.lines.map { line in
				SplitLineEditor(lineID: line.id, amount: abs(line.amount), categoryID: line.categoryId, notes: line.notes ?? "", style: workspace.numberStyle, reader: reader)
			}
		}
	}

	public var format: MoneyFormat { workspace.format }
	public var isNew: Bool { editing == nil }
	public var isSplit: Bool { !splits.isEmpty }

	public func load() async {
		payees = await workspace.report { try await $0.payees() } ?? []
		accounts = (await workspace.report { try await $0.accounts() })?.accounts.filter { !$0.closed } ?? []
		let month = Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		envelopes = await workspace.budgetMonth(month)?.groups.filter { !$0.hidden } ?? []
	}

	/// Payees whose names start with what's typed, favorites first, then the
	/// other accounts as transfers.
	public var suggestions: [PayeeDetail] {
		let typed = payeeText.trimmingCharacters(in: .whitespaces)
		guard !typed.isEmpty, payeeID == nil || payees.first(where: { $0.id == payeeID })?.name != payeeText else { return [] }
		return payees
			.filter { $0.transferAccountId != accountID && $0.name.localizedStandardRange(of: typed)?.lowerBound == $0.name.startIndex }
			.sorted { ($0.favorite ? 0 : 1, $0.name) < ($1.favorite ? 0 : 1, $1.name) }
			.prefix(8)
			.map { $0 }
	}

	public func typePayee(_ text: String) {
		payeeText = text
		if let chosen = payees.first(where: { $0.id == payeeID }), chosen.name != text { payeeID = nil }
	}

	public func typeAmount(_ text: String) {
		amountTouched = true
		amount.type(text)
	}

	/// D38: a new transaction takes the payee's last envelope and amount, only
	/// where nothing was typed yet.
	public func choose(_ payee: PayeeDetail) async {
		payeeID = payee.id
		payeeText = payee.name
		guard isNew, payee.transferAccountId == nil else { return }
		let id = payee.id
		guard let memory = await workspace.report({ try await $0.payeeMemory(id) }) ?? nil else { return }
		if categoryID == nil { categoryID = memory.categoryId }
		if !amountTouched, amount.text == reader.editText(0, style: workspace.numberStyle) {
			amount.type(reader.editText(abs(memory.amount), style: workspace.numberStyle))
			isDeposit = memory.amount > 0
		}
		remembered = true
	}

	public var choseTransfer: Bool {
		payees.first { $0.id == payeeID }?.transferAccountId != nil
	}

	public func addSplitLine() {
		if splits.isEmpty {
			splits.append(SplitLineEditor(lineID: nil, amount: 0, categoryID: categoryID, notes: "", style: workspace.numberStyle, reader: reader))
		}
		splits.append(SplitLineEditor(lineID: nil, amount: 0, categoryID: nil, notes: "", style: workspace.numberStyle, reader: reader))
	}

	public func removeSplitLine(_ line: SplitLineEditor) {
		splits.removeAll { $0.id == line.id }
	}

	@discardableResult
	public func save() async -> Bool {
		guard let magnitude = await amount.commit() else {
			problem = amount.problem
			return false
		}
		let sign: Int64 = isDeposit ? 1 : -1
		var lines: [SplitLineDraft] = []
		for line in splits {
			guard let value = await line.amount.commit() else {
				problem = line.amount.problem
				return false
			}
			lines.append(SplitLineDraft(id: line.lineID, amount: sign * value, payeeId: nil, payeeName: nil, categoryId: line.categoryID, notes: line.notes.isEmpty ? nil : line.notes))
		}
		let typed = payeeText.trimmingCharacters(in: .whitespaces)
		let draft = TransactionDraft(
			id: isNew ? UUID().uuidString.lowercased() : nil,
			accountId: accountID,
			date: Self.civilDate(date),
			amount: sign * magnitude,
			payeeId: payeeID,
			payeeName: payeeID == nil && !typed.isEmpty ? typed : nil,
			categoryId: lines.isEmpty && !choseTransfer ? categoryID : nil,
			notes: notes.isEmpty ? nil : notes,
			cleared: cleared,
			splits: lines
		)
		let command: Command = if let editing {
			.editTransaction(transactionId: editing.id, transaction: draft)
		} else {
			.addTransaction(transaction: draft)
		}
		problem = nil
		return await workspace.perform(command)
	}

	nonisolated static func date(_ text: String) -> Date? {
		let parts = text.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3 else { return nil }
		return Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
	}

	public nonisolated static func civilDate(_ date: Date) -> String {
		let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
		return String(format: "%04d-%02d-%02d", parts.year ?? 2000, parts.month ?? 1, parts.day ?? 1)
	}
}
