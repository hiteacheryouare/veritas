import AppKit
import PDFKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Tax summary PDF (R-REP-8): the header, TAX-4's note, the conflict,
/// ST-10.3's brackets, TAX-8's limits and TAX-5's footer on every page.
@MainActor
struct TaxPDFTests {
	private func document() async throws -> TaxDocument {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = TaxSummaryModel(workspace: workspace, thisYear: 2026)
		await model.load()
		return try #require(model.document)
	}

	@Test func thePDFHasTheSummaryRowsAndLimits() async throws {
		let document = try await document()
		let pdf = try #require(PDFDocument(data: TaxPDF.data(document)))
		let text = try #require(pdf.string).split(whereSeparator: \.isWhitespace).joined(separator: " ")
		for expected in [
			"Stub", "Tax summary", "Tax year 2026, in USD", "Summary", "Charitable giving", "130.00",
			"Business expenses", "\u{2212}15.00", "Refunds or reversals exceed what was counted.",
			"Needs a decision", "Tags name Medical and Education", "Known limits"
		] + TaxDocument.limits {
			#expect(text.contains(expected), "\(expected)")
		}
		for bucket in document.buckets {
			for row in bucket.rows {
				#expect(text.contains(row.amount), "\(bucket.name): \(row.id)")
			}
		}
		for index in 0..<pdf.pageCount {
			let page = try #require(pdf.page(at: index)?.string)
			#expect(page.contains(TaxDocument.footer), "page \(index + 1)")
			#expect(page.contains("Stub") && page.contains("Tax summary"), "page \(index + 1) has the header")
		}
		for banned in ["deductible", "taxable", "owed"] {
			#expect(!text.localizedCaseInsensitiveContains(banned))
		}
	}

	@Test func eachPageAsAnImage() async throws {
		let pdf = try #require(PDFDocument(data: TaxPDF.data(try await document())))
		let output = URL(filePath: #filePath).deletingLastPathComponent().appending(path: "../../../../../../target/screenshots/tax").standardizedFileURL
		try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
		for index in 0..<pdf.pageCount {
			let page = try #require(pdf.page(at: index))
			let image = page.thumbnail(of: CGSize(width: 1224, height: 1584), for: .mediaBox)
			let png = try #require(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:))?.representation(using: .png, properties: [:]))
			try png.write(to: output.appending(path: "tax-pdf-page-\(index + 1).png"))
		}
	}

	@Test func categoryLabelsKeepTheirBrackets() throws {
		let labels = ReportLabels(hidden: true, archived: false, deleted: true, closedGoal: false, needsReview: false)
		let summary = TaxSummary(
			year: 2026,
			buckets: [TaxBucketTotal(bucket: .medical, inflow: false, total: 4_500, count: 1, negative: false)],
			detail: [TaxDetailRow(transactionId: "doctor", parentId: nil, date: "2026-02-11", bucket: .medical, fromTag: false, amount: -4_500, counted: 4_500, category: "Old Gym", accountName: "Checking", payee: "Clinic", categoryLabels: labels, notes: "", conflicting: [])],
			conflicts: 0, summaryCsv: "", detailCsv: ""
		)
		let document = TaxDocument(summary: summary, entity: "Home", currencyCode: nil, format: MoneyFormat())
		let text = try #require(PDFDocument(data: TaxPDF.data(document))?.string)
		#expect(text.contains("(deleted) Old Gym (hidden)"))
		#expect(text.contains("Clinic"))
	}

	@Test func manyRowsRunOntoNumberedPages() throws {
		let rows = (1...120).map { index in
			TaxDetailRow(transactionId: "row-\(index)", parentId: nil, date: "2026-03-01", bucket: .charitable, fromTag: false, amount: -100, counted: 100, category: "Donations", accountName: "Checking", payee: "Payee \(index)", categoryLabels: nil, notes: "", conflicting: [])
		}
		let summary = TaxSummary(year: 2026, buckets: [TaxBucketTotal(bucket: .charitable, inflow: false, total: 12_000, count: 120, negative: false)], detail: rows, conflicts: 0, summaryCsv: "", detailCsv: "")
		let document = TaxDocument(summary: summary, entity: "Home", currencyCode: "USD", format: MoneyFormat())
		let pdf = try #require(PDFDocument(data: TaxPDF.data(document)))
		#expect(pdf.pageCount > 1)
		let text = try #require(pdf.string)
		for index in 1...120 {
			#expect(text.contains("Payee \(index)"), "every row, once")
		}
		for index in 0..<pdf.pageCount {
			#expect(pdf.page(at: index)?.string?.contains("Page \(index + 1) of \(pdf.pageCount)") == true)
		}
	}

	@Test func anEmptyYearSaysSo() throws {
		let summary = TaxSummary(year: 2024, buckets: [], detail: [], conflicts: 0, summaryCsv: "", detailCsv: "")
		let text = try #require(PDFDocument(data: TaxPDF.data(TaxDocument(summary: summary, entity: "Home", currencyCode: nil, format: MoneyFormat())))?.string)
		#expect(text.contains("No transactions in a tax bucket in 2024"))
		#expect(text.contains(TaxDocument.footer))
	}
}
