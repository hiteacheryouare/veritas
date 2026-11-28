import AppKit
import ObjCExceptions
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// What a UI test or VoiceOver pays to read the register: every element's
/// identifier, label and frame, as an accessibility snapshot reads them.
@MainActor
struct RegisterAccessibilityTests {
	private func register() async -> (RegisterModel, NSView, NSWindow) {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = RegisterModel(workspace: workspace, scope: .account(accountId: "checking"))
		await model.load()
		let host = NSHostingView(rootView: RegisterTable(model: model, open: { _ in }, delete: { _ in }).frame(width: 1100, height: 700))
		host.frame.size = CGSize(width: 1100, height: 700)
		let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.isReleasedWhenClosed = false
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		return (model, host, window)
	}

	private static func table(in view: NSView) -> NSTableView? {
		(view as? NSTableView) ?? view.subviews.lazy.compactMap(table).first
	}

	/// What a snapshot reads, asked the way AppKit's accessibility server asks:
	/// a table through its attribute route, where AppKit answers a table's rows
	/// itself, and an `NSAccessibilityElement` through its methods.
	private static func attribute(_ element: Any, _ name: NSAccessibility.Attribute) -> Any? {
		if let element = element as? NSAccessibilityElement {
			return switch name {
			case .children: element.accessibilityChildren()
			case .identifier: element.accessibilityIdentifier()
			case .title, .description: element.accessibilityLabel()
			case .value: element.accessibilityValue()
			default: nil
			}
		}
		return (element as? NSObject)?.perform(NSSelectorFromString("accessibilityAttributeValue:"), with: name.rawValue)?.takeUnretainedValue()
	}

	@Test func readingEveryRowBuildsNoCellAndFetchesNoPage() async throws {
		let (model, host, window) = await register()
		defer { window.close() }
		let table = try #require(Self.table(in: host))
		#expect(model.total > RegisterModel.pageSize, "more rows than one page: \(model.total)")
		let loaded = model.loadedRows.count
		let rows = try #require(Self.attribute(table, .rows) as? [Any])
		#expect(rows.count == model.total, "every row is there for VoiceOver to reach")

		let start = ContinuousClock.now
		var children = 0
		var payees: Set<String> = []
		var widest = 0
		func read(_ element: Any) {
			_ = Self.attribute(element, .identifier)
			_ = Self.attribute(element, .title)
			if let text = Self.attribute(element, .value) as? String { payees.insert(text) }
			for child in (Self.attribute(element, .children) as? [Any]) ?? [] {
				children += 1
				read(child)
			}
		}
		for row in rows {
			let before = children
			read(row)
			widest = max(widest, children - before)
		}
		let elapsed = ContinuousClock.now - start
		try await Task.sleep(for: .milliseconds(300))
		print("REGISTER-A11Y rows \(rows.count), elements under them \(children), read in \(elapsed)")
		#expect(widest <= 2, "a row is its payee and its cleared toggle: \(widest)")

		#expect(payees.isSuperset(of: ["→ To Savings", "← From Savings"]), "the payees UI tests look for")
		#expect(!payees.contains("Savings"), "a transfer never reads as a bare payee")
		#expect(model.loadedRows.count == loaded, "reading the register fetched no page")
		#expect(elapsed < .seconds(1), "AppKit's own rows took 0.6 to 4.4 s here, building every row's cells")
	}

	@Test func aPageArrivingInTheBackgroundLeavesTheScrollAlone() async throws {
		let (model, host, window) = await register()
		defer { window.close() }
		let table = try #require(Self.table(in: host))
		let selected = try #require(model.loadedRow(at: 2))
		model.selection = [selected.id]
		table.selectRowIndexes([2], byExtendingSelection: false)
		table.scrollRowToVisible(200)
		host.layoutSubtreeIfNeeded()
		let loaded = model.loadedRows.count
		_ = model.row(at: RegisterModel.pageSize + 10)
		while model.loadedRows.count == loaded { try await Task.sleep(for: .milliseconds(10)) }
		host.layoutSubtreeIfNeeded()
		#expect(table.rows(in: table.visibleRect).contains(200), "the register stays where it was scrolled")
		#expect(!table.rows(in: table.visibleRect).contains(2), "and doesn't jump back to the selection")
		#expect(table.selectedRowIndexes == [2], "the selection is kept")
	}

	@Test func aRowReadsItsDatePayeeEnvelopeNotesAmountAndStatus() async throws {
		let (model, host, window) = await register()
		defer { window.close() }
		let table = try #require(Self.table(in: host))
		let index = try #require((0..<model.loadedRows.count).first { model.loadedRow(at: $0)?.notes?.isEmpty == false && model.loadedRow(at: $0)?.reconciled == false })
		let row = try #require(model.loadedRow(at: index))
		let label = { ((table.accessibilityRows() ?? [])[index] as? NSAccessibilityElement)?.accessibilityLabel() ?? "" }
		let status = { (cleared: Bool) in cleared ? "Cleared" : "Uncleared" }
		for part in [row.shortDate(thisYear: model.thisYear), PayeeText(row).label, row.envelopeText, row.notes ?? "", model.format.amount(row.amount, on: .register), status(row.cleared)] {
			#expect(label().contains(part), "\(part) in \(label())")
		}
		#expect(await model.setCleared([row.id], !row.cleared))
		await model.load()
		let deadline = ContinuousClock.now + .seconds(5)
		while !label().hasSuffix(status(!row.cleared)), ContinuousClock.now < deadline {
			try await Task.sleep(for: .milliseconds(20))
		}
		#expect(label().hasSuffix(status(!row.cleared)), "the label follows the change: \(label())")
		let cleared = try #require(((table.accessibilityRows() ?? [])[index] as? NSAccessibilityElement)?.accessibilityChildren()?.last as? NSAccessibilityElement)
		#expect(cleared.accessibilityLabel() == status(!row.cleared))
	}

	@Test func theRowListRaisesForAnIndexPastItsEndAndComparesWithoutMakingRows() {
		var made = 0
		let list = RegisterRowList(rows: 2, then: ["column"]) { index in made += 1; return "row \(index)" }
		#expect(list.count == 3)
		#expect(ObjCExceptions.catching { _ = list.object(at: 3) }?.name == .rangeException, "as NSArray does, rather than a trap")
		#expect(ObjCExceptions.catching { _ = list.object(at: -1) }?.name == .rangeException)
		var last: Any?
		#expect(ObjCExceptions.catching { last = list.object(at: 2) } == nil)
		#expect(last as? String == "column")
		made = 0
		let other = RegisterRowList(rows: 2) { _ in made += 1; return "row" }
		#expect(!list.isEqual(other))
		#expect(list.isEqual(list))
		#expect(made == 0, "comparing lists made no row")
	}
}
