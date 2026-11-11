#if DEBUG
import Foundation
import VeritasKit

/// `-VeritasRealCore YES` runs the real core in a scratch budgets folder inside
/// the app's temporary directory, emptied at each such launch, so UI tests can
/// create real budgets without touching the user's own.
enum RealCoreLaunch {
	static let isOn = UserDefaults.standard.bool(forKey: "VeritasRealCore")

	static let budgetsDirectory: URL = {
		let folder = FileManager.default.temporaryDirectory.appending(path: "ui-test-budgets", directoryHint: .isDirectory)
		try? FileManager.default.removeItem(at: folder)
		return folder
	}()
}
#endif
