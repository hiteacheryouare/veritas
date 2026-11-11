/// Actual's number formats (`loot-core/src/shared/util.ts` `numberFormats`), stored per budget.
public enum NumberFormat: String, CaseIterable, Hashable, Sendable {
	case commaDot = "comma-dot"
	case dotComma = "dot-comma"
	case spaceComma = "space-comma"
	case apostropheDot = "apostrophe-dot"
	case commaDotIn = "comma-dot-in"

	/// How 1000.33 reads in this format, as Actual labels it.
	public var sample: String {
		switch self {
		case .commaDot: "1,000.33"
		case .dotComma: "1.000,33"
		case .spaceComma: "1\u{202F}000,33"
		case .apostropheDot: "1'000.33"
		case .commaDotIn: "1,00,000.33"
		}
	}
}

/// One currency the core supports, with the number format it usually uses.
public struct CurrencyOption: Hashable, Identifiable, Sendable {
	public let code: String
	public let name: String
	public let symbol: String
	public let numberFormat: NumberFormat

	public var id: String { code }

	public init(code: String, name: String, symbol: String, numberFormat: NumberFormat) {
		self.code = code
		self.name = name
		self.symbol = symbol
		self.numberFormat = numberFormat
	}
}

public struct StartingAccount: Hashable, Sendable {
	public let name: String
	public let balance: Int64
	public let isOffBudget: Bool

	public init(name: String, balance: Int64, isOffBudget: Bool) {
		self.name = name
		self.balance = balance
		self.isOffBudget = isOffBudget
	}
}

/// Everything new-budget setup collects (R-IO-0). The core adds the starter
/// categories, and an off-budget account for each paycheck savings plan (D56).
public struct NewBudget: Hashable, Sendable {
	public let name: String
	public let currencyCode: String
	public let numberFormat: NumberFormat
	public let accounts: [StartingAccount]
	public let hasRetirementPlan: Bool
	public let hasHealthSavingsAccount: Bool

	public init(
		name: String,
		currencyCode: String,
		numberFormat: NumberFormat,
		accounts: [StartingAccount],
		hasRetirementPlan: Bool,
		hasHealthSavingsAccount: Bool
	) {
		self.name = name
		self.currencyCode = currencyCode
		self.numberFormat = numberFormat
		self.accounts = accounts
		self.hasRetirementPlan = hasRetirementPlan
		self.hasHealthSavingsAccount = hasHealthSavingsAccount
	}
}
