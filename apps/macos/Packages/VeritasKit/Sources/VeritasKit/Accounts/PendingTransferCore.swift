import Foundation

// Shapes asked of Session for contract v9 (ws/session-6). Delete this file
// when the generated bindings carry them.

public struct BalanceAfter: Hashable, Sendable {
	public var accountId: String
	public var now: Int64
	public var after: Int64

	public init(accountId: String, now: Int64, after: Int64) {
		self.accountId = accountId
		self.now = now
		self.after = after
	}
}

public enum TransferPurpose: Hashable, Sendable {
	case none
	/// On budget to off budget: Veritas asks what the money is for (R-XFER-4).
	case ask
}

public struct TransferPreview: Hashable, Sendable {
	public var from: BalanceAfter
	public var to: BalanceAfter
	public var purpose: TransferPurpose
	/// `to` is a loan (ST-9.1) paid from an on-budget account.
	public var toLoan: Bool

	public init(from: BalanceAfter, to: BalanceAfter, purpose: TransferPurpose, toLoan: Bool) {
		self.from = from
		self.to = to
		self.purpose = purpose
		self.toLoan = toLoan
	}
}

public enum CategoryKind: Hashable, Sendable {
	case income
	case expense
	case debtPrincipal
	case savings
}

/// CLS-4.1: where a debt payment's principal or a savings move is filed.
public enum KindTarget: Hashable, Sendable {
	case none
	case one(categoryId: String)
	case several(categoryIds: [String])
}

/// R-DEBT-1. Every field is optional.
public struct LoanTerms: Hashable, Sendable {
	public var rateBps: Int64?
	public var payment: Int64?
	public var paymentDay: UInt8?
	public var lender: String?

	public init(rateBps: Int64? = nil, payment: Int64? = nil, paymentDay: UInt8? = nil, lender: String? = nil) {
		self.rateBps = rateBps
		self.payment = payment
		self.paymentDay = paymentDay
		self.lender = lender
	}
}

public enum LoanPaymentProblem: Hashable, Sendable {
	/// DEBT-2.2: blocks saving.
	case interestAbovePayment(shortBy: Int64)
	/// DEBT-2.3: a warning.
	case paysOff(credit: Int64)
}

public struct LoanPaymentPreview: Hashable, Sendable {
	public var principal: Int64
	public var outstanding: Int64
	/// DEBT-4's hint, when the loan has a rate.
	public var estimate: Int64?
	public var rateBps: Int64?
	public var problem: LoanPaymentProblem?

	public init(principal: Int64, outstanding: Int64, estimate: Int64?, rateBps: Int64?, problem: LoanPaymentProblem?) {
		self.principal = principal
		self.outstanding = outstanding
		self.estimate = estimate
		self.rateBps = rateBps
		self.problem = problem
	}
}

/// DEBT-3's pre-fill.
public struct LoanPaymentDefaults: Hashable, Sendable {
	public var fromAccountId: String?
	public var total: Int64?
	public var interest: Int64?
	public var escrow: Int64?
	public var principalCategoryId: String?
	public var interestCategoryId: String?
	public var escrowCategoryId: String?

	public init(fromAccountId: String? = nil, total: Int64? = nil, interest: Int64? = nil, escrow: Int64? = nil, principalCategoryId: String? = nil, interestCategoryId: String? = nil, escrowCategoryId: String? = nil) {
		self.fromAccountId = fromAccountId
		self.total = total
		self.interest = interest
		self.escrow = escrow
		self.principalCategoryId = principalCategoryId
		self.interestCategoryId = interestCategoryId
		self.escrowCategoryId = escrowCategoryId
	}
}

/// The core builds DEBT-6's split from this.
public struct LoanPaymentDraft: Hashable, Sendable {
	public var id: String?
	public var fromAccountId: String
	public var loanAccountId: String
	public var date: String
	public var total: Int64
	public var interest: Int64
	public var escrow: Int64
	public var fees: Int64
	public var principalCategoryId: String?
	public var interestCategoryId: String?
	public var escrowCategoryId: String?
	public var feesCategoryId: String?
	public var notes: String?
	public var cleared: Bool

	public init(id: String?, fromAccountId: String, loanAccountId: String, date: String, total: Int64, interest: Int64, escrow: Int64 = 0, fees: Int64 = 0, principalCategoryId: String?, interestCategoryId: String?, escrowCategoryId: String? = nil, feesCategoryId: String? = nil, notes: String? = nil, cleared: Bool = false) {
		self.id = id
		self.fromAccountId = fromAccountId
		self.loanAccountId = loanAccountId
		self.date = date
		self.total = total
		self.interest = interest
		self.escrow = escrow
		self.fees = fees
		self.principalCategoryId = principalCategoryId
		self.interestCategoryId = interestCategoryId
		self.escrowCategoryId = escrowCategoryId
		self.feesCategoryId = feesCategoryId
		self.notes = notes
		self.cleared = cleared
	}
}
