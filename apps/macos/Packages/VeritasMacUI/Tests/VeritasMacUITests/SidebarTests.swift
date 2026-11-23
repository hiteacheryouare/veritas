import AppKit
import SwiftUI
import Testing
import VeritasDesign
import VeritasKit
@testable import VeritasMacUI

/// The sidebar is the native list with the system highlight off and the
/// neutral fill drawn by the current row (the audit's B1).
@MainActor
struct SidebarTests {
	private func workspace() async -> WorkspaceModel {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		return workspace
	}

	private func host(_ workspace: WorkspaceModel) -> (NSView, NSWindow) {
		let host = NSHostingView(rootView: SidebarView(model: workspace).frame(width: 252, height: 900))
		let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 252, height: 900), styleMask: [.borderless], backing: .buffered, defer: false)
		window.isReleasedWhenClosed = false
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		RunLoop.main.run(until: .now.addingTimeInterval(0.3))
		return (host, window)
	}

	private func settle(_ host: NSView) {
		host.layoutSubtreeIfNeeded()
		RunLoop.main.run(until: .now.addingTimeInterval(0.2))
	}

	private func tables(in view: NSView) -> [NSTableView] {
		(view as? NSTableView).map { [$0] } ?? view.subviews.flatMap(tables)
	}

	@Test func theListsOwnHighlightIsOffAndStaysOff() async throws {
		let workspace = await workspace()
		let (host, window) = host(workspace)
		defer { window.close() }
		let table = try #require(tables(in: host).first)
		#expect(table.selectionHighlightStyle == .none, "only the neutral row fill marks the current row")
		#expect(table.numberOfSelectedRows == 1, "selection is still the list's")
		workspace.go(to: .account(id: "savings"))
		settle(host)
		#expect(table.selectionHighlightStyle == .none)
		#expect(table.numberOfSelectedRows == 1)
	}

	@Test func theFinderLeavesAScreensTableAloneUntilTheListsTableExists() {
		// The reviewer at a9017b6: reopening straight into a register, the register's
		// table exists before the List's. The finder must wait for the table that spans the List's columns.
		let root = NSView(frame: CGRect(x: 0, y: 0, width: 1000, height: 800))
		let finder = NoSystemHighlight.Finder(frame: CGRect(x: 0, y: 50, width: 252, height: 750))
		root.addSubview(finder)
		let register = Self.scrolledTable(frame: CGRect(x: 252, y: 0, width: 748, height: 800))
		let fullWidth = Self.scrolledTable(frame: root.bounds)
		root.addSubview(fullWidth.scroll)
		root.addSubview(register.scroll)
		finder.apply()
		#expect(register.table.selectionHighlightStyle == .regular, "a screen's table is never touched")
		#expect(fullWidth.table.selectionHighlightStyle == .regular, "nor one that only covers the List")
		// The List's scroll view runs under the pinned Settings row, so it's taller than the List.
		let list = Self.scrolledTable(frame: CGRect(x: 0, y: 0, width: 252, height: 800))
		root.addSubview(list.scroll)
		finder.apply()
		#expect(list.table.selectionHighlightStyle == .none, "the List's table, once it exists")
		#expect(register.table.selectionHighlightStyle == .regular)
		#expect(fullWidth.table.selectionHighlightStyle == .regular)
	}

	private static func scrolledTable(frame: CGRect) -> (scroll: NSScrollView, table: NSTableView) {
		let scroll = NSScrollView(frame: frame)
		let table = NSTableView(frame: CGRect(origin: .zero, size: frame.size))
		table.selectionHighlightStyle = .regular
		scroll.documentView = table
		return (scroll, table)
	}

	@Test func theSelectedRowsTextReadsAtSevenToOne() {
		// The fill sits on the sidebar; the current row's text is primary.
		for dark in [false, true] {
			let fill = dark ? ColorToken.surfaceSelected.dark : ColorToken.surfaceSelected.light
			let sidebar = dark ? ColorToken.surfaceSidebar.dark : ColorToken.surfaceSidebar.light
			let text = dark ? ColorToken.textPrimary.dark : ColorToken.textPrimary.light
			let base = Self.over(sidebar, dark ? ColorToken.surfaceWindow.dark : RGBA(0xffffff))
			#expect(Self.contrast(text, Self.over(fill, base)) >= 7, "dark \(dark)")
		}
	}

	private static func over(_ top: RGBA, _ bottom: RGBA) -> RGBA {
		func mix(_ a: Double, _ b: Double) -> Double { a * top.alpha + b * (1 - top.alpha) }
		let hex = [mix(top.red, bottom.red), mix(top.green, bottom.green), mix(top.blue, bottom.blue)].map { UInt32(($0 * 255).rounded()) }
		return RGBA(hex[0] << 16 | hex[1] << 8 | hex[2])
	}

	private static func contrast(_ a: RGBA, _ b: RGBA) -> Double {
		func channel(_ c: Double) -> Double { c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
		func luminance(_ c: RGBA) -> Double { 0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue) }
		let (high, low) = (max(luminance(a), luminance(b)), min(luminance(a), luminance(b)))
		return (high + 0.05) / (low + 0.05)
	}
}
