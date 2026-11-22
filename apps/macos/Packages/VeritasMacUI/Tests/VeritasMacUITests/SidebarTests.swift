import AppKit
import SwiftUI
import Testing
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The sidebar is a table that draws its own neutral selection (the audit's B1).
@MainActor
struct SidebarTests {
	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	/// The table inside a hosted sidebar, after a layout pass.
	private func table(_ workspace: WorkspaceModel) throws -> (NSTableView, NSWindow) {
		let host = NSHostingView(rootView: SidebarView(model: workspace).frame(width: 252, height: 900))
		let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 252, height: 900), styleMask: [.borderless], backing: .buffered, defer: false)
		window.isReleasedWhenClosed = false
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		RunLoop.main.run(until: .now.addingTimeInterval(0.2))
		func find(_ view: NSView) -> NSTableView? {
			if let table = view as? NSTableView { return table }
			return view.subviews.lazy.compactMap(find).first
		}
		return (try #require(find(host)), window)
	}

	@Test func theSelectedRowIsNeutralAndKeepsItsTextColour() throws {
		let row = SidebarRowView()
		row.isSelected = true
		row.isEmphasized = true
		#expect(row.interiorBackgroundStyle == .normal, "selected text never turns white on the light fill")
		for (name, token) in [(NSAppearance.Name.aqua, ColorToken.surfaceSelected.light), (.darkAqua, ColorToken.surfaceSelected.dark)] {
			var drawn: NSColor?
			NSAppearance(named: name)?.performAsCurrentDrawingAppearance { drawn = SidebarRowView.fill.usingColorSpace(.sRGB) }
			let fill = try #require(drawn)
			#expect(abs(fill.alphaComponent - token.alpha) < 0.01, "\(name)")
			#expect(abs(fill.redComponent - token.red) < 0.01, "\(name)")
		}
	}

	@Test func insightsHeadsItsPagesAndCantBeSelected() async throws {
		let workspace = await workspace()
		let items = SidebarItem.items(for: workspace)
		let insights = try #require(items.firstIndex(of: .insights))
		#expect(SidebarItem.insights.destination == nil)
		#expect(Array(items[(insights + 1)...].prefix(5)) == Screen.insights.pages.map(SidebarItem.page))
		#expect(items.filter { if case .groupName = $0 { true } else { false } }.allSatisfy { $0.destination == nil })
	}

	@Test func theTableSelectsTheCurrentPlaceAndTypeSelectsByName() async throws {
		let workspace = await workspace()
		workspace.go(to: .account(id: "savings"))
		let (table, window) = try table(workspace)
		defer { window.close() }
		let items = SidebarItem.items(for: workspace)
		#expect(table.numberOfRows == items.count)
		#expect(table.numberOfSelectedRows == 1)
		#expect(items[table.selectedRow] == items.first { $0.destination == .account(id: "savings") })
		let delegate = try #require(table.delegate)
		let header = try #require(items.firstIndex(of: .insights))
		#expect(delegate.tableView?(table, shouldSelectRow: header) == false)
		let goals = try #require(items.firstIndex(of: .section(.goals)))
		#expect(delegate.tableView?(table, typeSelectStringFor: nil, row: goals) == "Goals")
		#expect(table.rowView(atRow: goals, makeIfNecessary: true)?.accessibilityIdentifier() == "sidebar.goals")
	}

	@Test func choosingARowGoesThere() async throws {
		let workspace = await workspace()
		let (table, window) = try table(workspace)
		defer { window.close() }
		let items = SidebarItem.items(for: workspace)
		let bills = try #require(items.firstIndex(of: .section(.bills)))
		table.selectRowIndexes(IndexSet(integer: bills), byExtendingSelection: false)
		#expect(workspace.destination == .screen(.bills))
	}
}
