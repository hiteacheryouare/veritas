import AppKit

extension NSSavePanel {
	/// Opens as a sheet on the window that asked, like the statement and tax exports.
	@MainActor
	func beginOnKeyWindow() async -> NSApplication.ModalResponse {
		if let window = NSApp.keyWindow {
			await beginSheetModal(for: window)
		} else {
			await begin()
		}
	}
}
