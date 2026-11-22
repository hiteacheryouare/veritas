import AppKit
import SwiftUI
import VeritasDesign
import VeritasKit

/// Budget, Accounts with their balances, Bills, Goals, Reports and Insights,
/// with Settings kept at the bottom.
///
/// The list is an `NSTableView` so the selected row's fill is ours: a neutral
/// fill with primary text, whatever accent the app has (WCAG AA in both
/// appearances). The table keeps what a native list gives: arrow keys,
/// type-select, Home and End, Page Up and Down, the selection scrolled into
/// view, one Tab stop and VoiceOver's rows.
struct SidebarView: View {
	@Bindable var model: WorkspaceModel

	var body: some View {
		SidebarTable(items: SidebarItem.items(for: model), current: model.destination, format: model.format) { destination in
			model.go(to: destination)
		}
		.safeAreaInset(edge: .bottom, spacing: 0) {
			SettingsRow(isCurrent: model.destination == .screen(.settings)) {
				model.go(to: .screen(.settings))
			}
		}
	}
}

/// One row of the sidebar, in the order it's drawn.
enum SidebarItem: Hashable {
	case section(Screen)
	case page(Screen)
	case groupName(String)
	case account(SidebarAccount)
	/// Insights has no page of its own, so it heads its pages, which always show.
	case insights

	var destination: Destination? {
		switch self {
		case .section(let screen), .page(let screen): .screen(screen)
		case .account(let account): .account(id: account.id)
		case .groupName, .insights: nil
		}
	}

	var identifier: String {
		switch self {
		case .section(let screen), .page(let screen): "sidebar.\(screen.rawValue)"
		case .account(let account): "sidebar.account.\(account.id)"
		case .groupName(let name): "sidebar.group.\(name)"
		case .insights: "sidebar.insights"
		}
	}

	/// What type-select matches and VoiceOver reads first.
	var title: String {
		switch self {
		case .section(let screen), .page(let screen): screen.title
		case .account(let account): account.name
		case .groupName(let name): name
		case .insights: Screen.insights.title
		}
	}

	@MainActor static func items(for model: WorkspaceModel) -> [SidebarItem] {
		let section = model.destination.section
		var items: [SidebarItem] = [.section(.budget), .section(.accounts)]
		for group in model.overview?.accountGroups ?? [] {
			items.append(.groupName(group.name))
			items += group.accounts.map(SidebarItem.account)
		}
		items.append(.section(.bills))
		if section == .bills { items += Screen.bills.pages.map(SidebarItem.page) }
		items.append(.section(.goals))
		items.append(.section(.reports))
		if section == .reports { items += Screen.reports.pages.map(SidebarItem.page) }
		items.append(.insights)
		items += Screen.insights.pages.map(SidebarItem.page)
		return items
	}
}

/// The neutral selection, drawn by the row. The row's interior stays normal,
/// so text keeps its own colour instead of turning white on the fill.
final class SidebarRowView: NSTableRowView {
	static let fill = NSColor(Color(ColorToken.surfaceSelected))

	override func drawSelection(in dirtyRect: NSRect) {
		let rect = bounds.insetBy(dx: Spacing.s2, dy: 1)
		Self.fill.setFill()
		NSBezierPath(roundedRect: rect, xRadius: Radius.control, yRadius: Radius.control).fill()
	}

	override var interiorBackgroundStyle: NSView.BackgroundStyle { .normal }
}

struct SidebarTable: NSViewRepresentable {
	let items: [SidebarItem]
	let current: Destination
	let format: MoneyFormat
	let go: (Destination) -> Void

	func makeCoordinator() -> Coordinator { Coordinator(self) }

	func makeNSView(context: Context) -> NSScrollView {
		let table = NSTableView()
		table.addTableColumn(NSTableColumn(identifier: NSUserInterfaceItemIdentifier("row")))
		table.headerView = nil
		table.style = .plain
		table.backgroundColor = .clear
		table.intercellSpacing = NSSize(width: 0, height: 2)
		table.focusRingType = .none
		table.allowsEmptySelection = true
		table.allowsTypeSelect = true
		table.delegate = context.coordinator
		table.dataSource = context.coordinator
		table.setAccessibilityLabel("Sidebar")
		table.setAccessibilityIdentifier("sidebar")
		let scroll = NSScrollView()
		scroll.documentView = table
		scroll.drawsBackground = false
		scroll.hasVerticalScroller = true
		scroll.autohidesScrollers = true
		scroll.scrollerStyle = .overlay
		scroll.automaticallyAdjustsContentInsets = true
		context.coordinator.table = table
		context.coordinator.reload()
		return scroll
	}

	func updateNSView(_ scroll: NSScrollView, context: Context) {
		context.coordinator.parent = self
		context.coordinator.reload()
	}

	@MainActor
	final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
		var parent: SidebarTable
		weak var table: NSTableView?
		private var shown: [SidebarItem] = []
		private var shownCurrent: Destination?
		private var isSelecting = false

		init(_ parent: SidebarTable) {
			self.parent = parent
		}

		func reload() {
			guard let table else { return }
			if shown != parent.items || shownCurrent != parent.current {
				shown = parent.items
				shownCurrent = parent.current
				table.reloadData()
			}
			isSelecting = true
			if let row = shown.firstIndex(where: { $0.destination == parent.current }) {
				table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
				table.scrollRowToVisible(row)
			} else {
				table.deselectAll(nil)
			}
			isSelecting = false
		}

		func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

		func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
			if case .groupName = shown[row] { return 22 }
			return 28
		}

		func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
			let item = shown[row]
			return NSHostingView(rootView: SidebarCell(item: item, isCurrent: item.destination == parent.current, format: parent.format))
		}

		func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
			let item = shown[row]
			let view = SidebarRowView()
			view.setAccessibilityIdentifier(item.identifier)
			view.setAccessibilityLabel(SidebarCell.spoken(item, format: parent.format))
			return view
		}

		func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
			shown[row].destination != nil
		}

		func tableView(_ tableView: NSTableView, typeSelectStringFor tableColumn: NSTableColumn?, row: Int) -> String? {
			shown[row].destination == nil ? nil : shown[row].title
		}

		func tableViewSelectionDidChange(_ notification: Notification) {
			guard !isSelecting, let table, shown.indices.contains(table.selectedRow), let destination = shown[table.selectedRow].destination else { return }
			table.scrollRowToVisible(table.selectedRow)
			if destination != parent.current { parent.go(destination) }
		}
	}
}

/// What one row draws. The current row's text is primary on the neutral fill.
struct SidebarCell: View {
	let item: SidebarItem
	let isCurrent: Bool
	let format: MoneyFormat

	var body: some View {
		Group {
			switch item {
			case .section(let screen):
				Label(screen.title, systemImage: screen.symbol)
					.foregroundStyle(Color.textPrimary)
			case .insights:
				Label(Screen.insights.title, systemImage: Screen.insights.symbol)
					.foregroundStyle(Color.textPrimary)
			case .page(let screen):
				Text(screen.title)
					.foregroundStyle(isCurrent ? Color.textPrimary : Color.textSecondary)
					.padding(.leading, 26)
			case .groupName(let name):
				Text(name)
					.font(.text(.caption, weight: .semibold))
					.foregroundStyle(.textTertiary)
					.padding(.leading, 26)
			case .account(let account):
				HStack {
					Text(account.name)
						.lineLimit(1)
						.foregroundStyle(isCurrent ? Color.textPrimary : (account.isInactive ? Color.textTertiary : Color.textSecondary))
					Spacer(minLength: Spacing.s2)
					Text(format.amount(account.balance, on: .register))
						.font(.figures(.body))
						.foregroundStyle(account.isInactive && !isCurrent ? Color.textTertiary : Color.textPrimary)
				}
				.padding(.leading, 26)
			}
		}
		.fontWeight(isCurrent ? .medium : nil)
		.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
		.padding(.horizontal, Spacing.s4)
		.accessibilityHidden(true)
	}

	/// The row as VoiceOver reads it: the name, and an account's balance.
	static func spoken(_ item: SidebarItem, format: MoneyFormat) -> String {
		if case .account(let account) = item {
			return "\(account.name), \(format.amount(account.balance, on: .register))"
		}
		return item.title
	}
}

private struct SettingsRow: View {
	let isCurrent: Bool
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			Label(Screen.settings.title, systemImage: Screen.settings.symbol)
				.frame(maxWidth: .infinity, alignment: .leading)
				.padding(.horizontal, Spacing.s2)
				.frame(height: 30)
				.background(isCurrent ? AnyShapeStyle(.surfaceSelected) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.foregroundStyle(Color.textPrimary)
		.fontWeight(isCurrent ? .medium : .regular)
		.padding(.horizontal, Spacing.s2)
		.padding(.bottom, Spacing.s3)
		.accessibilityIdentifier("sidebar.settings")
		.accessibilityAddTraits(isCurrent ? .isSelected : [])
	}
}
