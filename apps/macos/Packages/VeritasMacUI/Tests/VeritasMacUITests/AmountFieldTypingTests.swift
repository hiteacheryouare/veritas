import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// Keystrokes into the real AmountTextField in an offscreen window, with the
/// core's amount reader (R-BUD-3, design/interaction.md "Arithmetic fields").
/// Needs no UI automation permission.
@MainActor
struct AmountFieldTypingTests {
	@Test func clickingTheCellThenTypingPlusFortyAppendsToTheValue() async throws {
		let field = AmountFieldModel(amount: 14_000, style: .commaDot, reader: CoreAmountReader(budgetDecimalPlaces: 2))
		let typing = try Typing(into: field)
		#expect(typing.view.stringValue == "140.00")
		try await typing.type("+40")
		#expect(field.text == "140.00+40")
		#expect(await field.commit() == 18_000)
	}

	@Test func clickingTheCellThenTypingANumberReplacesTheValue() async throws {
		let field = AmountFieldModel(amount: 14_000, style: .commaDot, reader: CoreAmountReader(budgetDecimalPlaces: 2))
		let typing = try Typing(into: field)
		try await typing.type("75")
		#expect(await field.commit() == 7_500)
	}

	/// BudgetScreen's key handler opens the field with the first key already
	/// typed (`beginEditing`, then `editing?.type(press.characters)`), so the
	/// field must open with the caret after it for the next key to append.
	@Test func typingPlusFortyOnASelectedEnvelopeAppendsToItsValue() async throws {
		let field = AmountFieldModel(amount: 14_000, style: .commaDot, reader: CoreAmountReader(budgetDecimalPlaces: 2))
		field.type("+")
		let typing = try Typing(into: field)
		try await typing.type("40")
		#expect(field.text == "140.00+40")
		#expect(await field.commit() == 18_000)
	}

	@Test func typingSeventyFiveOnASelectedEnvelopeSetsSeventyFive() async throws {
		let field = AmountFieldModel(amount: 14_000, style: .commaDot, reader: CoreAmountReader(budgetDecimalPlaces: 2))
		field.type("7")
		let typing = try Typing(into: field)
		try await typing.type("5")
		#expect(field.text == "75")
		#expect(await field.commit() == 7_500)
	}
}

/// Hosts the field as the Budget screen does and types into its field editor,
/// which is where a key press lands once the field has focus.
@MainActor
private struct Typing {
	let window: NSWindow
	let view: FocusingTextField
	let field: AmountFieldModel

	init(into field: AmountFieldModel) throws {
		self.field = field
		let host = NSHostingView(rootView: AmountTextField(field: field) { _ in }.frame(width: 140, height: 26))
		host.frame.size = CGSize(width: 200, height: 40)
		window = NSWindow(contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
		window.contentView = host
		host.layoutSubtreeIfNeeded()
		view = try #require(Self.field(in: host))
		try #require(view.currentEditor() != nil, "the field took focus when it appeared")
	}

	func type(_ keys: String) async throws {
		for key in keys {
			let editor = try #require(view.currentEditor() as? NSTextView)
			editor.insertText(String(key), replacementRange: NSRange(location: NSNotFound, length: 0))
			try await settle()
		}
	}

	/// Waits for SwiftUI to push the model's reshaped text back into the field.
	private func settle() async throws {
		for _ in 0..<200 where view.stringValue != field.text {
			try await Task.sleep(for: .milliseconds(5))
			window.contentView?.layoutSubtreeIfNeeded()
		}
		try #require(view.stringValue == field.text, "the field shows what the model holds")
	}

	private static func field(in view: NSView) -> FocusingTextField? {
		if let field = view as? FocusingTextField { return field }
		return view.subviews.lazy.compactMap(field(in:)).first
	}
}
