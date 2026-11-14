import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The Accounts screens on the stub budget, light and dark, into target/screenshots/accounts.
@MainActor
struct AccountScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/accounts")
		.standardizedFileURL

	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	@Test func accounts() async throws {
		let workspace = await workspace()
		let model = AccountsModel(workspace: workspace)
		await model.load()
		try render(AccountsScreen(workspace: workspace, model: model), named: "accounts", size: CGSize(width: 1000, height: 620))
	}

	@Test func register() async throws {
		let workspace = await workspace()
		let register = RegisterModel(workspace: workspace, scope: .account(accountId: "checking"))
		await register.load()
		try render(AccountRegisterScreen(workspace: workspace, accountID: "checking", register: register), named: "register", size: CGSize(width: 1200, height: 760), settle: 0.8)
		await register.setFilter { $0.search = "#medical" }
		try render(AccountRegisterScreen(workspace: workspace, accountID: "checking", register: register), named: "register-filtered", size: CGSize(width: 1200, height: 520), settle: 0.8)
	}

	@Test func quickEntry() async throws {
		let workspace = await workspace()
		let editor = TransactionEditorModel(workspace: workspace, accountID: "checking")
		await editor.load()
		editor.typePayee("Lake")
		try render(TransactionSheet(model: editor), named: "quick-entry-suggestions", settle: 0.6)
		if let pharmacy = editor.suggestions.first {
			await editor.choose(pharmacy)
		}
		editor.notes = "Allergy refill #medical"
		try render(TransactionSheet(model: editor), named: "quick-entry", settle: 0.6)
		editor.addSplitLine()
		try render(TransactionSheet(model: editor), named: "quick-entry-split", settle: 0.6)
	}

	@Test func reconcile() async throws {
		let workspace = await workspace()
		let model = ReconcileModel(workspace: workspace, accountID: "checking")
		await model.check(214_018)
		try render(ReconcileBar(model: model, format: MoneyFormat()) {}.frame(width: 1100), named: "reconcile")
	}

	@Test func payees() async throws {
		let workspace = await workspace()
		let model = PayeesModel(workspace: workspace)
		await model.load()
		model.selection = ["fernwood", "bistro"]
		try render(PayeesSheet(model: model), named: "payees", settle: 0.6)
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
