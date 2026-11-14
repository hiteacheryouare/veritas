import AppKit
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The register as an `NSTableView` (PLAN §6.7): SwiftUI lists can't scroll
/// 100k rows at display rate (R-PERF-3). Rows come a page at a time from
/// `RegisterModel`; a row not loaded yet draws empty until its page arrives.
struct RegisterTable: NSViewRepresentable {
	let model: RegisterModel
	let open: (RegisterRow) -> Void
	let delete: ([RegisterRow]) -> Void

	func makeCoordinator() -> Coordinator { Coordinator(self) }

	func makeNSView(context: Context) -> NSScrollView {
		let table = RegisterNSTable()
		table.coordinator = context.coordinator
		table.style = .inset
		table.usesAlternatingRowBackgroundColors = false
		table.rowHeight = 32
		table.intercellSpacing = NSSize(width: 12, height: 0)
		table.allowsMultipleSelection = true
		table.gridStyleMask = [.solidHorizontalGridLineMask]
		table.gridColor = NSColor.separatorColor.withAlphaComponent(0.4)
		table.setAccessibilityIdentifier("register.table")
		for column in Column.allCases {
			let tableColumn = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(column.rawValue))
			tableColumn.title = column.title
			tableColumn.width = column.width
			tableColumn.minWidth = column == .cleared ? 24 : 50
			if column == .cleared { tableColumn.maxWidth = 28 }
			if let key = column.sortKey {
				tableColumn.sortDescriptorPrototype = NSSortDescriptor(key: key, ascending: column == .date ? false : true)
			}
			if column.isNumeric { tableColumn.headerCell.alignment = .right }
			table.addTableColumn(tableColumn)
		}
		table.dataSource = context.coordinator
		table.delegate = context.coordinator
		table.target = context.coordinator
		table.doubleAction = #selector(Coordinator.openClicked)
		table.menu = context.coordinator.menu
		context.coordinator.table = table
		model.rowsArrived = { [weak coordinator = context.coordinator] range in
			coordinator?.reload(range)
		}
		let scroll = NSScrollView()
		scroll.documentView = table
		scroll.hasVerticalScroller = true
		scroll.drawsBackground = false
		return scroll
	}

	func updateNSView(_ scroll: NSScrollView, context: Context) {
		let coordinator = context.coordinator
		coordinator.parent = self
		guard let table = coordinator.table else { return }
		table.tableColumn(withIdentifier: NSUserInterfaceItemIdentifier(Column.balance.rawValue))?.isHidden = !model.showsBalance
		table.tableColumn(withIdentifier: NSUserInterfaceItemIdentifier(Column.account.rawValue))?.isHidden = model.accountID != nil
		if coordinator.revision != model.revision {
			coordinator.revision = model.revision
			table.reloadData()
			coordinator.restoreSelection()
		}
	}

	enum Column: String, CaseIterable {
		case cleared, date, account, payee, envelope, notes, amount, balance

		var title: String {
			switch self {
			case .cleared: ""
			case .date: "Date"
			case .account: "Account"
			case .payee: "Payee"
			case .envelope: "Envelope"
			case .notes: "Notes"
			case .amount: "Amount"
			case .balance: "Balance"
			}
		}

		var width: CGFloat {
			switch self {
			case .cleared: 24
			case .date: 84
			case .account: 120
			case .payee: 220
			case .envelope: 150
			case .notes: 260
			case .amount, .balance: 100
			}
		}

		var sortKey: String? {
			switch self {
			case .date, .account, .payee, .notes, .amount: rawValue
			case .envelope: "category"
			case .cleared, .balance: nil
			}
		}

		var isNumeric: Bool { self == .amount || self == .balance }
	}

	@MainActor
	final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
		var parent: RegisterTable
		weak var table: NSTableView?
		var revision = -1
		let menu = NSMenu()

		init(_ parent: RegisterTable) {
			self.parent = parent
			super.init()
			menu.delegate = self
			menu.autoenablesItems = false
		}

		var model: RegisterModel { parent.model }

		func reload(_ range: Range<Int>) {
			guard let table, !range.isEmpty else { return }
			let rows = IndexSet(integersIn: range.clamped(to: 0..<table.numberOfRows))
			table.reloadData(forRowIndexes: rows, columnIndexes: IndexSet(integersIn: 0..<table.numberOfColumns))
			restoreSelection()
		}

		func restoreSelection() {
			guard let table else { return }
			let rows = IndexSet(model.selection.compactMap { model.index(of: $0) })
			if rows != table.selectedRowIndexes { table.selectRowIndexes(rows, byExtendingSelection: false) }
			if let first = rows.first { table.scrollRowToVisible(first) }
		}

		func numberOfRows(in tableView: NSTableView) -> Int { model.total }

		func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row index: Int) -> NSView? {
			guard let tableColumn, let column = Column(rawValue: tableColumn.identifier.rawValue) else { return nil }
			let cell = tableView.makeView(withIdentifier: tableColumn.identifier, owner: self) as? RegisterCell ?? RegisterCell(column: column)
			cell.show(model.row(at: index), column: column, model: model) { [weak self] row in
				Task { await self?.model.toggleCleared(row) }
			}
			return cell
		}

		func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
			let view = NSTableRowView()
			if let id = model.row(at: row)?.id { view.setAccessibilityIdentifier("register.row.\(id)") }
			return view
		}

		func tableViewSelectionDidChange(_ notification: Notification) {
			guard let table else { return }
			model.selection = Set(table.selectedRowIndexes.compactMap { model.row(at: $0)?.id })
		}

		func tableView(_ tableView: NSTableView, sortDescriptorsDidChange oldDescriptors: [NSSortDescriptor]) {
			guard let descriptor = tableView.sortDescriptors.first, let key = descriptor.key else { return }
			let sortKey: RegisterSortKey = switch key {
			case "account": .account
			case "payee": .payee
			case "category": .category
			case "notes": .notes
			case "amount": .amount
			default: .date
			}
			Task { await model.setSort(sortKey, ascending: descriptor.ascending) }
		}

		@objc func openClicked() {
			guard let table, table.clickedRow >= 0, let row = model.row(at: table.clickedRow) else { return }
			parent.open(row)
		}

		func openSelected() {
			guard let table, let first = table.selectedRowIndexes.first, let row = model.row(at: first) else { return }
			parent.open(row)
		}

		func deleteSelected() {
			let rows = model.selectedRows
			guard !rows.isEmpty, !model.isReadOnly else { return }
			parent.delete(rows)
		}

		func menuNeedsUpdate(_ menu: NSMenu) {
			menu.removeAllItems()
			guard let table, table.clickedRow >= 0, let row = model.row(at: table.clickedRow) else { return }
			if !table.selectedRowIndexes.contains(table.clickedRow) {
				table.selectRowIndexes([table.clickedRow], byExtendingSelection: false)
			}
			let rows = model.selectedRows
			let editable = !model.isReadOnly
			add("Edit…", enabled: editable && rows.count == 1) { [weak self] in self?.parent.open(row) }
			add("Duplicate", enabled: editable && rows.count == 1) { [weak self] in Task { await self?.model.duplicate(row.id) } }
			menu.addItem(.separator())
			let cleared = rows.allSatisfy(\.cleared)
			add(cleared ? "Mark Uncleared" : "Mark Cleared", enabled: editable && rows.contains { !$0.reconciled }) { [weak self] in
				Task { await self?.model.setCleared(rows.filter { !$0.reconciled }.map(\.id), !cleared) }
			}
			if row.reconciled {
				add("Unlock Reconciled", enabled: editable) { [weak self] in Task { await self?.model.unlock(row.id) } }
			}
			menu.addItem(.separator())
			add(rows.count == 1 ? "Delete" : "Delete \(rows.count) Transactions", enabled: editable) { [weak self] in self?.parent.delete(rows) }
		}

		private func add(_ title: String, enabled: Bool, _ action: @escaping () -> Void) {
			let item = ClosureMenuItem(title: title, action: action)
			item.isEnabled = enabled
			menu.addItem(item)
		}
	}
}

/// Return opens the selected row and Delete removes it, as in Actual.
final class RegisterNSTable: NSTableView {
	weak var coordinator: RegisterTable.Coordinator?

	override func keyDown(with event: NSEvent) {
		switch event.keyCode {
		case 36, 76: coordinator?.openSelected()
		case 51, 117: coordinator?.deleteSelected()
		default: super.keyDown(with: event)
		}
	}
}

final class ClosureMenuItem: NSMenuItem {
	private let run: () -> Void

	init(title: String, action: @escaping () -> Void) {
		run = action
		super.init(title: title, action: #selector(fire), keyEquivalent: "")
		target = self
	}

	required init(coder: NSCoder) {
		run = {}
		super.init(coder: coder)
	}

	@objc private func fire() { run() }
}

final class RegisterCell: NSTableCellView {
	private let label = NSTextField(labelWithString: "")
	private let button = NSButton()
	private var toggle: ((RegisterRow) -> Void)?
	private var row: RegisterRow?

	init(column: RegisterTable.Column) {
		super.init(frame: .zero)
		identifier = NSUserInterfaceItemIdentifier(column.rawValue)
		if column == .cleared {
			button.bezelStyle = .inline
			button.isBordered = false
			button.imagePosition = .imageOnly
			button.target = self
			button.action = #selector(clicked)
			button.translatesAutoresizingMaskIntoConstraints = false
			addSubview(button)
			NSLayoutConstraint.activate([button.centerXAnchor.constraint(equalTo: centerXAnchor), button.centerYAnchor.constraint(equalTo: centerYAnchor)])
		} else {
			label.lineBreakMode = .byTruncatingTail
			label.translatesAutoresizingMaskIntoConstraints = false
			label.alignment = column.isNumeric ? .right : .left
			addSubview(label)
			textField = label
			NSLayoutConstraint.activate([
				label.leadingAnchor.constraint(equalTo: leadingAnchor),
				label.trailingAnchor.constraint(equalTo: trailingAnchor),
				label.centerYAnchor.constraint(equalTo: centerYAnchor)
			])
		}
	}

	required init?(coder: NSCoder) { nil }

	@objc private func clicked() {
		if let row { toggle?(row) }
	}

	func show(_ row: RegisterRow?, column: RegisterTable.Column, model: RegisterModel, toggle: @escaping (RegisterRow) -> Void) {
		self.row = row
		self.toggle = toggle
		let body = NSFont.systemFont(ofSize: TextSize.body.rawValue)
		let figures = NSFont.monospacedDigitSystemFont(ofSize: TextSize.body.rawValue, weight: .medium)
		label.textColor = .labelColor
		label.font = body
		guard let row else {
			label.stringValue = ""
			button.image = nil
			return
		}
		switch column {
		case .cleared:
			let (symbol, description): (String, String) = row.reconciled
				? ("lock.fill", "Reconciled")
				: (row.cleared ? ("checkmark.circle.fill", "Cleared") : ("circle", "Uncleared"))
			button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: description)
			button.contentTintColor = row.cleared ? .controlAccentColor : .tertiaryLabelColor
			button.isEnabled = !row.reconciled && !model.isReadOnly
			button.setAccessibilityIdentifier("register.cleared.\(row.id)")
		case .date:
			label.stringValue = row.shortDate(thisYear: model.thisYear)
			label.textColor = .secondaryLabelColor
			label.font = figures
		case .account:
			label.stringValue = row.accountName
			label.textColor = .secondaryLabelColor
		case .payee:
			let payee = PayeeText(row)
			let text = NSMutableAttributedString()
			if let direction = payee.direction {
				text.append(NSAttributedString(string: direction + " ", attributes: [.font: NSFont.systemFont(ofSize: TextSize.body.rawValue, weight: .semibold), .foregroundColor: NSColor.secondaryLabelColor]))
			}
			text.append(NSAttributedString(string: payee.text, attributes: [.font: NSFont.systemFont(ofSize: TextSize.body.rawValue, weight: .medium), .foregroundColor: NSColor.labelColor]))
			label.attributedStringValue = text
		case .envelope:
			label.stringValue = row.envelopeText
			label.textColor = row.amount > 0 && !row.isTransfer ? NSColor(Color(ColorToken.moneyIncome)) : .secondaryLabelColor
		case .notes:
			label.stringValue = row.notes ?? ""
			label.textColor = .secondaryLabelColor
		case .amount:
			label.stringValue = (row.amount > 0 ? "+" : "") + model.format.amount(row.amount)
			label.font = NSFont.monospacedDigitSystemFont(ofSize: TextSize.body.rawValue, weight: .semibold)
			label.textColor = row.amount > 0 && !row.isTransfer ? NSColor(Color(ColorToken.moneyIncome)) : .labelColor
		case .balance:
			label.stringValue = row.runningBalance.map(model.format.amount) ?? ""
			label.textColor = .secondaryLabelColor
			label.font = figures
		}
	}
}
