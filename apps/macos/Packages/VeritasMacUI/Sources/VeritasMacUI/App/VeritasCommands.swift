import AppKit
import SwiftUI
import VeritasKit

struct VeritasCommands: Commands {
	@FocusedValue(\.workspace) private var model
	@Environment(\.openWindow) private var openWindow

	private var isOpen: Bool { model?.phase == .open }

	var body: some Commands {
		CommandGroup(replacing: .appSettings) {
			Button("Settings…") { model?.go(to: .screen(.settings)) }
				.keyboardShortcut(",")
				.disabled(!isOpen)
		}
		CommandGroup(replacing: .newItem) {
			Button("Add Transaction…") { model?.perform(.addTransaction) }
				.keyboardShortcut("n")
				.disabled(!isOpen)
			Button("New Window") { openWindow(id: WorkspaceWindow.id) }
				.keyboardShortcut("n", modifiers: [.command, .option])
			Divider()
			Button("Import from Actual…") { model?.isImportPanelShown = true }
				.disabled(model == nil || model?.phase == .working)
			Button("Export to Actual…") {
				guard let model else { return }
				Task { await ExportPanel.run(for: model) }
			}
			.disabled(!isOpen || model?.canExport != true)
			Button("Close Budget") {
				guard let model else { return }
				Task { await model.closeBudget() }
			}
			.disabled(!isOpen)
		}
		CommandGroup(replacing: .undoRedo) {
			Button("Undo") { undo(redo: false) }
				.keyboardShortcut("z")
				.disabled(model?.canUndo != true)
			Button("Redo") { undo(redo: true) }
				.keyboardShortcut("z", modifiers: [.command, .shift])
				.disabled(model?.canRedo != true)
		}
		CommandGroup(after: .pasteboard) {
			Divider()
			Button("Find") { model?.isFindShown = true }
				.keyboardShortcut("f")
				.disabled(!isOpen)
		}
		CommandGroup(before: .toolbar) {
			Toggle("Dense", isOn: Binding(get: { model?.isDense ?? false }, set: { _ in model?.toggleDense() }))
				.keyboardShortcut("d")
				.disabled(model?.canToggleDense != true)
			Divider()
		}
		CommandMenu("Go") {
			ForEach(Screen.sections, id: \.self) { screen in
				Button(screen.title) { model?.go(to: .screen(screen)) }
					.keyboardShortcut(KeyEquivalent(Character(String(screen.shortcutNumber ?? 0))))
					.disabled(!isOpen)
			}
			Divider()
			Button("Go to Anything…") { model?.isPaletteShown = true }
				.keyboardShortcut("k")
				.disabled(!isOpen)
		}
	}

	/// While a text field is editing, ⌘Z belongs to its own text undo.
	private func undo(redo: Bool) {
		if let editor = NSApp.keyWindow?.firstResponder as? NSTextView, let undoManager = editor.undoManager,
			redo ? undoManager.canRedo : undoManager.canUndo
		{
			redo ? undoManager.redo() : undoManager.undo()
			return
		}
		guard let model else { return }
		Task { redo ? await model.redo() : await model.undo() }
	}
}
