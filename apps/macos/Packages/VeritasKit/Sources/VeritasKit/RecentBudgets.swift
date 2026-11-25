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

	public static let limit = 10
	static let defaultsKey = "recentBudgets"

	public private(set) var entries: [Entry]

	@ObservationIgnored private let defaults: UserDefaults?
	@ObservationIgnored private let exists: (URL) -> Bool

	/// Without `defaults` the list lives only as long as this object.
	public init(
		defaults: UserDefaults?,
		exists: @escaping (URL) -> Bool = { FileManager.default.fileExists(atPath: $0.path(percentEncoded: false)) }
	) {
		self.defaults = defaults
		self.exists = exists
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

	public func forgetMissing() {
		let present = entries.filter { exists($0.folder) }
		if present != entries { save(present) }
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
