import AppKit
import PDFKit
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// The four statements in one PDF (R-REP-7).
@MainActor
struct StatementSetPDFTests {
	@Test func theSetHasEachStatementWithItsHeaderAndNumberedPages() async throws {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = StatementsModel(workspace: workspace, today: "2026-11-09")
		await model.show(.incomeStatement)
		await model.choose(.quarter)
		await model.previous()
		let documents = try #require(await model.statementSet())
		#expect(documents.map(\.kind) == [.incomeStatement, .ownersEquity, .balanceSheet, .cashFlow])
		#expect(documents[2].basis == "As of September 30, 2026, in USD")
		#expect(model.setFileName == "Stub Statements 2026-07 to 2026-09.pdf")
		let pdf = try #require(PDFDocument(data: StatementPDF.data(documents, title: "Stub Statements", format: model.format)))
		let pages = documents.map { StatementPDF.paginate($0, format: model.format).count }
		#expect(pdf.pageCount == pages.reduce(0, +))
		var first = 0
		for (document, count) in zip(documents, pages) {
			let page = try #require(pdf.page(at: first)?.string)
			#expect(page.contains(document.title + StatementDocument.footnoteMark) && page.contains("Stub"), "\(document.title) starts its own page")
			#expect(StatementsScreenshotTests.flattened(page).contains(StatementsScreenshotTests.flattened(StatementDocument.footnote)), "\(document.title) carries the footnote")
			#expect(page.contains("Page \(first + 1) of \(pdf.pageCount)"))
			first += count
		}
	}
}
