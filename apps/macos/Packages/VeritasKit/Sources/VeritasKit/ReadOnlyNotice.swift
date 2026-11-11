import VeritasCore

public struct ReadOnlyNotice: Hashable, Sendable {
	public enum Action: Hashable, Sendable {
		case showIntegrityReport

		public var title: String {
			switch self {
			case .showIntegrityReport: "Show report"
			}
		}
	}

	public let message: String
	public let action: Action?
}

public extension ReadOnlyKind {
	var notice: ReadOnlyNotice {
		switch self {
		case .newerActualVersion:
			ReadOnlyNotice(message: "A newer version of Actual saved this budget, so it's read-only. Update Veritas to edit it.", action: nil)
		case .trackingBudget:
			ReadOnlyNotice(message: "This is a tracking budget, so it's read-only. Veritas edits envelope budgets only.", action: nil)
		case .openElsewhere:
			ReadOnlyNotice(message: "This budget is open in another window, so it's read-only here. Close it there to edit it.", action: nil)
		case .auditFailed:
			ReadOnlyNotice(message: "The integrity check found problems in this budget, so it's read-only.", action: .showIntegrityReport)
		case .newerVeritasSchema:
			ReadOnlyNotice(message: "A newer version of Veritas changed this budget, so it's read-only. Update Veritas to edit it.", action: nil)
		}
	}
}
