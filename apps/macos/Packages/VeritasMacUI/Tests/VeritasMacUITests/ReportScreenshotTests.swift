import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Reports screens on the stub budget, light and dark, into target/screenshots/reports.
@MainActor
struct ReportScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/reports")
		.standardizedFileURL
	static let november = Month(year: 2026, month: 11)

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	@Test func netProfit() async throws {
		let workspace = await workspace()
		let model = NetProfitModel(workspace: workspace, thisMonth: Self.november)
		await model.load()
		try render(NetProfitScreen(workspace: workspace, model: model), named: "net-profit", size: CGSize(width: 1100, height: 760))
		model.toggle(.expenses)
		try render(NetProfitScreen(workspace: workspace, model: model), named: "net-profit-categories", size: CGSize(width: 1100, height: 1100))
	}

	@Test func sankey() async throws {
		let workspace = await workspace()
		let model = SankeyModel(workspace: workspace, thisMonth: Self.november)
		await model.choose(.month)
		await model.previous()
		try render(SankeyScreen(workspace: workspace, model: model), named: "sankey", size: CGSize(width: 1100, height: 680))
	}

	@Test func dashboard() async throws {
		let workspace = await workspace()
		let model = DashboardModel(workspace: workspace)
		await model.load()
		try render(DashboardScreen(workspace: workspace, model: model), named: "dashboard", size: CGSize(width: 1200, height: 860))
		try render(DashboardScreen(workspace: workspace, model: model), named: "dashboard-full", size: CGSize(width: 1200, height: 1660))
		model.isEditing = true
		try render(DashboardScreen(workspace: workspace, model: model), named: "dashboard-editing", size: CGSize(width: 1200, height: 860))
	}

	@Test func widgetConfig() async throws {
		let workspace = await workspace()
		let model = DashboardModel(workspace: workspace)
		await model.load()
		let salary = try #require(model.sheet?.cards.first { $0.id == "salary" })
		try render(WidgetConfigSheet(card: salary, reports: []) { _ in }, named: "widget-config-summary")
		let notes = try #require(model.sheet?.cards.first { $0.id == "notes" })
		try render(WidgetConfigSheet(card: notes, reports: []) { _ in }, named: "widget-config-text")
	}

	@Test func standardReports() async throws {
		let workspace = await workspace()
		for kind in StandardReport.allCases {
			let model = CustomReportsModel(workspace: workspace, thisMonth: Self.november)
			await model.load()
			await model.show(.standard(kind))
			try render(CustomReportsScreen(workspace: workspace, model: model), named: "standard-\(kind.rawValue)", size: CGSize(width: 1200, height: 820))
		}
	}

	@Test func customReport() async throws {
		let workspace = await workspace()
		let model = CustomReportsModel(workspace: workspace, thisMonth: Self.november)
		await model.load()
		await model.show(.custom(id: "spending"))
		try render(CustomReportsScreen(workspace: workspace, model: model), named: "custom-report", size: CGSize(width: 1200, height: 820))
		await model.edit()
		model.draft?.settings.totalMode = false
		model.draft?.settings.graphType = "StackedBarGraph"
		model.draft?.conditions = [ReportCondition(field: "category", op: "isNot", value: "Rent"), ReportCondition(field: "amount", op: "gt", value: "20.00")]
		await model.refreshPreview()
		try render(CustomReportsScreen(workspace: workspace, model: model), named: "custom-report-editing", size: CGSize(width: 1340, height: 1000))
		model.cancelEditing()
		await model.startNew()
		try render(CustomReportsScreen(workspace: workspace, model: model), named: "custom-report-new", size: CGSize(width: 1340, height: 1000))
	}

	@Test func drilldown() async throws {
		let workspace = await workspace()
		let model = DrilldownModel(workspace: workspace, title: "Expenses, August 2026", drill: .netProfit(first: "2026-08-01", last: "2026-08-31", line: .expenses, category: nil))
		await model.load()
		try render(DrilldownSheet(model: model), named: "drilldown")
	}

	func render(_ view: some View, named name: String, size: CGSize? = nil, settle: TimeInterval = 0.4) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = size ?? host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(settle))
			if size == nil { host.frame.size = host.fittingSize }
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
