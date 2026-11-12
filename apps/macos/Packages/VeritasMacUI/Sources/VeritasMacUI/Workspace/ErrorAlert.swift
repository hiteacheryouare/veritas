import AppKit
import SwiftUI
import VeritasKit

extension View {
	func errorAlert(model: WorkspaceModel) -> some View {
		modifier(ErrorAlertModifier(model: model))
	}
}

private struct ErrorAlertModifier: ViewModifier {
	@Bindable var model: WorkspaceModel

	func body(content: Content) -> some View {
		content.alert(
			model.alert?.title ?? "",
			isPresented: Binding(get: { model.alert != nil }, set: { if !$0 { model.alert = nil } }),
			presenting: model.alert
		) { alert in
			if alert.offersRetry {
				Button("Try again") { Task { await model.retryFailedAction() } }
					.keyboardShortcut(.defaultAction)
				Button("Cancel", role: .cancel) {}
			} else if let details = alert.details {
				Button("Copy details") { copy(details) }
				Button("Close", role: .cancel) {}
			} else {
				Button("Close", role: .cancel) {}
			}
		} message: { alert in
			if let message = alert.message {
				Text(message)
			}
		}
	}

	private func copy(_ details: String) {
		NSPasteboard.general.clearContents()
		NSPasteboard.general.setString(details, forType: .string)
	}
}
