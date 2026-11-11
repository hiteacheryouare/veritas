import Foundation
import VeritasKit

/// Keeps each budget's backup folder as a security-scoped bookmark, so the
/// sandboxed app can write to it again after a relaunch (R-REL-4).
final class BookmarkedBudgetSettings: LocalBudgetSettings, @unchecked Sendable {
	private let defaults: UserDefaults
	private let lock = NSLock()
	private var accessing: [URL: URL] = [:]

	init(defaults: UserDefaults = .standard) {
		self.defaults = defaults
	}

	func backupFolder(for budget: URL) -> URL? {
		lock.withLock {
			if let folder = accessing[budget] { return folder }
			guard let bookmark = bookmarks[key(budget)] else { return nil }
			var isStale = false
			guard let folder = try? URL(resolvingBookmarkData: bookmark, options: .withSecurityScope, bookmarkDataIsStale: &isStale),
				folder.startAccessingSecurityScopedResource()
			else { return nil }
			if isStale, let fresh = try? folder.bookmarkData(options: .withSecurityScope) {
				var all = bookmarks
				all[key(budget)] = fresh
				bookmarks = all
			}
			accessing[budget] = folder
			return folder
		}
	}

	func rememberBackupFolder(_ folder: URL?, for budget: URL) throws {
		let bookmark = try folder?.bookmarkData(options: .withSecurityScope)
		lock.withLock {
			accessing.removeValue(forKey: budget)?.stopAccessingSecurityScopedResource()
			var all = bookmarks
			all[key(budget)] = bookmark
			bookmarks = all
		}
	}

	func backupSchedule(for budget: URL) -> BackupSchedule {
		let raw = (defaults.dictionary(forKey: "backupSchedules") as? [String: String])?[key(budget)]
		return raw.flatMap(BackupSchedule.init(rawValue:)) ?? .off
	}

	func setBackupSchedule(_ schedule: BackupSchedule, for budget: URL) {
		var all = defaults.dictionary(forKey: "backupSchedules") as? [String: String] ?? [:]
		all[key(budget)] = schedule.rawValue
		defaults.set(all, forKey: "backupSchedules")
	}

	private var bookmarks: [String: Data] {
		get { defaults.dictionary(forKey: "backupFolderBookmarks") as? [String: Data] ?? [:] }
		set { defaults.set(newValue, forKey: "backupFolderBookmarks") }
	}

	private func key(_ budget: URL) -> String {
		budget.standardizedFileURL.path(percentEncoded: false)
	}
}
