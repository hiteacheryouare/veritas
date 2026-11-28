import SwiftUI
import VeritasDesign
import VeritasKit

/// The whole app. The Xcode project's only source file puts this in its `App`.
public struct VeritasScenes: Scene {
	@State private var assistant = AssistantSettings()

	public init() {}

	public var body: some Scene {
		WindowGroup(id: WorkspaceWindow.id) {
			WorkspaceWindow()
				.environment(assistant)
				.onAppear { AppearanceSetting.stored.apply() }
		}
		.defaultSize(width: 1280, height: 800)
		.commands {
			VeritasCommands()
		}
		Window("About Veritas", id: AboutView.windowID) {
			AboutView()
				.tint(Color.accent)
		}
		.windowResizability(.contentSize)
		.windowStyle(.hiddenTitleBar)
		.restorationBehavior(.disabled)
		.defaultPosition(.center)
	}
}
