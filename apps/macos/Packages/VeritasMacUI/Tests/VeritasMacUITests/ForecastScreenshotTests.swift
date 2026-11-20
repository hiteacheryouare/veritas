import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// Forecast and What-ifs on the stub budget, light and dark, into
/// target/screenshots/forecast.
@MainActor
struct ForecastScreenshotTests {
	nonisolated static let today = "2026-11-19"
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/forecast")
		.standardizedFileURL

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	/// A what-if with a trip next March and one with a raise, both compared.
	private func withWhatIfs(_ workspace: WorkspaceModel) async throws -> WhatIfsModel {
		let whatIfs = WhatIfsModel(workspace: workspace, today: { Self.today })
		await whatIfs.load()
		let trip = NewWhatIf(model: whatIfs)
		trip.name = "Japan in March"
		let tripID = await trip.save(.oneTime(date: "2027-03-10", accountId: "checking", categoryId: "hobbies", amount: -400_000))
		#expect(tripID != nil)
		let raise = NewWhatIf(model: whatIfs)
		raise.name = "Raise from February"
		let raiseID = try #require(await raise.save(.incomeChange(accountId: "checking", categoryId: nil, change: .percent(percentBp: 500), startMonth: Month(year: 2027, month: 2), endMonth: nil)))
		#expect(await whatIfs.add(.goalContribution(goalId: "laptop", amount: 20_000, startMonth: Month(year: 2026, month: 12), endMonth: Month(year: 2027, month: 2)), to: raiseID, inline: { Issue.record("\($0)") }))
		return whatIfs
	}

	@Test func theBaseline() async throws {
		let workspace = await workspace()
		let model = ForecastModel(workspace: workspace, today: { Self.today })
		await model.load()
		#expect(model.forecast?.baseline.horizons.map(\.months) == [3, 6, 10, 12])
		try render(ForecastScreen(workspace: workspace, model: model), named: "forecast", size: CGSize(width: 1000, height: 2100))
	}

	@Test func comparingTwoWhatIfsAtSixMonths() async throws {
		let workspace = await workspace()
		_ = try await withWhatIfs(workspace)
		let model = ForecastModel(workspace: workspace, today: { Self.today })
		model.horizon = 6
		await model.load()
		#expect(model.projections.count == 3)
		try render(ForecastScreen(workspace: workspace, model: model), named: "forecast-compare", size: CGSize(width: 1000, height: 1300))
	}

	@Test func theWhatIfsPage() async throws {
		let workspace = await workspace()
		let whatIfs = try await withWhatIfs(workspace)
		try render(WhatIfsScreen(workspace: workspace, model: whatIfs), named: "what-ifs", size: CGSize(width: 900, height: 520))
	}

	@Test func theNewWhatIfSheet() async throws {
		let workspace = await workspace()
		let whatIfs = WhatIfsModel(workspace: workspace, today: { Self.today })
		await whatIfs.load()
		try render(NewWhatIfSheet(model: whatIfs), named: "new-what-if", size: CGSize(width: 520, height: 520))
	}

	func render(_ view: some View, named name: String, size: CGSize, settle: TimeInterval = 0.6) throws {
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
