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
	/// How many windows hold each folder now, so a new window doesn't open one
	/// again, and closing one of two windows on a budget doesn't forget the other.
	@ObservationIgnored private var holders: [String: Int] = [:]
	/// Where each folder was when the list last changed. The menus read this,
	/// so drawing them never waits on a drive or share that isn't answering.
	@ObservationIgnored private var locations: [String: Location] = [:]

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

	/// A window holds this folder from before it starts opening it until it
	/// closes it, so a second window can't pick the same budget meanwhile.
	public func hold(_ folder: URL) {
		holders[Self.key(folder), default: 0] += 1
	}

	public func release(_ folder: URL) {
		let key = Self.key(folder)
		guard let count = holders[key] else { return }
		holders[key] = count > 1 ? count - 1 : nil
	}

	/// What a window with nothing to restore opens: the most recent budget,
	/// if its folder is here and no window holds it.
	public var budgetToReopen: Entry? {
		guard let latest = entries.first, holders[latest.id] == nil, locate(latest.folder) == .here else { return nil }
		return latest
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
		locations[entry.id, default: .here] == .here
	}

	/// Looks for every folder again, drops those gone from a connected drive,
	/// and remembers where the rest are for the menus.
	public func forgetMissing() {
		locations = Dictionary(entries.map { ($0.id, locate($0.folder)) }, uniquingKeysWith: { first, _ in first })
		let kept = entries.filter { locations[$0.id] != .gone }
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
		for entry in list where locations[entry.id] == nil {
			locations[entry.id] = locate(entry.folder)
		}
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
