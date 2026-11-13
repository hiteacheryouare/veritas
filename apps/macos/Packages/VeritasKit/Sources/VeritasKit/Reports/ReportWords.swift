import VeritasCore

/// Report copy as Fred ruled it for the Gate 1 screens (docs/finance/statements.md
/// ST-1 and ST-8, CLS-1 "Hidden and archived"). Labels never filter anything out.
public enum ReportWords {
	public static let uncategorized = "Uncategorized (needs review)"
	public static let unreviewedNote = "No category. Positive means money left. Not in Net cash flow."
	public static let unallocatedCaption = "Income not yet spent, saved or paid toward debt"
	public static let unallocatedAction = "Show income and outflow totals"
	public static let shortfallCaption = "Money drawn from earlier balances"

	/// "Old Gym (hidden, archived)", "(deleted) Old Gym", "Uncategorized (needs review)".
	public static func category(_ name: String, labels: ReportLabels) -> String {
		if labels.needsReview || name.isEmpty { return uncategorized }
		var suffixes: [String] = []
		if labels.hidden { suffixes.append("hidden") }
		if labels.archived { suffixes.append("archived") }
		if labels.closedGoal { suffixes.append("closed goal") }
		var text = labels.deleted ? "(deleted) \(name)" : name
		if !suffixes.isEmpty { text += " (\(suffixes.joined(separator: ", ")))" }
		return text
	}

	public static func node(_ node: SankeyNode) -> String {
		switch node.key.kind {
		case .incomeCategory, .categoryGroup, .category:
			node.key.id == nil ? uncategorized : category(node.name, labels: node.labels)
		case .refundsAndWithdrawals: "Refunds and withdrawals"
		case .unreviewedTransfersIn: "Unreviewed transfers in"
		case .shortfall: "Shortfall"
		case .income: "Income"
		case .savings: "Savings"
		case .debtPaydown: "Debt paydown"
		case .incomeReversals: "Income reversals"
		case .unreviewedTransfers: "Unreviewed transfers"
		case .unallocated: "Unallocated"
		}
	}

	/// "Show 47 transactions", "Show 1 transaction", or `nil` when there's nothing to show.
	public static func showTransactions(_ count: UInt64) -> String? {
		guard count > 0 else { return nil }
		let number = Int(count).formatted(.number.grouping(.automatic))
		return count == 1 ? "Show 1 transaction" : "Show \(number) transactions"
	}

	/// The drilldown button for a Sankey node: Shortfall has none, Unallocated is derived.
	public static func drillAction(for node: SankeyNode) -> String? {
		switch node.key.kind {
		case .shortfall: nil
		case .unallocated: node.count > 0 ? unallocatedAction : nil
		default: showTransactions(node.count)
		}
	}

	public static func line(_ line: NetProfitLine) -> String {
		switch line {
		case .income: "Income"
		case .expenses: "Expenses"
		case .netProfit: "Net Profit"
		case .debtPaydown: "Debt paydown"
		case .savings: "Savings"
		case .netCashFlow: "Net cash flow"
		case .unreviewed: "Unreviewed transfers"
		}
	}
}

extension ReportTransaction: Identifiable {}
extension CustomReportItem: Identifiable {}
extension CustomReportGroup: Identifiable {}
extension DashboardPage: Identifiable {}
extension DashboardWidget: Identifiable {}
