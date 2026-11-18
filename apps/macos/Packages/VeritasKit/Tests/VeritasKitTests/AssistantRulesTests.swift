import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// Fred's rules for what the model may say (PR #81 review): tax totals never
/// become advice, reserved words stay with their figures, and scale notes travel with them.
struct AssistantRulesTests {
	@Test func theInstructionsCarryEachRuleWordForWord() {
		let instructions = AssistantPrompt.instructions(today: "2026-11-18")
		for rule in [
			"Tax buckets only total what the user tagged. Never say an amount is deductible, taxable or owed, and don't give tax advice.",
			"Use each tool's own words for its figures. Call a figure Income, Expenses, Spending or Net Profit only when the tool names it that. Never call a balance, a saved report's total or a budget figure by those words.",
			"If a result includes a note about scales, repeat it with the figures.",
			"A question about now, such as a current balance or net worth, uses today's date.",
			"Today is 2026-11-18."
		] {
			#expect(instructions.contains(rule), "\(rule)")
		}
	}

	@Test func aNegativeTaxBucketCarriesTheTax4Note() async throws {
		let summary = TaxSummary(
			year: 2026,
			buckets: [
				TaxBucketTotal(bucket: .charitable, inflow: false, total: 13_000, count: 2),
				TaxBucketTotal(bucket: .medical, inflow: false, total: -2_300, count: 1)
			],
			detail: [], conflicts: 0, summaryCsv: "", detailCsv: ""
		)
		let context = AssistantContext(queries: CannedQueries(tax: summary), format: MoneyFormat(), today: "2026-11-18")
		let text = try await TaxSummaryTool(context: context).call(arguments: .init(year: 2026))
		#expect(text.contains("\nMedical: \u{2212}23.00 from 1 transaction. Refunds or reversals exceed what was counted."))
		#expect(text.contains("\nCharitable giving: 130.00 from 2 transactions\n") || text.hasSuffix("\nCharitable giving: 130.00 from 2 transactions"))
		#expect(text.components(separatedBy: TaxBuckets.negativeNote).count == 2, "only the negative bucket has the note")
	}
}

/// Conforms to `ReadOnlyBudgetQueries` alone and answers from fixed values.
struct CannedQueries: ReadOnlyBudgetQueries {
	struct Missing: Error {}

	var tax: TaxSummary?
	var goalList: [GoalSummary]?

	func netProfit(first: Month, last: Month) async throws -> NetProfitReport { throw Missing() }
	func balanceSheet(asOf: String) async throws -> BalanceSheet { throw Missing() }
	func cashFlowStatement(first: String, last: String) async throws -> CashFlowColumn { throw Missing() }
	func budgetMonth(_ month: Month, today: String) async throws -> BudgetMonth { throw Missing() }
	func accounts() async throws -> AccountList { throw Missing() }
	func goals(today: String) async throws -> [GoalSummary] { guard let goalList else { throw Missing() }; return goalList }
	func taxSummary(year: Int32) async throws -> TaxSummary { guard let tax else { throw Missing() }; return tax }
	func customReports() async throws -> [CustomReportItem] { throw Missing() }
	func customReport(_ id: String, today: String) async throws -> CustomReportView { throw Missing() }
}
