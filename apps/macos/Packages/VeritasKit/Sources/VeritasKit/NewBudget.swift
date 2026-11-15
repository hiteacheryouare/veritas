import VeritasCore

public extension NumberStyle {
	static let allCases: [NumberStyle] = [.commaDot, .dotComma, .spaceComma, .apostropheDot, .commaDotIn]

	/// How 1000.33 reads in this style, as Actual labels it.
	var sample: String {
		switch self {
		case .commaDot: "1,000.33"
		case .dotComma: "1.000,33"
		case .spaceComma: "1\u{202F}000,33"
		case .apostropheDot: "1'000.33"
		case .commaDotIn: "1,00,000.33"
		}
	}
}

/// One currency the budget can use, with the number style it usually uses.
public struct CurrencyOption: Hashable, Identifiable, Sendable {
	public let code: String
	public let name: String
	public let symbol: String
	public let numberStyle: NumberStyle
	public let decimalPlaces: UInt8

	public var id: String { code }

	public init(code: String, name: String, symbol: String, numberStyle: NumberStyle, decimalPlaces: UInt8 = 2) {
		self.code = code
		self.name = name
		self.symbol = symbol
		self.numberStyle = numberStyle
		self.decimalPlaces = decimalPlaces
	}
}

public extension NewAccountKind {
	static let allCases: [NewAccountKind] = [
		.checking, .savings, .cash, .creditCard, .lineOfCredit, .investment, .retirement, .property, .loan, .mortgage
	]

	var title: String {
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

	/// Where the core places an account of this kind (ST-9.2, D8). The picker
	/// groups by it so the choice shows where the account will appear.
	var group: VeritasCore.AccountGroup {
		switch self {
		case .checking, .savings, .cash: .cash
		case .creditCard, .lineOfCredit: .credit
		case .investment, .retirement, .property: .investments
		case .loan, .mortgage: .loans
		}
	}

	/// The core takes what's owed on these as a positive amount.
	var isOwed: Bool {
		group == .credit || group == .loans
	}
}

public extension VeritasCore.AccountGroup {
	static let allCases: [VeritasCore.AccountGroup] = [.cash, .credit, .investments, .loans]

	var title: String {
		switch self {
		case .cash: "Cash"
		case .credit: "Credit"
		case .investments: "Investments & Other assets"
		case .loans: "Loans"
		}
	}
}

public struct StartingAccount: Hashable, Sendable {
	public let name: String
	public let type: NewAccountKind
	/// What the account holds, or for an owed type what's owed, in cents.
	public let balance: Int64

	public init(name: String, type: NewAccountKind, balance: Int64) {
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
