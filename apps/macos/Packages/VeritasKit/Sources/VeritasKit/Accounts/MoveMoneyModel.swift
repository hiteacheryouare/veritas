import Foundation
import Observation
import VeritasCore

/// Move money (R-XFER-1, design/mockups/move-money.html): one paired
/// transfer, both balances before and after, R-XFER-4's question when the
/// money leaves the budget, and the principal/interest split when it pays a
/// loan (R-DEBT-2). Editing a transfer opens the same sheet, since either
/// side edits the pair (R-XFER-3).
@MainActor
@Observable
public final class MoveMoneyModel: Identifiable {
	public struct PreviewKey: Hashable {
		let from: String
		let to: String?
		let amount: String
		let interest: String
		let escrow: String
		let fees: String
		let date: Date
	}

	public let id = UUID()
	public let editing: RegisterRow?
	public var fromID: String
	public var toID: String?
	public let amount: AmountFieldModel
	public let interest: AmountFieldModel
	public var interestCategoryID: String?
	public var principalCategoryID: String?
	/// DEBT-2's other parts, behind a disclosure.
	public let escrow: AmountFieldModel
	public let fees: AmountFieldModel
	public var escrowCategoryID: String?
	public var feesCategoryID: String?
	public var showsEscrowAndFees = false
	public var date: Date
	public var notes: String
	public let purpose: TransferPurposeModel
	public private(set) var accounts: [AccountDetail] = []
	public private(set) var envelopes: [EnvelopeGroup] = []
	public private(set) var preview: TransferPreview?
	/// False until an amount is typed: the preview then only says what kind of move it is.
	public private(set) var showsAfter = false
	public private(set) var loan: LoanPaymentPreview?
	/// The interest the last preview read. A 0 interest writes no interest line, so it needs no category.
	public private(set) var previewedInterest: Int64 = 0
	public private(set) var problem: String?

	@ObservationIgnored let workspace: WorkspaceModel
	@ObservationIgnored private let reader: any AmountReader
	@ObservationIgnored private var payees: [PayeeDetail] = []
	@ObservationIgnored private var amountTouched = false
	@ObservationIgnored private var defaultsFor: String?
	/// The leg written on save: the one leaving `from`, when it exists.
	@ObservationIgnored private var leg: RegisterRow?

	public init(workspace: WorkspaceModel, fromID: String, editing: RegisterRow? = nil, reader: any AmountReader = CoreAmountReader()) {
		self.workspace = workspace
		self.reader = reader
		self.editing = editing
		let outflow = (editing?.amount ?? -1) < 0
		if let editing, let other = editing.transferAccountId {
			self.fromID = outflow ? editing.accountId : other
			toID = outflow ? other : editing.accountId
		} else {
			self.fromID = fromID
			toID = nil
		}
		amount = AmountFieldModel(amount: abs(editing?.amount ?? 0), style: workspace.numberStyle, reader: reader)
		interest = AmountFieldModel(amount: 0, style: workspace.numberStyle, reader: reader)
		escrow = AmountFieldModel(amount: 0, style: workspace.numberStyle, reader: reader)
		fees = AmountFieldModel(amount: 0, style: workspace.numberStyle, reader: reader)
		date = editing.flatMap { TransactionEditorModel.date($0.date) } ?? .now
		notes = editing?.notes ?? ""
		purpose = TransferPurposeModel(workspace: workspace, categoryID: editing?.categoryId)
		amountTouched = editing != nil
		leg = outflow ? editing : nil
	}

	public var isNew: Bool { editing == nil }
	public var format: MoneyFormat { workspace.format }
	public var canPlan: Bool { workspace.canPlanTransfers }
	public var previewKey: PreviewKey {
		PreviewKey(from: fromID, to: toID, amount: amount.text, interest: interest.text, escrow: escrow.text, fees: fees.text, date: date)
	}

	public var isLoanPayment: Bool { preview?.toLoan == true }
	public var asksPurpose: Bool { preview?.purpose == .ask && !isLoanPayment }

	public func envelopeName(_ id: String) -> String? {
		envelopes.lazy.flatMap(\.envelopes).first { $0.id == id }?.name
	}

	public func account(_ id: String?) -> AccountDetail? {
		accounts.first { $0.id == id }
	}

	public var canSave: Bool {
		guard let toID, toID != fromID, workspace.readOnly == nil else { return false }
		if asksPurpose, !purpose.isAnswered { return false }
		if isLoanPayment, isNew {
			if case .interestAbovePayment? = loan?.problem { return false }
			if principalCategoryID == nil || (previewedInterest != 0 && interestCategoryID == nil) { return false }
		}
		return true
	}

	public func load() async {
		accounts = (await workspace.report { try await $0.accounts() })?.accounts.filter { !$0.closed || $0.id == editing?.accountId } ?? []
		payees = await workspace.report { try await $0.payees() } ?? []
		let month = Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1)
		envelopes = await workspace.budgetMonth(month)?.groups.filter { !$0.hidden } ?? []
		if let editing, editing.amount > 0, let counterpart = editing.counterpartId {
			leg = await workspace.report { try await $0.registerRow(counterpart) }
			if let category = leg?.categoryId { purpose.categoryID = category }
		}
		if canPlan { await purpose.load(envelopes: envelopes) }
		await startFromLoan()
	}

	/// Opened on a loan's register, Move money pays the loan (R-DEBT-2).
	private func startFromLoan() async {
		guard isNew, canPlan, toID == nil else { return }
		let loanID = fromID
		guard await workspace.report({ try await $0.loanTerms(loanID) }) ?? nil != nil else { return }
		let defaults = await workspace.report { try await $0.loanPaymentDefaults(loanID, today: WorkspaceModel.today()) }
		toID = loanID
		fromID = defaults?.fromAccountId ?? accounts.first { !$0.offBudget }?.id ?? loanID
	}

	public func typeAmount(_ text: String) {
		amountTouched = true
		amount.type(text)
	}

	/// The core's figures for what's typed. The view calls this whenever `previewKey` changes.
	public func refreshPreview() async {
		guard canPlan, let toID, toID != fromID else {
			preview = nil
			loan = nil
			return
		}
		let fromID = fromID
		let moved = max(await amount.peek() ?? 0, 0)
		// The core refuses a zero move, so an empty amount asks about one cent
		// to learn the purpose and whether it pays a loan, and hides After.
		preview = await workspace.report { try await $0.transferPreview(from: fromID, to: toID, amount: max(moved, 1)) }
		showsAfter = moved > 0
		guard isLoanPayment else {
			loan = nil
			return
		}
		if defaultsFor != toID {
			defaultsFor = toID
			await fill(from: toID)
		}
		let total = max(await amount.peek() ?? 0, 0)
		let interestPart = max(await interest.peek() ?? 0, 0)
		previewedInterest = interestPart
		let escrowPart = max(await escrow.peek() ?? 0, 0)
		let feesPart = max(await fees.peek() ?? 0, 0)
		let day = TransactionEditorModel.civilDate(date)
		loan = await workspace.report { try await $0.loanPaymentPreview(toID, date: day, total: total, interest: interestPart, escrow: escrowPart, fees: feesPart) }
	}

	/// DEBT-3: the last payment's figures, where nothing was typed yet.
	private func fill(from loanID: String) async {
		guard isNew, let defaults = await workspace.report({ try await $0.loanPaymentDefaults(loanID, today: WorkspaceModel.today()) }) else { return }
		let style = workspace.numberStyle
		if !amountTouched, let total = defaults.total { amount.type(reader.editText(total, style: style)) }
		if let part = defaults.interest { interest.type(reader.editText(part, style: style)) }
		if let part = defaults.escrow, part != 0 {
			escrow.type(reader.editText(part, style: style))
			showsEscrowAndFees = true
		}
		escrowCategoryID = escrowCategoryID ?? defaults.escrowCategoryId
		interestCategoryID = interestCategoryID ?? defaults.interestCategoryId
		principalCategoryID = principalCategoryID ?? defaults.principalCategoryId ?? purpose.fixedDebtEnvelope
	}

	@discardableResult
	public func save() async -> Bool {
		guard let toID, canSave else { return false }
		guard let moved = await amount.commit() else {
			problem = amount.problem
			return false
		}
		guard moved > 0 else {
			problem = "Enter an amount to move."
			return false
		}
		problem = nil
		let fromID = fromID
		let day = TransactionEditorModel.civilDate(date)
		let text = notes.trimmingCharacters(in: .whitespaces)
		let note = text.isEmpty ? nil : text
		if isLoanPayment, isNew {
			var parts: [Int64] = []
			for field in [interest, escrow, fees] {
				guard let part = await field.commit() else {
					problem = field.problem
					return false
				}
				parts.append(part)
			}
			guard let principalCategory = principalCategoryID, parts[0] == 0 || interestCategoryID != nil else { return false }
			// v10's draft needs an interest category, but the core reads it only for a
			// nonzero interest line (contract: each line only when its part isn't 0).
			// v12 (#72) makes it optional.
			let interestCategory = interestCategoryID ?? ""
			let payment = LoanPaymentDraft(
				id: UUID().uuidString.lowercased(), fromAccountId: fromID, loanAccountId: toID, date: day, total: moved,
				interest: parts[0], escrow: parts[1], fees: parts[2], principalCategoryId: principalCategory, interestCategoryId: interestCategory,
				escrowCategoryId: escrowCategoryID, feesCategoryId: feesCategoryID, notes: note, cleared: false
			)
			return await workspace.change { session -> Bool in
				try await session.recordLoanPayment(payment)
				return true
			} ?? false
		}
		// A one-sided transfer (edge case 4) has no leg leaving `from`, so it's written from its own side.
		let ownSide = leg == nil && editing != nil
		let account = ownSide ? toID : fromID
		let other = ownSide ? fromID : toID
		guard let payee = payees.first(where: { $0.transferAccountId == other }) else {
			problem = "\(self.account(other)?.name ?? "That account") has no transfer payee. Reopen the budget and try again."
			return false
		}
		let asks = asksPurpose && !ownSide
		let purpose = purpose
		let rowID = (leg ?? editing)?.id
		let cleared = (leg ?? editing)?.cleared ?? false
		let payeeID = payee.id
		return await workspace.change { session -> Bool in
			let category = asks ? try await purpose.resolve(using: session) : nil
			let draft = TransactionDraft(
				id: rowID == nil ? UUID().uuidString.lowercased() : nil,
				accountId: account, date: day, amount: ownSide ? moved : -moved, payeeId: payeeID, payeeName: nil,
				categoryId: category, notes: note, cleared: cleared, splits: []
			)
			if let rowID {
				try await session.perform(.editTransaction(transactionId: rowID, transaction: draft))
			} else {
				try await session.perform(.addTransaction(transaction: draft))
			}
			return true
		} ?? false
	}
}
