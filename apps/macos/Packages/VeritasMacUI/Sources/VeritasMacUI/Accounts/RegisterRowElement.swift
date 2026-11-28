import AppKit
import VeritasCore
import VeritasKit

/// One register row for accessibility, read from the rows the register has
/// loaded. It never builds a cell or fetches a page. A row that isn't loaded
/// yet reads as loading until its page arrives. AppKit asks for accessibility
/// on the main thread, so these elements read the main-actor model directly.
final class RegisterRowElement: NSAccessibilityElement, NSAccessibilityRow {
	private weak var table: RegisterNSTable?
	private let index: Int
	private var children: (id: String, elements: [Any])?

	init(table: RegisterNSTable, index: Int) {
		self.table = table
		self.index = index
		super.init()
	}

	private var row: RegisterRow? {
		let (table, index) = (table, index)
		return MainActor.assumeIsolated { table?.coordinator?.model.loadedRow(at: index) }
	}

	override func accessibilityRole() -> NSAccessibility.Role? { .row }
	override func accessibilityParent() -> Any? { table }
	override func accessibilityIndex() -> Int { index }
	override func isAccessibilityElement() -> Bool { true }

	override func accessibilityIdentifier() -> String {
		row.map { "register.row.\($0.id)" } ?? ""
	}

	override func accessibilityLabel() -> String? {
		guard let row, let coordinator = table?.coordinator else { return "Loading" }
		return MainActor.assumeIsolated { coordinator.accessibilityLabel(of: row) }
	}

	override func accessibilityFrame() -> NSRect {
		guard let table else { return .zero }
		return table.screenFrame(table.rect(ofRow: index))
	}

	override func isAccessibilitySelected() -> Bool {
		table?.selectedRowIndexes.contains(index) ?? false
	}

	override func setAccessibilitySelected(_ selected: Bool) {
		guard let table else { return }
		if selected {
			table.selectRowIndexes([index], byExtendingSelection: false)
		} else {
			table.deselectRow(index)
		}
	}

	override func accessibilityPerformPress() -> Bool {
		guard let row, let coordinator = table?.coordinator else { return false }
		MainActor.assumeIsolated { coordinator.open(row) }
		return true
	}

	/// The payee, as the register shows it, and the cleared toggle.
	override func accessibilityChildren() -> [Any]? {
		guard let row, let table else { return [] }
		if let children, children.id == row.id { return children.elements }
		let elements: [Any] = [
			RegisterCellElement(table: table, row: index, column: .payee, text: PayeeText(row).label, parent: self),
			RegisterClearedElement(table: table, row: row, index: index, parent: self)
		]
		children = (row.id, elements)
		return elements
	}
}

/// A cell's text, framed where the cell draws it.
final class RegisterCellElement: NSAccessibilityElement {
	private weak var table: RegisterNSTable?
	private weak var row: RegisterRowElement?
	private let index: Int
	private let column: RegisterTable.Column
	private let text: String

	init(table: RegisterNSTable, row index: Int, column: RegisterTable.Column, text: String, parent: RegisterRowElement) {
		self.table = table
		self.row = parent
		self.index = index
		self.column = column
		self.text = text
		super.init()
	}

	override func accessibilityRole() -> NSAccessibility.Role? { .staticText }
	override func accessibilityParent() -> Any? { row }
	override func accessibilityLabel() -> String? { text }
	override func accessibilityValue() -> Any? { text }
	override func isAccessibilityElement() -> Bool { true }

	override func accessibilityFrame() -> NSRect {
		guard let table else { return .zero }
		let column = table.column(withIdentifier: NSUserInterfaceItemIdentifier(column.rawValue))
		return table.screenFrame(column < 0 ? table.rect(ofRow: index) : table.frameOfCell(atColumn: column, row: index))
	}
}

/// The cleared toggle, as the first column's button. It reads the row as it
/// is now, so a toggle reads its new state as soon as the rows reload.
final class RegisterClearedElement: NSAccessibilityElement {
	private weak var table: RegisterNSTable?
	private weak var parentRow: RegisterRowElement?
	private let id: String
	private let index: Int

	init(table: RegisterNSTable, row: RegisterRow, index: Int, parent: RegisterRowElement) {
		self.table = table
		self.id = row.id
		self.index = index
		self.parentRow = parent
		super.init()
	}

	private var row: RegisterRow? {
		let (table, index, id) = (table, index, id)
		return MainActor.assumeIsolated { table?.coordinator?.model.loadedRow(at: index).flatMap { $0.id == id ? $0 : nil } }
	}

	override func accessibilityRole() -> NSAccessibility.Role? { .button }
	override func accessibilityParent() -> Any? { parentRow }
	override func accessibilityIdentifier() -> String { "register.cleared.\(id)" }
	override func isAccessibilityElement() -> Bool { true }

	override func accessibilityLabel() -> String? {
		guard let row else { return nil }
		return row.reconciled ? "Reconciled" : (row.cleared ? "Cleared" : "Uncleared")
	}

	override func isAccessibilityEnabled() -> Bool {
		guard let row, !row.reconciled else { return false }
		let table = table
		return MainActor.assumeIsolated { table?.coordinator?.model.isReadOnly == false }
	}

	override func accessibilityFrame() -> NSRect {
		guard let table else { return .zero }
		let column = table.column(withIdentifier: NSUserInterfaceItemIdentifier(RegisterTable.Column.cleared.rawValue))
		return table.screenFrame(column < 0 ? .zero : table.frameOfCell(atColumn: column, row: index))
	}

	override func accessibilityPerformPress() -> Bool {
		guard isAccessibilityEnabled(), let row else { return false }
		let table = table
		MainActor.assumeIsolated { table?.coordinator?.toggleCleared(row) }
		return true
	}
}

/// The register's rows as AppKit lists them. Each row's element is made when
/// it's first read, so listing 100,000 rows after a reload costs nothing.
final class RegisterRowList: NSArray {
	private var rows = 0
	private var extra: [Any] = []
	private var row: (Int) -> Any = { _ in NSNull() }

	// Overriding every designated initializer lets Swift inherit NSArray's
	// required array-literal initializer, which a subclass can't declare.
	override init() { super.init() }
	override init(objects: UnsafePointer<AnyObject>?, count: Int) { super.init(objects: objects, count: count) }
	required init?(coder: NSCoder) { nil }

	convenience init(rows: Int, then extra: [Any] = [], row: @escaping (Int) -> Any) {
		self.init()
		self.rows = rows
		self.extra = extra
		self.row = row
	}

	override var count: Int { rows + extra.count }

	/// Immutable, so a copy is the list itself rather than every row made at once.
	override func copy(with zone: NSZone? = nil) -> Any { self }

	override func object(at index: Int) -> Any {
		guard (0..<count).contains(index) else {
			NSException(name: .rangeException, reason: "index \(index) beyond bounds of \(count) register rows", userInfo: nil).raise()
			return NSNull()
		}
		return index < rows ? row(index) : extra[index - rows]
	}

	/// Only the same list is equal, so comparing two lists never makes every
	/// row's element to compare them one by one.
	override func isEqual(_ object: Any?) -> Bool {
		(object as AnyObject?) === self
	}
}
