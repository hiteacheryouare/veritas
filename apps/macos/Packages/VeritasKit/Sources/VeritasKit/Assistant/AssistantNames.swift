import Foundation

/// Finds the category or group a question names. Only a match that ignores
/// case, accents, "the", a trailing "category" or "group", and a plural
/// counts. Anything looser offers the closest names instead of guessing (AI-3.6).
enum AssistantNames {
	struct Candidate: Hashable, Sendable {
		let id: String
		let name: String
		let isGroup: Bool
		/// A category's group, which tells two categories of one name apart.
		var group: String?
	}

	enum Match: Hashable, Sendable {
		case found(Candidate)
		case notFound(closest: [String])
		case several([String])
	}

	static func match(_ wanted: String, in candidates: [Candidate]) -> Match {
		let key = Self.key(wanted, dropping: true)
		let exact = candidates.filter { Self.key($0.name, dropping: false) == key }
		let categories = exact.filter { !$0.isGroup }
		if categories.count == 1 { return .found(categories[0]) }
		if categories.count > 1 { return .several(categories.map { category in category.group.map { "\(category.name) in \($0)" } ?? category.name }) }
		if categories.isEmpty, exact.count == 1 { return .found(exact[0]) }
		return .notFound(closest: closest(to: key, in: candidates))
	}

	/// What a tool says when `match` found no single category or group.
	static func text(_ match: Match, wanted: String) -> String? {
		let asked = wanted.trimmingCharacters(in: .whitespacesAndNewlines)
		switch match {
		case .found: return nil
		case .several(let names): return "More than one category is called \"\(asked)\": \(names.joined(separator: ", ")). Ask which one."
		case .notFound(let closest) where closest.isEmpty: return "No category or group is called \"\(asked)\"."
		case .notFound(let closest): return "No category or group is called \"\(asked)\". The closest names are \(closest.joined(separator: ", "))."
		}
	}

	/// Names that start with or hold the words asked for come first, then the fewest edits away.
	private static func closest(to key: String, in candidates: [Candidate], count: Int = 3) -> [String] {
		let ranked = candidates.map { candidate in
			let name = Self.key(candidate.name, dropping: false)
			let holds = !key.isEmpty && (name.hasPrefix(key) || name.contains(key) || key.contains(name))
			return (candidate.name, holds ? 0 : 1, distance(key, name))
		}
		.sorted { ($0.1, $0.2, $0.0) < ($1.1, $1.2, $1.0) }
		var names: [String] = []
		for (name, _, _) in ranked where !names.contains(name) {
			names.append(name)
			if names.count == count { break }
		}
		return names
	}

	private static func key(_ text: String, dropping: Bool) -> String {
		var words = text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
			.replacingOccurrences(of: "&", with: " and ")
			.split(whereSeparator: { !$0.isLetter && !$0.isNumber })
			.map(String.init)
		if dropping {
			if let first = words.first, ["the", "my", "our"].contains(first) { words.removeFirst() }
			if let last = words.last, ["category", "categories", "group", "envelope"].contains(last), words.count > 1 { words.removeLast() }
		}
		return words.map(singular).joined(separator: " ")
	}

	private static func singular(_ word: String) -> String {
		if word.count > 4, word.hasSuffix("ies") { return String(word.dropLast(3)) + "y" }
		if word.count > 3, word.hasSuffix("s"), !word.hasSuffix("ss") { return String(word.dropLast()) }
		return word
	}

	private static func distance(_ a: String, _ b: String) -> Int {
		let (a, b) = (Array(a), Array(b))
		guard !a.isEmpty else { return b.count }
		var row = Array(0...b.count)
		for i in 1...a.count {
			var previous = row[0]
			row[0] = i
			for j in stride(from: 1, through: b.count, by: 1) {
				let saved = row[j]
				row[j] = min(row[j] + 1, row[j - 1] + 1, previous + (a[i - 1] == b[j - 1] ? 0 : 1))
				previous = saved
			}
		}
		return row[b.count]
	}
}
