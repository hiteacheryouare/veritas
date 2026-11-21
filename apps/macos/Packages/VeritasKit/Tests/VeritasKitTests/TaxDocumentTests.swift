import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Tax summary PDF's words (R-REP-8): TAX-4's note, ST-10.3's brackets,
/// the conflict's buckets and TAX-8's limits.
struct TaxDocumentTests {
	static let labels = ReportLabels(hidden: true, archived: true, deleted: true, closedGoal: false, needsReview: false)

	static let summary = TaxSummary(
		year: 2027,
		buckets: [
			TaxBucketTotal(bucket: .charitable, inflow: false, total: -2_000, count: 1, negative: true),
			TaxBucketTotal(bucket: .medical, inflow: false, total: 3_000, count: 1, negative: false)
		],
		detail: [
			TaxDetailRow(transactionId: "refund", parentId: nil, date: "2027-02-03", bucket: .charitable, fromTag: false, amount: 2_000, counted: -2_000, category: "Old Gym", accountName: "Checking", payee: "Food Bank", categoryLabels: labels, notes: "Returned", conflicting: []),
			TaxDetailRow(transactionId: "pharmacy", parentId: nil, date: "2027-03-14", bucket: .medical, fromTag: true, amount: -3_000, counted: 3_000, category: nil, accountName: "Visa", payee: "Pharmacy", categoryLabels: nil, notes: "", conflicting: []),
			TaxDetailRow(transactionId: "lunch", parentId: nil, date: "2027-04-01", bucket: nil, fromTag: true, amount: -2_500, counted: nil, category: "Dining", accountName: "Visa", payee: "Cafe", categoryLabels: nil, notes: "#medical #education", conflicting: [.medical, .education])
		],
		conflicts: 1, summaryCsv: "", detailCsv: ""
	)

	@Test func theDocumentCarriesTheCoresFiguresAndWords() throws {
		let document = TaxDocument(summary: Self.summary, entity: "Household, LLC", currencyCode: "USD", format: MoneyFormat())
		#expect(document.basis == "Tax year 2027, in USD")
		#expect(document.fileName == "Household, LLC Tax summary 2027.pdf")
		let charitable = try #require(document.buckets.first)
		#expect(charitable.name == "Charitable giving")
		#expect(charitable.total == "\u{2212}20.00", "a true minus sign, never parentheses (ST-10.4)")
		#expect(charitable.note == "Refunds or reversals exceed what was counted.")
		#expect(charitable.rows.map(\.category) == ["(deleted) Old Gym (hidden, archived)"])
		#expect(charitable.rows.map(\.amount) == ["\u{2212}20.00"], "the counted amount")
		#expect(charitable.rows.map(\.note) == ["Returned"])
		#expect(charitable.rows.map(\.date) == ["Feb 3"])
		#expect(document.buckets[1].note == nil)
		#expect(document.buckets[1].rows.map(\.category) == [ReportWords.uncategorized])
		#expect(document.conflicts.map(\.note) == ["Tags name Medical and Education"])
		#expect(document.conflicts.map(\.amount) == ["\u{2212}25.00"], "a conflict shows its own amount")
	}

	@Test func theCopyNeverJudgesTheAmounts() {
		let words = TaxDocument.limits + [TaxDocument.footer, TaxDocument.title]
		for line in words {
			for banned in ["deductible", "taxable", "owed"] {
				#expect(!line.localizedCaseInsensitiveContains(banned), "\(line)")
			}
		}
		#expect(TaxDocument.footer == "Organized from your own categories and tags. Not tax advice.")
		#expect(TaxBuckets.list([.medical, .education, .hsa]) == "Medical, Education and HSA contributions")
	}
}
