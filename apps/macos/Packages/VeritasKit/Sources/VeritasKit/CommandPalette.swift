import Foundation

public enum PaletteCommand: String, CaseIterable, Sendable {
	case addTransaction
	case moveMoney
	case newBill

	public var title: String {
		switch self {
		case .addTransaction: "Add transaction"
		case .moveMoney: "Move money"
		case .newBill: "New bill"
		}
	}

	public var sheet: WorkspaceSheet {
		switch self {
		case .addTransaction: .addTransaction
		case .moveMoney: .moveMoney
		case .newBill: .newBill
		}
	}
}

public struct PaletteItem: Hashable, Identifiable, Sendable {
	public enum Target: Hashable, Sendable {
		case go(Destination)
		case perform(PaletteCommand)
	}

	public let title: String
	public let detail: String?
	public let target: Target

	public var id: Target { target }
}

/// Everything ⌘K can reach, and the matching for what's typed into it.
public struct PaletteIndex: Sendable {
	public let items: [PaletteItem]

	public init(accounts: [SidebarAccount], format: MoneyFormat) {
		let commands = PaletteCommand.allCases.map {
			PaletteItem(title: $0.title, detail: $0 == .addTransaction ? "⌘N" : nil, target: .perform($0))
		}
		let screens = Screen.allCases.map {
			PaletteItem(title: $0.title, detail: nil, target: .go(.screen($0)))
		}
		let accountItems = accounts.map {
			PaletteItem(title: $0.name, detail: format.amount($0.balance), target: .go(.account(id: $0.id)))
		}
		items = commands + screens + accountItems
	}

	/// Title prefix first, then the start of any word, then anywhere in the title.
	/// Ties keep the index's order.
	public func results(for query: String) -> [PaletteItem] {
		let needle = Self.fold(query.trimmingCharacters(in: .whitespacesAndNewlines))
		guard !needle.isEmpty else { return items }
		return items.enumerated()
			.compactMap { offset, item in Self.rank(of: needle, in: item.title).map { (rank: $0, offset: offset, item: item) } }
			.sorted { ($0.rank, $0.offset) < ($1.rank, $1.offset) }
			.map(\.item)
	}

	private static func rank(of needle: String, in title: String) -> Int? {
		let haystack = fold(title)
		if haystack.hasPrefix(needle) { return 0 }
		let words = haystack.split { !$0.isLetter && !$0.isNumber }
		if words.contains(where: { $0.hasPrefix(needle) }) { return 1 }
		if haystack.contains(needle) { return 2 }
		return nil
	}

	private static func fold(_ text: String) -> String {
		text.folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: nil)
	}
}
