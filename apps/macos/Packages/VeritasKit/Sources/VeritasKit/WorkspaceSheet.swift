public enum WorkspaceSheet: String, Identifiable, Sendable {
	case addTransaction
	case moveMoney
	case newBill
	case integrityReport

	public var id: Self { self }

	public var title: String {
		switch self {
		case .addTransaction: "Add transaction"
		case .moveMoney: "Move money"
		case .newBill: "New bill"
		case .integrityReport: "Integrity report"
		}
	}
}
