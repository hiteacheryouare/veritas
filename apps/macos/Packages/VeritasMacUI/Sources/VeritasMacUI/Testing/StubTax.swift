#if DEBUG
import Foundation
import VeritasCore

/// The stub budget's tax summaries: 2026 with six buckets, a refund that
/// leaves Business expenses negative and one conflict, and a quieter 2025.
/// The CSVs are written out as the core writes them (TAX-5).
enum StubTax {
	static func summary(year: Int32) throws -> TaxSummary {
		switch year {
		case 2026: y2026
		case 2025: y2025
		default: TaxSummary(year: year, buckets: [], detail: [], conflicts: 0, summaryCsv: summaryHeader, detailCsv: detailHeader)
		}
	}

	static let summaryHeader = "tax_year,bucket,direction,total_cents,total,transaction_count\n"
	static let detailHeader = "tax_year,bucket,source,date,account,payee,category,category_status,amount_cents,counted_cents,notes,transaction_id,parent_id\n"

	private static func row(_ id: String, _ date: String, _ bucket: TaxBucket?, tag: Bool = false, _ amount: Int64, _ counted: Int64?, _ category: String, conflicting: [TaxBucket] = []) -> TaxDetailRow {
		TaxDetailRow(
			transactionId: id, parentId: nil, date: date, bucket: bucket, fromTag: tag, amount: amount, counted: counted, category: category,
			accountName: "Checking", payee: "", categoryLabels: nil, notes: "", conflicting: conflicting
		)
	}

	static let y2026 = TaxSummary(
		year: 2026,
		buckets: [
			TaxBucketTotal(bucket: .businessExpense, inflow: false, total: -1_500, count: 1, negative: true),
			TaxBucketTotal(bucket: .charitable, inflow: false, total: 13_000, count: 3),
			TaxBucketTotal(bucket: .hsa, inflow: false, total: 40_000, count: 2),
			TaxBucketTotal(bucket: .interestIncome, inflow: true, total: 3_810, count: 3),
			TaxBucketTotal(bucket: .medical, inflow: false, total: 6_800, count: 2),
			TaxBucketTotal(bucket: .w2Income, inflow: true, total: 625_000, count: 2)
		],
		detail: [
			row("tax-supplies-refund", "2026-04-03", .businessExpense, 1_500, -1_500, "Supplies"),
			row("tax-donation-march", "2026-03-14", .charitable, -10_000, 10_000, "Donations"),
			row("tax-donation-june", "2026-06-02", .charitable, -5_000, 5_000, "Donations"),
			row("tax-donation-refund", "2026-09-20", .charitable, 2_000, -2_000, "Donations"),
			row("tax-glasses", "2026-05-22", nil, tag: true, -2_500, nil, "Shopping", conflicting: [.medical, .education]),
			row("tax-hsa-january", "2026-01-15", .hsa, tag: true, -20_000, 20_000, "Health savings"),
			row("tax-hsa-july", "2026-07-15", .hsa, tag: true, -20_000, 20_000, "Health savings"),
			row("tax-interest-march", "2026-03-31", .interestIncome, 1_240, 1_240, "Interest"),
			row("tax-interest-june", "2026-06-30", .interestIncome, 1_270, 1_270, "Interest"),
			row("tax-interest-september", "2026-09-30", .interestIncome, 1_300, 1_300, "Interest"),
			row("tax-doctor", "2026-02-11", .medical, -4_500, 4_500, "Medical"),
			row("tax-pharmacy", "2026-08-08", .medical, tag: true, -2_300, 2_300, "Groceries"),
			row("tax-salary-15", "2026-10-15", .w2Income, 312_500, 312_500, "Salary"),
			row("tax-salary-30", "2026-10-30", .w2Income, 312_500, 312_500, "Salary")
		],
		conflicts: 1,
		summaryCsv: summaryHeader + """
			2026,business_expense,outflow,-1500,-15.00,1
			2026,charitable,outflow,13000,130.00,3
			2026,hsa,outflow,40000,400.00,2
			2026,interest_income,inflow,3810,38.10,3
			2026,medical,outflow,6800,68.00,2
			2026,w2_income,inflow,625000,6250.00,2

			""",
		detailCsv: detailHeader + """
			2026,business_expense,category,2026-04-03,Visa Card,Office Depot,Supplies,,1500,-1500,Returned toner,tax-supplies-refund,
			2026,charitable,category,2026-03-14,Checking,Red Cross,Donations,,-10000,10000,,tax-donation-march,
			2026,charitable,category,2026-06-02,Checking,Food Bank,Donations,,-5000,5000,,tax-donation-june,
			2026,charitable,category,2026-09-20,Checking,Red Cross,Donations,,2000,-2000,Duplicate gift refunded,tax-donation-refund,
			2026,conflict,tag,2026-05-22,Visa Card,Campus Optical,Shopping,,-2500,,"Glasses, for class #medical #education",tax-glasses,
			2026,hsa,tag,2026-01-15,Checking,HSA,Health savings,,-20000,20000,#hsa,tax-hsa-january,
			2026,hsa,tag,2026-07-15,Checking,HSA,Health savings,,-20000,20000,#hsa,tax-hsa-july,
			2026,interest_income,category,2026-03-31,Savings,First Bank,Interest,,1240,1240,,tax-interest-march,
			2026,interest_income,category,2026-06-30,Savings,First Bank,Interest,,1270,1270,,tax-interest-june,
			2026,interest_income,category,2026-09-30,Savings,First Bank,Interest,,1300,1300,,tax-interest-september,
			2026,medical,category,2026-02-11,Visa Card,Dr. Patel,Medical,,-4500,4500,,tax-doctor,
			2026,medical,tag,2026-08-08,Visa Card,Corner Pharmacy,Groceries,,-2300,2300,Prescription #medical,tax-pharmacy,
			2026,w2_income,category,2026-10-15,Checking,Acme Corp,Salary,,312500,312500,,tax-salary-15,
			2026,w2_income,category,2026-10-30,Checking,Acme Corp,Salary,,312500,312500,,tax-salary-30,

			"""
	)

	static let y2025 = TaxSummary(
		year: 2025,
		buckets: [
			TaxBucketTotal(bucket: .charitable, inflow: false, total: 10_000, count: 2),
			TaxBucketTotal(bucket: .interestIncome, inflow: true, total: 2_390, count: 2)
		],
		detail: [
			row("tax-donation-november-2025", "2025-11-28", .charitable, -7_500, 7_500, "Donations"),
			row("tax-donation-december-2025", "2025-12-20", .charitable, -2_500, 2_500, "Donations"),
			row("tax-interest-september-2025", "2025-09-30", .interestIncome, 1_180, 1_180, "Interest"),
			row("tax-interest-december-2025", "2025-12-31", .interestIncome, 1_210, 1_210, "Interest")
		],
		conflicts: 0,
		summaryCsv: summaryHeader + """
			2025,charitable,outflow,10000,100.00,2
			2025,interest_income,inflow,2390,23.90,2

			""",
		detailCsv: detailHeader + """
			2025,charitable,category,2025-11-28,Checking,Food Bank,Donations,,-7500,7500,,tax-donation-november-2025,
			2025,charitable,category,2025-12-20,Checking,Red Cross,Donations,,-2500,2500,,tax-donation-december-2025,
			2025,interest_income,category,2025-09-30,Savings,First Bank,Interest,,1180,1180,,tax-interest-september-2025,
			2025,interest_income,category,2025-12-31,Savings,First Bank,Interest,,1210,1210,,tax-interest-december-2025,

			"""
	)
}
#endif
