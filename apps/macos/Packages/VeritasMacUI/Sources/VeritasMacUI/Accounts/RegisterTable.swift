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
			coordinator.reloadAll()
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
		private var rowElements: [Int: RegisterRowElement] = [:]
		private var labels: [String: String] = [:]

		init(_ parent: RegisterTable) {
			self.parent = parent
			super.init()
			menu.delegate = self
			menu.autoenablesItems = false
		}

		var model: RegisterModel { parent.model }

		/// A page arrived. It keeps the selection without scrolling to it, so
		/// a page loading in the background never moves the register.
		func reload(_ range: Range<Int>) {
			guard let table, !range.isEmpty else { return }
			let rows = IndexSet(integersIn: range.clamped(to: 0..<table.numberOfRows))
			table.reloadData(forRowIndexes: rows, columnIndexes: IndexSet(integersIn: 0..<table.numberOfColumns))
			labels = [:]
			restoreSelection(scrolling: false)
		}

		func reloadAll() {
			rowElements = [:]
			labels = [:]
			table?.reloadData()
			restoreSelection(scrolling: true)
		}

		func restoreSelection(scrolling: Bool) {
			guard let table else { return }
			let rows = IndexSet(model.selection.compactMap { model.index(of: $0) })
			if rows != table.selectedRowIndexes { table.selectRowIndexes(rows, byExtendingSelection: false) }
			if scrolling, let first = rows.first { table.scrollRowToVisible(first) }
		}

		func accessibilityRows(of table: RegisterNSTable) -> [RegisterRowElement] {
			(0..<model.total).map { index in
				if let element = rowElements[index] { return element }
				let element = RegisterRowElement(table: table, index: index)
				rowElements[index] = element
				return element
			}
		}

		/// What VoiceOver reads for a row, built once per row until the rows reload.
		func accessibilityLabel(of row: RegisterRow) -> String {
			if let label = labels[row.id] { return label }
			let status = row.reconciled ? "Reconciled" : (row.cleared ? "Cleared" : "Uncleared")
			let parts = [
				row.shortDate(thisYear: model.thisYear),
				model.accountID == nil ? row.accountName : nil,
				PayeeText(row).label,
				row.envelopeText,
				row.notes,
				(row.amount > 0 ? "+" : "") + model.format.amount(row.amount, on: .register),
				model.showsBalance ? row.runningBalance.map { "balance \(model.format.amount($0, on: .register))" } : nil,
				status
			]
			let label = parts.compactMap { $0?.isEmpty == false ? $0 : nil }.joined(separator: ", ")
			labels[row.id] = label
			return label
		}

		func toggleCleared(_ row: RegisterRow) {
			guard !row.reconciled, !model.isReadOnly else { return }
			Task { await model.toggleCleared(row) }
		}

		func open(_ row: RegisterRow) {
			parent.open(row)
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
			add(rows.count == 1 ? "Delete Transaction" : "Delete \(rows.count) Transactions", enabled: editable) { [weak self] in self?.parent.delete(rows) }
		}

		private func add(_ title: String, enabled: Bool, _ action: @escaping () -> Void) {
			let item = ClosureMenuItem(title: title, action: action)
			item.isEnabled = enabled
			menu.addItem(item)
		}
	}
}

/// Return opens the selected row and Delete removes it, as in Actual.
///
/// Accessibility reads one element per row from the rows already loaded.
/// AppKit's own rows build every cell of every row when they're read, so on
/// a long register each UI test lookup, and VoiceOver, waited seconds.
final class RegisterNSTable: NSTableView {
	weak var coordinator: RegisterTable.Coordinator?

	override func accessibilityRows() -> [any NSAccessibilityRow]? {
		coordinator?.accessibilityRows(of: self)
	}

	override func accessibilityVisibleRows() -> [any NSAccessibilityRow]? {
		let visible = rows(in: visibleRect)
		return accessibilityRows()?.dropFirst(visible.location).prefix(visible.length).map(\.self)
	}

	override func accessibilitySelectedRows() -> [any NSAccessibilityRow]? {
		let rows = accessibilityRows() ?? []
		return selectedRowIndexes.compactMap { rows.indices.contains($0) ? rows[$0] : nil }
	}

	override func accessibilityChildren() -> [Any]? {
		(accessibilityRows() ?? []) + (accessibilityColumns() ?? [])
	}

	/// Accessibility clients, UI tests and VoiceOver among them, read a table's
	/// rows through this attribute route, where AppKit answers with its own rows
	/// instead of asking `accessibilityRows()`.
	@available(macOS, deprecated: 10.10, message: "AppKit still reads a table's rows here")
	override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
		guard [.rows, .visibleRows, .selectedRows, .children].contains(attribute) else {
			return super.accessibilityAttributeValue(attribute)
		}
		// AppKit calls this on the main thread; the value only passes back to it.
		nonisolated(unsafe) var value: Any?
		MainActor.assumeIsolated {
			value = switch attribute {
			case .rows: accessibilityRows()
			case .visibleRows: accessibilityVisibleRows()
			case .selectedRows: accessibilitySelectedRows()
			default: accessibilityChildren()
			}
		}
		return value
	}

	func screenFrame(_ rect: NSRect) -> NSRect {
		guard let window else { return .zero }
		return window.convertToScreen(convert(rect, to: nil))
	}

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

	private static let body = NSFont.systemFont(ofSize: TextSize.body.rawValue)
	private static let figures = NSFont.monospacedDigitSystemFont(ofSize: TextSize.body.rawValue, weight: .medium)
	private static let amountFont = NSFont.monospacedDigitSystemFont(ofSize: TextSize.body.rawValue, weight: .semibold)
	private static let direction = NSFont.systemFont(ofSize: TextSize.body.rawValue, weight: .semibold)
	private static let payee = NSFont.systemFont(ofSize: TextSize.body.rawValue, weight: .medium)
	private static let income = NSColor(Color(ColorToken.moneyIncome))
	private static let reconciled = NSImage(systemSymbolName: "lock.fill", accessibilityDescription: "Reconciled")
	private static let cleared = NSImage(systemSymbolName: "checkmark.circle.fill", accessibilityDescription: "Cleared")
	private static let uncleared = NSImage(systemSymbolName: "circle", accessibilityDescription: "Uncleared")

	func show(_ row: RegisterRow?, column: RegisterTable.Column, model: RegisterModel, toggle: @escaping (RegisterRow) -> Void) {
		self.row = row
		self.toggle = toggle
		let body = Self.body
		let figures = Self.figures
		label.textColor = .labelColor
		label.font = body
		guard let row else {
			label.stringValue = ""
			button.image = nil
			return
		}
		switch column {
		case .cleared:
			button.image = row.reconciled ? Self.reconciled : (row.cleared ? Self.cleared : Self.uncleared)
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
				text.append(NSAttributedString(string: direction + " ", attributes: [.font: Self.direction, .foregroundColor: NSColor.secondaryLabelColor]))
			}
			text.append(NSAttributedString(string: payee.text, attributes: [.font: Self.payee, .foregroundColor: NSColor.labelColor]))
			label.attributedStringValue = text
		case .envelope:
			label.stringValue = row.envelopeText
			label.textColor = row.amount > 0 && !row.isTransfer ? Self.income : .secondaryLabelColor
		case .notes:
			label.stringValue = row.notes ?? ""
			label.textColor = .secondaryLabelColor
		case .amount:
			label.stringValue = (row.amount > 0 ? "+" : "") + model.format.amount(row.amount, on: .register)
			label.font = Self.amountFont
			label.textColor = row.amount > 0 && !row.isTransfer ? Self.income : .labelColor
		case .balance:
			label.stringValue = row.runningBalance.map { model.format.amount($0, on: .register) } ?? ""
			label.textColor = .secondaryLabelColor
			label.font = figures
		}
	}
}
