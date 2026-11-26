import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Wide reports scroll inside their own area: they never push the report
/// editor off screen or force the window wider than the detail column.
@MainActor
struct ReportLayoutTests {
	/// The detail column in the smallest window (900) beside the default
	/// sidebar. Dragging the sidebar wider stops at the detail's minimum.
	static let narrowest: CGFloat = 900 - Size.sidebar - 1
	/// Full screen on a 1440-point display beside the default sidebar.
	static let fullScreen: CGFloat = 1190
	static let november = Month(year: 2026, month: 11)

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	private func editingThirteenMonths(_ workspace: WorkspaceModel) async throws -> CustomReportsModel {
		let model = CustomReportsModel(workspace: workspace, thisMonth: Self.november)
		await model.load()
		await model.show(.custom(id: "spending"))
		await model.edit()
		model.draft?.settings.totalMode = false
		model.draft?.settings.graphType = "StackedBarGraph"
		model.setFixedRange(first: Month(year: 2025, month: 11), last: Self.november)
		await model.refreshPreview()
		let columns = try #require(model.shown?.data?.intervals.count)
		#expect(columns == 13)
		return model
	}

	@Test func aThirteenMonthReportKeepsItsEditorInsideTheWindow() async throws {
		let workspace = await workspace()
		let model = try await editingThirteenMonths(workspace)
		for width in [Self.narrowest, Self.fullScreen] {
			let (host, window) = Self.host(CustomReportsScreen(workspace: workspace, model: model), size: CGSize(width: width, height: 900)) { Self.editor(in: $0) != nil && Self.scrollsSideways(in: $0) }
			defer { window.close() }
			let editor = try #require(Self.editor(in: host), "width \(width)")
			let editorFrame = host.convert(editor.bounds, from: editor)
			#expect(editorFrame.maxX <= host.bounds.maxX + 0.5, "width \(width): the editor at \(editorFrame)")
			#expect(editorFrame.width >= 339, "width \(width): the editor keeps its width, \(editorFrame.width)")
			let name = try #require(Self.views(in: editor).first { ($0 as? NSTextField)?.isEditable == true })
			#expect(host.bounds.contains(host.convert(name.bounds, from: name)), "width \(width): the Name field is on screen")
			for scroll in Self.views(in: host).compactMap({ $0 as? NSScrollView }) {
				let frame = host.convert(scroll.bounds, from: scroll)
				#expect(frame.maxX <= host.bounds.maxX + 0.5, "width \(width): a scroll view at \(frame)")
			}
			#expect(Self.scrollsSideways(in: host), "width \(width): the 13 months scroll inside the table's card")
		}
		Self.expectFits(CustomReportsScreen(workspace: workspace, model: model), "editing 13 months")
	}

	@Test func screenshotsOfAWideReportWhileEditing() async throws {
		let workspace = await workspace()
		let model = try await editingThirteenMonths(workspace)
		for (width, name) in [(Self.narrowest, "narrow"), (Self.fullScreen, "full")] {
			let (host, window) = Self.host(CustomReportsScreen(workspace: workspace, model: model), size: CGSize(width: width, height: 900)) { Self.editor(in: $0) != nil && Self.scrollsSideways(in: $0) }
			defer { window.close() }
			try Self.capture(host, named: "custom-report-13-months-\(name)")
			let table = try #require(Self.views(in: host).compactMap { $0 as? NSScrollView }.first { ($0.documentView?.frame.width ?? 0) > $0.contentView.bounds.width + 1 })
			table.contentView.scroll(to: NSPoint(x: 360, y: 0))
			table.reflectScrolledClipView(table.contentView)
			Self.runLoop(host) { _ in table.contentView.bounds.minX >= 359 }
			try Self.capture(host, named: "custom-report-13-months-\(name)-scrolled")
		}
	}

	/// Net Profit's note runs the card's width under its line, and the lines
	/// below it stay level in the pinned column and the figures.
	@Test func screenshotsOfATableWithANoteUnderALine() throws {
		let figures = { (text: String) in (0..<7).map { _ in AnyView(Text(text).padding(.horizontal, Spacing.s4)) } }
		let lines = [
			TableLine(id: "transfers", height: 38, leading: AnyView(Text("Unreviewed transfers").padding(.leading, Spacing.s4)), cells: figures("120.00")),
			TableLine(id: "note", height: 40, rule: nil, span: AnyView(Text(ReportWords.unreviewedNote).font(.text(.caption)).foregroundStyle(.textSecondary).lineLimit(2).padding(.leading, 28).padding(.trailing, Spacing.s4))),
			TableLine(id: "total", height: 38, rule: .strong, leading: AnyView(Text("Net cash flow").bold().padding(.leading, Spacing.s4)), cells: figures("305.70"))
		]
		for (width, name) in [(1000.0, "wide"), (520.0, "narrow")] {
			let table = PinnedTable(headings: ["Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "6 months"], lines: lines).padding(Spacing.s5)
			let (host, window) = Self.host(table, size: CGSize(width: width, height: 260)) { name == "wide" || Self.scrollsSideways(in: $0) }
			defer { window.close() }
			try Self.capture(host, named: "table-with-note-\(name)")
		}
	}

	static func capture(_ host: NSView, named name: String) throws {
		try FileManager.default.createDirectory(at: ReportScreenshotTests.output, withIntermediateDirectories: true)
		host.layoutSubtreeIfNeeded()
		let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
		host.cacheDisplay(in: host.bounds, to: bitmap)
		let png = try #require(bitmap.representation(using: .png, properties: [:]))
		try png.write(to: ReportScreenshotTests.output.appending(path: "\(name)-light.png"))
	}

	@Test func aDashboardCardsReportKeepsItsEditorInsideTheWindow() async throws {
		let workspace = await workspace()
		let model = try await editingThirteenMonths(workspace)
		let (host, window) = Self.host(CustomReportPane(model: model) { _ in }, size: CGSize(width: Self.narrowest, height: 900)) { Self.editor(in: $0) != nil }
		defer { window.close() }
		let editor = try #require(Self.editor(in: host))
		let frame = host.convert(editor.bounds, from: editor)
		#expect(frame.maxX <= host.bounds.maxX + 0.5, "the editor at \(frame)")
	}

	@Test func everyReportScreenFitsTheNarrowestDetailColumn() async throws {
		let workspace = await workspace()
		for kind in StandardReport.allCases {
			let model = CustomReportsModel(workspace: workspace, thisMonth: Self.november)
			await model.load()
			await model.show(.standard(kind))
			await model.standard.choose(.year)
			Self.expectFits(CustomReportsScreen(workspace: workspace, model: model), "\(kind)")
		}
		let custom = CustomReportsModel(workspace: workspace, thisMonth: Self.november)
		await custom.load()
		await custom.show(.custom(id: "spending"))
		Self.expectFits(CustomReportsScreen(workspace: workspace, model: custom), "custom report")

		let netProfit = NetProfitModel(workspace: workspace, thisMonth: Self.november)
		await netProfit.choose(.year)
		netProfit.toggle(.expenses)
		Self.expectFits(NetProfitScreen(workspace: workspace, model: netProfit), "net profit")

		let sankey = SankeyModel(workspace: workspace, thisMonth: Self.november)
		await sankey.choose(.month)
		Self.expectFits(SankeyScreen(workspace: workspace, model: sankey), "sankey")

		let statements = StatementsModel(workspace: workspace, today: "2026-11-09")
		await statements.load()
		Self.expectFits(StatementsScreen(workspace: workspace, model: statements), "statements")

		let dashboard = DashboardModel(workspace: workspace)
		await dashboard.load()
		Self.expectFits(DashboardScreen(workspace: workspace, model: dashboard), "dashboard")
		for card in dashboard.sheet?.cards ?? [] {
			Self.expectFits(CardDetail(card: card, workspace: workspace, model: dashboard, configure: { _ in }, close: {}), "card \(card.id)")
		}
	}

	/// The narrowest the view lays itself out is what it would force the window to.
	static func expectFits(_ view: some View, _ label: String, sourceLocation: SourceLocation = #_sourceLocation) {
		let minimum = NSHostingController(rootView: view).sizeThatFits(in: CGSize(width: 1, height: 900)).width
		#expect(minimum <= narrowest, "\(label): \(minimum)", sourceLocation: sourceLocation)
	}

	/// Lays the view out in a window and runs the main loop until `ready`
	/// holds, or five seconds pass.
	static func host(_ view: some View, size: CGSize, until ready: (NSView) -> Bool) -> (NSView, NSWindow) {
		let host = NSHostingView(rootView: view.background(.surfaceWindow))
		host.frame.size = size
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.isReleasedWhenClosed = false
		window.appearance = NSAppearance(named: .aqua)
		window.contentView = host
		runLoop(host, until: ready)
		return (host, window)
	}

	static func runLoop(_ host: NSView, until ready: (NSView) -> Bool) {
		let deadline = Date.now.addingTimeInterval(5)
		host.layoutSubtreeIfNeeded()
		while !ready(host), Date.now < deadline {
			RunLoop.main.run(until: .now.addingTimeInterval(0.02))
			host.layoutSubtreeIfNeeded()
		}
		RunLoop.main.run(until: .now.addingTimeInterval(0.02))
		host.layoutSubtreeIfNeeded()
	}

	/// The report editor's form: the scroll view that holds its Name field.
	static func editor(in host: NSView) -> NSScrollView? {
		views(in: host).compactMap { $0 as? NSScrollView }.last { scroll in
			views(in: scroll).contains { ($0 as? NSTextField)?.isEditable == true }
		}
	}

	static func scrollsSideways(in host: NSView) -> Bool {
		views(in: host).compactMap { $0 as? NSScrollView }.contains { scroll in
			(scroll.documentView?.frame.width ?? 0) > scroll.contentView.bounds.width + 1
		}
	}

	static func views(in view: NSView) -> [NSView] {
		[view] + view.subviews.flatMap(views)
	}
}
