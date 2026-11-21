import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Rules page on the stub budget, light and dark, into target/screenshots/rules.
@MainActor
struct RulesScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/rules")
		.standardizedFileURL

	private func model() async -> (WorkspaceModel, RulesModel) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = RulesModel(workspace: workspace)
		await model.load()
		return (workspace, model)
	}

	@Test func list() async throws {
		let (workspace, model) = await model()
		#expect(model.rules.count == 5)
		try render(RulesScreen(workspace: workspace, model: model), named: "list", size: CGSize(width: 900, height: 720))
	}

	@Test func editorForASimpleRule() async throws {
		let (_, model) = await model()
		model.edit(try #require(model.rules.first { $0.id == "amazon" }))
		try render(RuleEditorSheet(model: model), named: "editor")
		model.editing?.problem = "Type what the imported payee should contain."
		try render(RuleEditorSheet(model: model), named: "editor-problem")
	}

	@Test func editorForAnOrRule() async throws {
		let (_, model) = await model()
		model.edit(try #require(model.rules.first { $0.id == "coffee" }))
		try render(RuleEditorSheet(model: model), named: "editor-any")
	}

	@Test func editorForASplitRule() async throws {
		let (_, model) = await model()
		model.edit(try #require(model.rules.first { $0.id == "costco" }))
		#expect(model.editing?.splitLines == [1, 2])
		try render(RuleEditorSheet(model: model), named: "editor-split")
	}

	/// Save and Cancel clear the draft while the sheet is still on screen.
	@Test func editorSurvivesItsDraftClosing() async throws {
		let (_, model) = await model()
		model.edit(try #require(model.rules.first { $0.id == "costco" }))
		closeWhileShown(RuleEditorSheet(model: model)) { model.cancelEditing() }
		#expect(model.editing == nil)
	}

	private func closeWhileShown(_ view: some View, close: () -> Void) {
		let host = NSHostingView(rootView: view)
		host.frame.size = host.fittingSize
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.contentView = host
		RunLoop.main.run(until: .now.addingTimeInterval(0.4))
		close()
		RunLoop.main.run(until: .now.addingTimeInterval(0.4))
		host.layoutSubtreeIfNeeded()
	}

	/// Removing every split line, then every action, and saving, all while the sheet is drawn.
	@Test func editorSurvivesRemovingRowsAndSavingWhileShown() async throws {
		let (_, model) = await model()
		model.edit(try #require(model.rules.first { $0.id == "costco" }))
		let lines = try #require(model.editing?.splitLines)
		closeWhileShown(RuleEditorSheet(model: model)) {
			for line in lines.reversed() { model.editing?.removeSplitLine(line) }
		}
		#expect(model.editing?.splitLines.isEmpty == true)
		closeWhileShown(RuleEditorSheet(model: model)) { model.editing?.draft.actions.removeAll() }
		#expect(model.editing?.draft.actions.isEmpty == true)

		model.edit(try #require(model.rules.first { $0.id == "amazon" }))
		let host = shown(RuleEditorSheet(model: model))
		#expect(await model.save(), "\(String(describing: model.editing?.problem))")
		settle(host)
		#expect(model.editing == nil)
	}

	private func shown(_ view: some View) -> NSView {
		let host = NSHostingView(rootView: view)
		host.frame.size = host.fittingSize
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.contentView = host
		RunLoop.main.run(until: .now.addingTimeInterval(0.4))
		return host
	}

	private func settle(_ host: NSView) {
		RunLoop.main.run(until: .now.addingTimeInterval(0.4))
		host.layoutSubtreeIfNeeded()
	}

	@Test func applyPreviewWithAReconciledRow() async throws {
		let (_, model) = await model()
		await model.preview(.saved(ruleId: "amazon"))
		let applying = try #require(model.applying)
		#expect(applying.reconciledRows.map(\.id) == [StubRules.reconciledRow])
		#expect(!applying.selected.contains(StubRules.reconciledRow))
		try render(RuleApplySheet(model: model), named: "apply")
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
