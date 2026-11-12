import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// Settings, the Integrity report and Open budget, light and dark, into
/// target/screenshots/shell for review.
@MainActor
struct ShellScreenshotTests {
	static let output = URL(filePath: #filePath)
		.deletingLastPathComponent()
		.appending(path: "../../../../../../target/screenshots/shell")
		.standardizedFileURL

	@Test func backups() async throws {
		let model = await openStub()
		let settings = try #require(model.settings)
		await settings.chooseBackupFolder(StubLaunch.backupFolder)
		await settings.backUpNow()
		await settings.load()
		try render(BackupSettingsView(settings: settings).frame(width: 900, height: 820), named: "settings-backups")
	}

	@Test func failedBackup() async throws {
		let library = StubLibrary(readOnly: nil, importError: nil, hasFindings: false, backupFails: true)
		let model = WorkspaceModel(library: library, localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await model.reopen(directory: StubLaunch.budgetDirectory)
		let settings = try #require(model.settings)
		await settings.chooseBackupFolder(StubLaunch.backupFolder)
		await settings.backUpNow()
		await settings.load()
		try render(BackupSettingsView(settings: settings).frame(width: 900, height: 820), named: "settings-backup-failed")
	}

	@Test func integrityReport() throws {
		let findings = [
			AuditFinding(check: .unpairedTransfer, detail: nil, table: "transactions", id: "t1", related: nil),
			AuditFinding(check: .splitSumMismatch, detail: "The parts add up to 40.00 of 45.00.", table: "transactions", id: "s1", related: nil)
		]
		try render(IntegrityReportSheet(report: .budget(findings: findings)), named: "integrity-report")
		try render(IntegrityReportSheet(report: .budget(findings: [])), named: "integrity-report-clean")
		try render(IntegrityReportSheet(report: .refusedImport(problems: 2)), named: "integrity-report-refused-import")
	}

	@Test func openBudget() async throws {
		let budgets = try await StubLibrary(readOnly: nil, importError: nil).listBudgets()
		try render(OpenBudgetSheet(budgets: budgets) { _ in }, named: "open-budget")
	}

	private func openStub() async -> WorkspaceModel {
		let model = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await model.reopen(directory: StubLaunch.budgetDirectory)
		return model
	}

	private func render(_ view: some View, named name: String) throws {
		try FileManager.default.createDirectory(at: Self.output, withIntermediateDirectories: true)
		for (appearance, suffix) in [(NSAppearance.Name.aqua, "light"), (.darkAqua, "dark")] {
			let host = NSHostingView(rootView: view.tint(Color.accent).background(.surfaceRaised))
			host.frame.size = host.fittingSize
			let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			window.appearance = NSAppearance(named: appearance)
			window.contentView = host
			host.layoutSubtreeIfNeeded()
			let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
			host.cacheDisplay(in: host.bounds, to: bitmap)
			let png = try #require(bitmap.representation(using: .png, properties: [:]))
			try png.write(to: Self.output.appending(path: "\(name)-\(suffix).png"))
		}
	}
}

struct SettingsSectionTests {
	@Test func onlyTheSectionsThatWorkAreShown() {
		// D69: AI and Tags come back when they work.
		#expect(SettingsView.Section.allCases.map(\.title) == ["Backups", "Actual", "Appearance"])
	}
}
