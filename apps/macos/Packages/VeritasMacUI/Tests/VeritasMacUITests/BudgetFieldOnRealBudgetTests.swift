import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// The Budget screen's amount field as the row hosts it (`screen.editing`),
/// on fixtures/synthetic/all-features through the real core: typing to open
/// it, Esc and retyping, and undo with a field open (R-BUD-3).
@MainActor
struct BudgetFieldOnRealBudgetTests {
	static let october = Month(year: 2026, month: 10)
	static let groceries = "fc093a41-2d11-4d98-a211-e5db92205f23"
	static let emergencyFund = "b8070040-5368-407b-b9e7-f6dabe59b7a8"

	@Test func anUndoUnderAnUntouchedFieldShowsTheNewValueSelected() async throws {
		try await withScreen { screen, workspace in
			screen.beginEditing(Self.groceries)
			screen.editing?.type("200")
			await screen.commitEditing()
			screen.beginEditing(Self.groceries)
			let row = try RowField(screen)
			try await row.shows("200.00", selected: true)

			let count = workspace.changeCount
			await workspace.undo()
			try await until { workspace.changeCount > count }
			await screen.load()
			try await row.shows("140.00", selected: true)
			try await row.type("55")
			await screen.commitEditing()
			#expect(try budgeted(screen, Self.groceries) == 5_500, "typing over the refreshed value replaces it")
		}
	}

	@Test func escapeThenTypingAgainStartsFromWhatWasTyped() async throws {
		try await withScreen { screen, _ in
			screen.selection = Self.groceries
			screen.beginEditing(Self.groceries)
			screen.editing?.type("+")
			var row = try RowField(screen)
			try await row.type("40")
			try await row.shows("140.00+40", selected: false)
			screen.cancelEditing()
			#expect(try budgeted(screen, Self.groceries) == 14_000, "Esc writes nothing")

			screen.beginEditing(Self.groceries)
			screen.editing?.type("7")
			row = try RowField(screen)
			try await row.type("5")
			try await row.shows("75", selected: false)
			await screen.commitEditing()
			#expect(try budgeted(screen, Self.groceries) == 7_500)
		}
	}

	@Test func undoingATypedEditLeavesTheNextFieldReadyToTypeOver() async throws {
		try await withScreen { screen, workspace in
			screen.selection = Self.groceries
			screen.beginEditing(Self.groceries)
			screen.editing?.type("+")
			var row = try RowField(screen)
			try await row.type("40")
			await screen.commitEditing(thenMove: 1)
			#expect(try budgeted(screen, Self.groceries) == 18_000)
			#expect(screen.editingID == Self.emergencyFund)
			row = try RowField(screen)
			try await row.shows("300.00", selected: true)

			let count = workspace.changeCount
			await workspace.undo()
			try await until { workspace.changeCount > count }
			await screen.load()
			#expect(try budgeted(screen, Self.groceries) == 14_000)
			try await row.shows("300.00", selected: true)
			try await row.type("1")
			try await row.shows("1", selected: false)
		}
	}

	private func budgeted(_ screen: BudgetScreenModel, _ id: String) throws -> Int64 {
		try #require(screen.sheet?.groups.lazy.flatMap(\.envelopes).first { $0.id == id }).budgeted
	}

	private func withScreen(_ body: (BudgetScreenModel, WorkspaceModel) async throws -> Void) async throws {
		let budgets = URL.temporaryDirectory.appending(path: "veritas-field-\(UUID().uuidString)", directoryHint: .isDirectory)
		defer { try? FileManager.default.removeItem(at: budgets) }
		let fixture = URL(filePath: #filePath)
			.deletingLastPathComponent()
			.appending(path: "../../../../../../fixtures/synthetic/all-features.zip")
			.standardizedFileURL
		let workspace = WorkspaceModel(library: CoreBudgetLibrary(budgetsDirectory: budgets), format: MoneyFormat())
		await workspace.importActualZip(fixture)
		do {
			try #require(workspace.phase == .open)
			let screen = BudgetScreenModel(workspace: workspace, reader: CoreAmountReader(budgetDecimalPlaces: workspace.budgetDecimalPlaces), month: Self.october)
			await screen.load()
			try await body(screen, workspace)
		} catch {
			await workspace.closeBudget()
			throw error
		}
		await workspace.closeBudget()
	}
}

@MainActor
private func until(_ condition: () -> Bool) async throws {
	for _ in 0..<400 where !condition() {
		try await Task.sleep(for: .milliseconds(5))
	}
	let met = condition()
	try #require(met)
}

/// Whatever field the screen is editing, hosted the way EnvelopeRowView hosts
/// it, so a new `screen.editing` reaches the same NSTextField through SwiftUI.
@MainActor
private struct RowField {
	let screen: BudgetScreenModel
	let window: NSWindow

	init(_ screen: BudgetScreenModel) throws {
		self.screen = screen
		let host = NSHostingView(rootView: EditingCell(screen: screen).frame(width: 140, height: 26))
		host.frame.size = CGSize(width: 200, height: 40)
		window = NSWindow(contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		try #require(view?.currentEditor() != nil, "the field took focus when it appeared")
	}

	var view: FocusingTextField? {
		window.contentView.flatMap(Self.field(in:))
	}

	func type(_ keys: String) async throws {
		for key in keys {
			let editor = try #require(view?.currentEditor() as? NSTextView)
			editor.insertText(String(key), replacementRange: NSRange(location: NSNotFound, length: 0))
			try await until { [self] in view?.stringValue == screen.editing?.text }
		}
	}

	func shows(_ text: String, selected: Bool) async throws {
		try await until { [self] in
			window.contentView?.layoutSubtreeIfNeeded()
			return view?.stringValue == text
		}
		let range = try #require((view?.currentEditor() as? NSTextView)?.selectedRange)
		let length = text.utf16.count
		#expect(range == (selected ? NSRange(location: 0, length: length) : NSRange(location: length, length: 0)), "\(text): \(range)")
	}

	private static func field(in view: NSView) -> FocusingTextField? {
		if let field = view as? FocusingTextField { return field }
		return view.subviews.lazy.compactMap(field(in:)).first
	}
}

private struct EditingCell: View {
	let screen: BudgetScreenModel

	var body: some View {
		if let field = screen.editing {
			AmountTextField(field: field) { _ in }
		}
	}
}
