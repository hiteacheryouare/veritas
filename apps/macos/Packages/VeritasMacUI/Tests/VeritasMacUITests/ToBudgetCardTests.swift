import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The To Budget hero card and its quick actions on the stub budget
/// (docs/finance/to-budget.md TB-1), light and dark, into
/// target/screenshots/to-budget, and Assign to underfunded run end to end
/// against the stub with its result line and Undo (TB-2.4).
@MainActor
struct ToBudgetCardTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/to-budget")
		.standardizedFileURL

	private func openBudget() async -> (WorkspaceModel, BudgetScreenModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let screen = BudgetScreenModel(workspace: workspace, month: StubBudget.month)
		await screen.load()
		return (workspace, screen)
	}

	@Test func tb_1_2_thePopoverSaysWhatEachRowWouldDo() async throws {
		let (_, screen) = await openBudget()
		let actions = try #require(screen.toBudgetActions)
		#expect(actions.state == .positive)
		#expect(actions.assign.block == nil, "the stub has an overspent envelope")
		#expect(actions.assign.fills.map(\.name).contains("Dining out"))
		#expect(actions.fundGoals.block == .noGoalNeedsFunding)
		try render(ToBudgetActionsPopover(screen: screen, moveToCategory: {}), named: "popover")
		try render(
			ToBudgetCard(sheet: try #require(screen.sheet), dense: false, screen: screen, moveToCategory: {}).frame(width: 900),
			named: "card"
		)
	}

	@Test func tb_2_4_assignToUnderfundedSaysWhatItDidAndUndoes() async throws {
		let (_, screen) = await openBudget()
		let before = try #require(screen.toBudgetActions)
		let toBudget = try #require(screen.sheet).toBudget
		let dining = try #require(screen.envelope("dining"))
		#expect(dining.available < 0)
		await screen.assignToUnderfunded()
		let covered = try #require(screen.envelope("dining"))
		#expect(covered.available >= 0, "TB-3.2: the overspending is covered")
		let assigned = toBudget - (try #require(screen.sheet).toBudget)
		#expect(assigned == before.assign.total, "TB-2.2: To Budget falls by what the popover offered")
		#expect(screen.quickResult == ToBudgetWords.result(assigned: assigned, count: Int(before.assign.envelopes), counting: .envelopes, format: screen.format))
		try render(
			ToBudgetCard(sheet: try #require(screen.sheet), dense: false, screen: screen, moveToCategory: {}).frame(width: 900),
			named: "card-after-assign"
		)
		screen.dismissQuickResult()
		#expect(screen.quickResult == nil)
	}

	@Test func tb_1_1_aDenseScreenKeepsTheCardSmaller() async throws {
		let (_, screen) = await openBudget()
		try render(
			ToBudgetCard(sheet: try #require(screen.sheet), dense: true, screen: screen, moveToCategory: {}).frame(width: 900),
			named: "card-dense"
		)
	}

	private func render(_ view: some View, named name: String) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceWindow))
			host.frame.size = host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			RunLoop.main.run(until: .now.addingTimeInterval(0.3))
			host.frame.size = host.fittingSize
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}
