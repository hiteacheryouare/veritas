import VeritasCore

public extension NumberStyle {
	static let allCases: [NumberStyle] = [.commaDot, .dotComma, .spaceComma, .apostropheDot]

	/// How 1000.33 reads in this style, as Actual labels it.
	var sample: String {
		switch self {
		case .commaDot: "1,000.33"
		case .dotComma: "1.000,33"
		case .spaceComma: "1\u{202F}000,33"
		case .apostropheDot: "1'000.33"
		}
	}
}

/// One currency the budget can use, with the number style it usually uses.
public struct CurrencyOption: Hashable, Identifiable, Sendable {
	public let code: String
	public let name: String
	public let symbol: String
	public let numberStyle: NumberStyle

	public var id: String { code }

	public init(code: String, name: String, symbol: String, numberStyle: NumberStyle) {
		self.code = code
		self.name = name
		self.symbol = symbol
		self.numberStyle = numberStyle
	}
}

/// What kind of account a new one is. The core turns it into Actual's
/// off-budget flag and Veritas's account kind, which place it in its
/// sidebar group (ST-9.2, D8: cards on budget, loans off).
public enum AccountType: String, CaseIterable, Hashable, Sendable {
	case checking
	case savings
	case cash
	case creditCard
	case lineOfCredit
	case investment
	case retirement
	case property
	case loan
	case mortgage

	public var title: String {
		switch self {
		case .checking: "Checking"
		case .savings: "Savings"
		case .cash: "Cash"
		case .creditCard: "Credit card"
		case .lineOfCredit: "Line of credit"
		case .investment: "Investment"
		case .retirement: "Retirement"
		case .property: "Property"
		case .loan: "Loan"
		case .mortgage: "Mortgage"
		}
	}

	/// The sidebar group an account of this type lands in, which the picker groups by.
	public var group: VeritasCore.AccountGroup {
		switch self {
		case .checking, .savings, .cash: .cash
		case .creditCard, .lineOfCredit: .credit
		case .investment, .retirement, .property: .investments
		case .loan, .mortgage: .loans
		}
	}

	/// For these the field asks what's owed, as a positive amount.
	public var isOwed: Bool {
		group == .credit || group == .loans
	}
}

public extension VeritasCore.AccountGroup {
	static let allCases: [VeritasCore.AccountGroup] = [.cash, .credit, .investments, .loans]

	var title: String {
		switch self {
		case .cash: "Cash"
		case .credit: "Credit"
		case .investments: "Investments"
		case .loans: "Loans"
		}
	}
}

public struct StartingAccount: Hashable, Sendable {
	public let name: String
	public let type: AccountType
	/// What the account holds, or for an owed type what's owed, in cents.
	public let balance: Int64

	public init(name: String, type: AccountType, balance: Int64) {
		self.name = name
		self.type = type
		self.balance = balance
	}
}

/// Everything new-budget setup collects (R-IO-0). The core adds the starter
/// categories, and an off-budget account for each paycheck savings plan (D56).
public struct NewBudget: Hashable, Sendable {
	public let name: String
	public let currencyCode: String
	public let numberStyle: NumberStyle
	public let accounts: [StartingAccount]
	public let hasRetirementPlan: Bool
	public let hasHealthSavingsAccount: Bool

	public init(
		name: String,
		currencyCode: String,
		numberStyle: NumberStyle,
		accounts: [StartingAccount],
		hasRetirementPlan: Bool,
		hasHealthSavingsAccount: Bool
	) {
		self.name = name
		self.currencyCode = currencyCode
		self.numberStyle = numberStyle
		self.accounts = accounts
		self.hasRetirementPlan = hasRetirementPlan
		self.hasHealthSavingsAccount = hasHealthSavingsAccount
	}
}
