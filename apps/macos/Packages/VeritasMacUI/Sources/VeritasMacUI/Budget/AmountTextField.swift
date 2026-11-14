import AppKit
import SwiftUI
import VeritasDesign
import VeritasKit

/// The amount field every screen uses (R-BUD-3). It opens with the value
/// selected, shapes each keystroke through the core, and reports Return, Tab
/// and Esc. It's an NSTextField because it must take focus the moment it
/// appears, which a SwiftUI field inserted mid-update can't.
struct AmountTextField: NSViewRepresentable {
	enum Exit {
		case next
		case previous
		case done
		case cancel
	}

	let field: AmountFieldModel
	var alignment: NSTextAlignment = .right
	/// Off in a form whose first field is something else.
	var takesFocus = true
	let exit: (Exit) -> Void

	func makeNSView(context: Context) -> FocusingTextField {
		let view = FocusingTextField()
		view.takesFocus = takesFocus
		view.stringValue = field.text
		view.alignment = alignment
		view.font = NSFont.monospacedDigitSystemFont(ofSize: TextSize.body.rawValue, weight: .medium)
			.withDesign(.rounded)
		view.delegate = context.coordinator
		view.bezelStyle = .roundedBezel
		view.focusRingType = .exterior
		view.setAccessibilityIdentifier("amountField")
		return view
	}

	/// Gives up first responder as it goes, so its field editor isn't left
	/// holding the window's keys.
	static func dismantleNSView(_ view: FocusingTextField, coordinator: Coordinator) {
		if view.currentEditor() != nil { view.window?.makeFirstResponder(nil) }
	}

	func updateNSView(_ view: FocusingTextField, context: Context) {
		context.coordinator.parent = self
		if view.stringValue != field.text {
			view.stringValue = field.text
			view.currentEditor()?.selectedRange = NSRange(location: field.text.utf16.count, length: 0)
		}
		view.hasProblem = field.problem != nil
	}

	func makeCoordinator() -> Coordinator {
		Coordinator(parent: self)
	}

	@MainActor
	final class Coordinator: NSObject, NSTextFieldDelegate {
		var parent: AmountTextField

		init(parent: AmountTextField) {
			self.parent = parent
		}

		func controlTextDidChange(_ notification: Notification) {
			guard let view = notification.object as? NSTextField else { return }
			parent.field.type(view.stringValue)
		}

		func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
			let shift = NSApp.currentEvent?.modifierFlags.contains(.shift) == true
			switch selector {
			case #selector(NSResponder.insertNewline(_:)):
				parent.exit(shift ? .previous : .next)
			case #selector(NSResponder.insertTab(_:)):
				parent.exit(.next)
			case #selector(NSResponder.insertBacktab(_:)):
				parent.exit(.previous)
			case #selector(NSResponder.cancelOperation(_:)):
				parent.exit(.cancel)
			default:
				return false
			}
			return true
		}
	}
}

final class FocusingTextField: NSTextField {
	var takesFocus = true

	var hasProblem = false {
		didSet { layer?.borderWidth = hasProblem ? 1.5 : 0 }
	}

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		guard let window else { return }
		wantsLayer = true
		layer?.cornerRadius = Radius.field
		layer?.borderColor = NSColor(Color(ColorToken.moneyOverspent)).cgColor
		guard takesFocus else { return }
		window.makeFirstResponder(self)
		currentEditor()?.selectAll(nil)
	}
}

private extension NSFont {
	func withDesign(_ design: NSFontDescriptor.SystemDesign) -> NSFont {
		fontDescriptor.withDesign(design).flatMap { NSFont(descriptor: $0, size: pointSize) } ?? self
	}
}
