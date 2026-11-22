import Foundation
import Testing
import VeritasCore
@testable import VeritasKit

/// The Tax summary model against the real core on fixtures/synthetic/all-features
/// (R-REP-8), with two category buckets, a tag over a category and a conflict
/// written into the budget's veritas tables.
@MainActor
struct RealCoreTaxTests {
	static let pharmacy = "c4978414-a33d-47a2-b87f-05fbdba2dd1f"
	static let conflicted = "b7e53a09-ba50-400d-b4a0-e75d6e43663c"
	static let gift = "cc4a9bf0-c7a0-407c-976e-874a1fb5d2e1"

	/// Interest is `interest_income` and Gifts `charitable`. A Groceries row is
	/// tagged `#medical`, and a Dining out row `#medical #education`.
	private func tagged(_ budget: RealBudget) async throws {
		let directory = try budget.directory
		await budget.workspace.closeBudget()
		try ActualDatabase.execute(directory.appending(path: "db.sqlite"), """
			INSERT INTO veritas_category_meta (category_id, tax_bucket) VALUES ('\(AllFeatures.interest)', 'interest_income'), ('\(AllFeatures.gifts)', 'charitable')
				ON CONFLICT (category_id) DO UPDATE SET tax_bucket = excluded.tax_bucket;
			INSERT OR REPLACE INTO veritas_tax_tag (tag, bucket) VALUES ('medical', 'medical'), ('education', 'education');
			UPDATE transactions SET notes = 'Pharmacy #medical' WHERE id = '\(Self.pharmacy)';
			UPDATE transactions SET notes = 'Lunch #medical #education' WHERE id = '\(Self.conflicted)';
			""")
		await budget.workspace.reopen(directory: directory)
		try #require(budget.workspace.phase == .open)
	}

	@Test func theModelShowsTheCoresBucketsDetailAndConflictsAsGiven() async throws {
		try await RealBudget.with { budget in
			try await tagged(budget)
			let model = TaxSummaryModel(workspace: budget.workspace, thisYear: 2026)
			await model.load()
			let summary = try #require(model.summary)
			let core = try #require(await budget.workspace.report { try await $0.taxSummary(year: 2026) })
			#expect(summary == core)
			#expect(model.buckets == core.buckets)
			#expect(model.buckets.map(\.bucket) == [.charitable, .interestIncome, .medical])

			let medical = model.rows(in: .medical)
			#expect(medical.map(\.transactionId) == [Self.pharmacy])
			#expect(medical.first?.fromTag == true)
			#expect(medical.first?.counted == 2_300)
			#expect(model.rows(in: .charitable).map(\.transactionId) == [Self.gift])
			for bucket in model.buckets {
				#expect(UInt64(model.rows(in: bucket.bucket).count) == bucket.count)
			}
			#expect(core.detail.filter { $0.bucket != nil } == model.buckets.flatMap { model.rows(in: $0.bucket) })

			#expect(model.conflicts.map(\.transactionId) == [Self.conflicted])
			#expect(model.conflicts.first?.counted == nil)
			#expect(UInt64(model.conflicts.count) == core.conflicts)
		}
	}

	@Test func aRefundBiggerThanThePurchaseLeavesTheBucketNegativeWithTheNote() async throws {
		// TAX-4: a +38.00 refund tagged #medical against the 23.00 pharmacy purchase reads -15.00, never clamped.
		try await RealBudget.with { budget in
			try await tagged(budget)
			#expect(await budget.workspace.perform(.addTransaction(transaction: TransactionDraft(
				id: nil, accountId: AllFeatures.checking, date: "2026-06-15", amount: 3_800, payeeId: nil, payeeName: "Pharmacy",
				categoryId: AllFeatures.groceries, notes: "Refund #medical", cleared: true, splits: []
			))))
			let model = TaxSummaryModel(workspace: budget.workspace, thisYear: 2026)
			await model.load()
			let summary = try #require(model.summary)
			let medical = try #require(model.buckets.first { $0.bucket == .medical })
			#expect(medical.total == -1_500)
			#expect(medical.count == 2)
			#expect(TaxBuckets.note(for: medical) == "Refunds or reversals exceed what was counted.")
			#expect(summary.summaryCsv.contains(",medical,outflow,-1500,"))
			for bucket in model.buckets where bucket.bucket != .medical {
				#expect(TaxBuckets.note(for: bucket) == nil, "\(bucket.bucket)")
			}
		}
	}

	@Test func eachExportWritesTheCoresCsvByteForByte() async throws {
		try await RealBudget.with { budget in
			try await tagged(budget)
			let model = TaxSummaryModel(workspace: budget.workspace, thisYear: 2026)
			await model.load()
			let summary = try #require(model.summary)
			#expect(summary.detailCsv.contains("conflict"))
			let folder = URL.temporaryDirectory.appending(path: "veritas-tax-\(UUID().uuidString)", directoryHint: .isDirectory)
			try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
			defer { try? FileManager.default.removeItem(at: folder) }
			for (export, csv) in [(TaxSummaryModel.Export.summary, summary.summaryCsv), (.detail, summary.detailCsv)] {
				#expect(model.fileName(export).hasSuffix("-2026.csv"))
				let file = folder.appending(path: model.fileName(export))
				#expect(model.export(export, to: file))
				#expect(try Data(contentsOf: file) == Data(csv.utf8), "\(export)")
			}
		}
	}

	@Test func anotherYearLoadsThatYear() async throws {
		try await RealBudget.with { budget in
			try await tagged(budget)
			let model = TaxSummaryModel(workspace: budget.workspace, thisYear: 2026)
			await model.load()
			model.toggle(.medical)
			await model.previous()
			#expect(model.year == 2025)
			#expect(model.summary?.year == 2025)
			#expect(model.expanded.isEmpty)
			#expect(model.summary == (await budget.workspace.report { try await $0.taxSummary(year: 2025) }))
			#expect(model.summary?.detail.contains { $0.transactionId == Self.pharmacy } == false)
			#expect(model.canGoForward)
		}
	}
}
