import AppKit
import SwiftUI
import Testing
import VeritasCore
import VeritasKit
@testable import VeritasMacUI

/// Typing an amount into the Move money sheet on the stub budget, as
/// AccountsJourneyTests does with UI automation (R-XFER-1).
@MainActor
struct MoveMoneyAfterTests {
	@Test func typingAnAmountShowsTheBalanceAfterTheMove() async throws {
		let workspace = WorkspaceModel(library: StubLibrary(readOnly: nil, importError: nil), localSettings: InMemoryBudgetSettings(), format: MoneyFormat())
		await workspace.reopen(directory: StubLaunch.budgetDirectory)
		let model = MoveMoneyModel(workspace: workspace, fromID: "checking")
		let sheet = try await HostedSheet(model: model)
		model.toID = "brokerage"
		try await sheet.waitUntil { model.preview != nil }
		#expect(!model.showsAfter, "an empty amount has no After yet")
		let before = try #require(model.preview?.to)
		#expect(MoveMoneySheet(model: model).balanceLabel(before) == "Now $31,800.00")

		try await sheet.type("500")
		try await sheet.waitUntil { model.preview?.to.after == 3_230_000 }
		#expect(model.showsAfter)
		let after = try #require(model.preview?.to)
		#expect(MoveMoneySheet(model: model).balanceLabel(after) == "Now $31,800.00, After $32,300.00")
	}
}

/// The sheet in an offscreen window, so its `.task(id: previewKey)` runs on
/// each keystroke as it does in the app.
@MainActor
private struct HostedSheet {
	let window: NSWindow
	let host: NSView

	init(model: MoveMoneyModel) async throws {
		let host = NSHostingView(rootView: MoveMoneySheet(model: model))
		host.frame.size = CGSize(width: 600, height: 520)
		window = NSWindow(contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
		window.contentView = host
		self.host = host
		try await waitUntil { !model.accounts.isEmpty }
	}

	func type(_ keys: String) async throws {
		let field = try #require(Self.amountField(in: host))
		let editor = try #require(field.currentEditor() as? NSTextView, "the amount field took focus when the sheet appeared")
		for key in keys {
			editor.insertText(String(key), replacementRange: editor.selectedRange())
			try await settle()
		}
	}

	func waitUntil(_ condition: () -> Bool) async throws {
		for _ in 0..<400 where !condition() {
			try await settle()
		}
		try #require(condition())
	}

	private func settle() async throws {
		try await Task.sleep(for: .milliseconds(5))
		host.layoutSubtreeIfNeeded()
	}

	private static func amountField(in view: NSView) -> FocusingTextField? {
		if let field = view as? FocusingTextField { return field }
		return view.subviews.lazy.compactMap(amountField(in:)).first
	}
}

struct LoanShortfallWordsTests {
	@Test func aPaymentShortOfItsPartsSaysByHowMuch() {
		// DEBT-2.2: $50 against $30 interest and $40 escrow is $20 short.
		let dollars = MoneyFormat(currencySymbol: "$", decimalPlaces: 2)
		#expect(MoveMoneySheet.shortText(2_000, format: dollars) == "This payment is $20.00 less than its interest, escrow and fees.")
	}
}
