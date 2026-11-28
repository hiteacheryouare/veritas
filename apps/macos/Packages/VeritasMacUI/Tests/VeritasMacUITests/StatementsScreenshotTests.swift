import AppKit
import PDFKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The four statements on the stub budget, light and dark, and their PDF,
/// into target/screenshots/insights.
@MainActor
struct StatementsScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/insights")
		.standardizedFileURL
	static let today = "2026-11-09"

	private func model(_ kind: StatementKind) async -> (WorkspaceModel, StatementsModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = StatementsModel(workspace: workspace, today: Self.today)
		await model.show(kind)
		if !kind.isDated {
			await model.choose(.quarter)
			await model.previous()
		}
		return (workspace, model)
	}

	@Test(arguments: StatementKind.allCases)
	func statement(_ kind: StatementKind) async throws {
		let (workspace, model) = await model(kind)
		let document = try #require(model.document)
		#expect(document.entity == "Stub")
		#expect(document.basis == (kind.isDated ? "As of November 9, 2026, in USD" : "July to September 2026, in USD"))
		try render(StatementsScreen(workspace: workspace, model: model), named: "statement-\(kind.rawValue)", size: CGSize(width: 900, height: kind == .incomeStatement ? 1120 : 820))
	}

	@Test func theBalanceSheetLaysOutAsTheMockupDoes() async throws {
		let (_, model) = await model(.balanceSheet)
		let lines = try #require(model.document).lines
		#expect(lines.map(\.title) == [
			"Assets", "Cash", "Checking", "Savings", "Old savings (closed)", "Total cash", "Investments", "Brokerage", "Total assets",
			"Liabilities", "Credit", "Visa", "Loans", "Car loan", "Total liabilities", "Net worth"
		])
	}

	@Test func ownersEquityHidesItsZeroLines() async throws {
		let (_, model) = await model(.ownersEquity)
		let titles = try #require(model.document).lines.map(\.title)
		#expect(titles == [
			"Opening equity", "Net income", "Off-budget assets acquired (expensed above)", "Reconciliation adjustments",
			"Off-budget revaluations", "Closing equity"
		])
	}

	/// ST-4: an equity statement that doesn't close, as the core would report it.
	private func unclosedEquity(_ unexplained: Int64) async throws -> (StatementDocument, MoneyFormat) {
		let (_, model) = await model(.ownersEquity)
		let document = try #require(model.document)
		#expect(document.unexplained == nil, "the stub closes")
		return (StatementDocument(kind: document.kind, entity: document.entity, basis: document.basis, lines: document.lines, unexplained: unexplained), model.format)
	}

	@Test func anUnexplainedGapIsANoticeBelowTheStatement() async throws {
		let (short, format) = try await unclosedEquity(1_000)
		#expect(short.notice(format: format) == "The lines above are $10.00 short of closing equity. Check the audit.")
		#expect(!short.lines.contains { $0.id == "unexplained" }, "never a line in the column")
		try render(
			StatementPaper(document: short, format: format).padding(Spacing.s5),
			named: "statement-ownersEquity-unexplained",
			size: CGSize(width: 900, height: 700)
		)
		let pdf = try #require(PDFDocument(data: StatementPDF.data(short, format: format)))
		let text = try #require(pdf.string)
		#expect(text.contains("The lines above are $10.00 short of closing equity."))
		#expect(text.contains("Check the audit."))
		let (over, _) = try await unclosedEquity(-1_000)
		#expect(over.notice(format: format) == "The lines above are $10.00 over closing equity. Check the audit.")
	}

	@Test func theNoticeTakesTheLastLineOntoANewPageWhenItDoesntFit() async throws {
		let (short, format) = try await unclosedEquity(1_000)
		let filler = (1...40).map { StatementLine(id: "filler.\($0)", title: "Line \($0)", amount: 100, style: .line) }
		let lines = Array(short.lines.dropLast()) + filler + [try #require(short.lines.last)]
		for count in 0...filler.count {
			let document = StatementDocument(kind: short.kind, entity: short.entity, basis: short.basis, lines: Array(lines.prefix(short.lines.count - 1 + count)) + [lines[lines.count - 1]], unexplained: 1_000)
			let pages = StatementPDF.paginate(document, format: format)
			#expect(pages.flatMap { $0 } == document.lines, "every line, once, in order")
			#expect(pages.allSatisfy { !$0.isEmpty })
			let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: format)))
			let found = try #require(pdf.findString("Check the audit.", withOptions: []).first)
			let page = try #require(found.pages.first)
			#expect(pdf.index(for: page) == pdf.pageCount - 1, "the notice is on the last page")
			#expect(found.bounds(for: page).minY >= PaperMetrics.bottom + StatementPDF.footer - 1, "above the footnote with \(count) extra lines")
		}
	}

	@Test func thePDFHasTheScreensLabelsAndNumbers() async throws {
		for kind in StatementKind.allCases {
			let (_, model) = await model(kind)
			let document = try #require(model.document)
			let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: model.format)))
			#expect(pdf.pageCount == StatementPDF.paginate(document, format: model.format).count, "\(kind)")
			#expect(pdf.pageCount == (kind == .incomeStatement ? 2 : 1), "\(kind)")
			let text = try #require(pdf.string)
			for expected in ["Stub", document.title + StatementDocument.footnoteMark, document.basis] {
				#expect(text.contains(expected), "\(kind): \(expected)")
			}
			for line in document.lines {
				#expect(text.contains(line.title), "\(kind): \(line.title)")
				if let amount = line.amount {
					#expect(text.contains(model.format.statement(amount, on: .register)), "\(kind): \(line.title)")
				}
			}
		}
	}

	@Test func aLongStatementRunsOntoMorePagesWithItsHeaderOnEach() async throws {
		let (_, model) = await model(.balanceSheet)
		let short = try #require(model.document)
		let accounts = (1...90).map { StatementLine(id: "account.\($0)", title: "Account \($0)", amount: Int64($0) * 10_000, style: .line) }
		let long = StatementDocument(
			kind: .balanceSheet, entity: short.entity, basis: short.basis,
			lines: [StatementLine(id: "assets", title: "Assets", style: .section)] + accounts + Array(short.lines.drop { $0.style != .total })
		)
		let pages = StatementPDF.paginate(long, format: model.format)
		#expect(pages.count > 1)
		#expect(pages.flatMap { $0 } == long.lines, "every line, once, in order")
		#expect(pages.dropLast().allSatisfy { page in page.last.map { $0.style != .section && $0.style != .group } ?? false }, "no heading ends a page")
		let pdf = try #require(PDFDocument(data: StatementPDF.data(long, format: model.format)))
		#expect(pdf.pageCount == pages.count)
		for index in 0..<pdf.pageCount {
			let text = try #require(pdf.page(at: index)?.string)
			#expect(text.contains("Balance sheet") && text.contains(short.basis), "page \(index + 1) has the header")
			#expect(Self.flattened(text).contains(Self.flattened(StatementDocument.footnote)), "page \(index + 1) has the footnote")
			#expect(text.contains("Page \(index + 1) of \(pdf.pageCount)"))
		}
		#expect(pdf.page(at: pdf.pageCount - 1)?.string?.contains("Net worth") == true)
	}

	@Test func aPDFPageAsAnImage() async throws {
		let (_, model) = await model(.balanceSheet)
		let document = try #require(model.document)
		let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: model.format)))
		let page = try #require(pdf.page(at: 0))
		let bounds = page.bounds(for: .mediaBox)
		let scale: CGFloat = 2
		let bitmap = try #require(NSBitmapImageRep(
			bitmapDataPlanes: nil, pixelsWide: Int(bounds.width * scale), pixelsHigh: Int(bounds.height * scale),
			bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
		))
		let context = try #require(NSGraphicsContext(bitmapImageRep: bitmap)).cgContext
		context.setFillColor(.white)
		context.fill(CGRect(x: 0, y: 0, width: bounds.width * scale, height: bounds.height * scale))
		context.scaleBy(x: scale, y: scale)
		page.draw(with: .mediaBox, to: context)
		let png = try #require(bitmap.representation(using: .png, properties: [:]))
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		try png.write(to: Self.output.appending(path: "statement-pdf-page-1.png"))
	}

	@Test func everyPDFPageCarriesTheFootnote() async throws {
		for kind in StatementKind.allCases {
			let (_, model) = await model(kind)
			let document = try #require(model.document)
			let pdf = try #require(PDFDocument(data: StatementPDF.data(document, format: model.format)))
			for index in 0..<pdf.pageCount {
				let text = Self.flattened(try #require(pdf.page(at: index)?.string))
				#expect(text.contains("\(StatementDocument.footnoteMark) \(Self.flattened(StatementDocument.footnote))"), "\(kind) page \(index + 1)")
			}
		}
	}

	/// PDF text breaks a wrapped footnote across lines.
	static func flattened(_ text: String) -> String {
		text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
	}

	func render(_ view: some View, named name: String, size: CGSize, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
