import SwiftUI

/// The whole app. The Xcode project's only source file puts this in its `App`.
public struct VeritasScenes: Scene {
	public init() {}

	public var body: some Scene {
		WindowGroup(id: WorkspaceWindow.id) {
			WorkspaceWindow()
		}
		.defaultSize(width: 1280, height: 800)
		.commands {
			VeritasCommands()
		}
	}
}
