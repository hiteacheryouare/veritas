import Foundation
import Observation

/// Budgets opened on this Mac, most recent first, for the sidebar's budget
/// menu and File › Open Recent. An app preference: nothing is written into a
/// budget's folder.
@MainActor
@Observable
public final class RecentBudgets {
	public struct Entry: Codable, Hashable, Sendable, Identifiable {
		public let name: String?
		public let folder: URL

		public init(name: String?, folder: URL) {
			self.name = name
			self.folder = folder
		}

		public var id: String { RecentBudgets.key(folder) }

		/// The folder by its last two parts, "budgets/6F1C2A9E-…", enough to
		/// tell two budgets with the same name apart.
		public var place: String {
			folder.pathComponents.suffix(2).joined(separator: "/")
		}
	}

	/// Where a recent budget's folder is now.
	public enum Location: Sendable {
		case here
		/// On a drive or share that isn't connected, so it may come back.
		case unmounted
		case gone
	}

	public static let limit = 10
	static let defaultsKey = "recentBudgets"

	public private(set) var entries: [Entry]

	@ObservationIgnored private let defaults: UserDefaults?
	@ObservationIgnored private let locate: (URL) -> Location

	/// Without `defaults` the list lives only as long as this object.
	public init(defaults: UserDefaults?, locate: @escaping (URL) -> Location = RecentBudgets.location) {
		self.defaults = defaults
		self.locate = locate
		entries = defaults?.data(forKey: Self.defaultsKey).flatMap { try? JSONDecoder().decode([Entry].self, from: $0) } ?? []
		forgetMissing()
	}

	/// The menus' list: every recent budget but the one this window has open.
	public func entries(besides folder: URL?) -> [Entry] {
		guard let folder else { return entries }
		let current = Self.key(folder)
		return entries.filter { $0.id != current }
	}

	public func opened(name: String?, folder: URL) {
		let entry = Entry(name: name, folder: folder)
		save([entry] + entries.filter { $0.id != entry.id })
	}

	/// Keeps a budget's place in the list while its name changes.
	public func renamed(_ name: String?, folder: URL) {
		guard let index = entries.firstIndex(where: { $0.id == Self.key(folder) }), entries[index].name != name else { return }
		var renamed = entries
		renamed[index] = Entry(name: name, folder: entries[index].folder)
		save(renamed)
	}

	/// A budget on a drive that isn't connected stays in the list, unavailable.
	public func isAvailable(_ entry: Entry) -> Bool {
		locate(entry.folder) == .here
	}

	/// Drops budgets whose folder is gone from a connected drive.
	public func forgetMissing() {
		let kept = entries.filter { locate($0.folder) != .gone }
		if kept != entries { save(kept) }
	}

	/// A folder under `/Volumes` whose volume isn't mounted is `unmounted`.
	public nonisolated static func location(_ folder: URL) -> Location {
		let files = FileManager.default
		if files.fileExists(atPath: folder.path(percentEncoded: false)) { return .here }
		let parts = folder.standardizedFileURL.pathComponents
		guard parts.count > 2, parts[1] == "Volumes" else { return .gone }
		return files.fileExists(atPath: "/Volumes/\(parts[2])") ? .gone : .unmounted
	}

	public func clear() {
		save([])
	}

	private func save(_ list: [Entry]) {
		entries = Array(list.prefix(Self.limit))
		defaults?.set(try? JSONEncoder().encode(entries), forKey: Self.defaultsKey)
	}

	nonisolated static func key(_ folder: URL) -> String {
		folder.standardizedFileURL.path(percentEncoded: false).trimmingSuffix("/")
	}
}

private extension String {
	func trimmingSuffix(_ suffix: String) -> String {
		hasSuffix(suffix) && count > suffix.count ? String(dropLast(suffix.count)) : self
	}
}
