import Foundation

/// Finds the category or group a question names (docs/finance/assistant.md
/// AI-3.6). Only one exact match answers; anything else names the closest
/// candidates and gives no figure, so the app never guesses.
enum AssistantNames {
	struct Candidate: Hashable, Sendable {
		let id: String
		let name: String
		let isGroup: Bool
		/// A category's group, which tells two categories of one name apart.
		var group: String?

		/// "the category Food in Everyday", "the category group Food".
		var described: String {
			if isGroup { return "the category group \(name)" }
			return group.map { "the category \(name) in \($0)" } ?? "the category \(name)"
		}
	}

	enum Match: Hashable, Sendable {
		case found(Candidate)
		case notFound(closest: [String])
		case several([Candidate])
	}

	static let maximumSuggestions = 3
	static let maximumEdits = 2

	static func match(_ wanted: String, in all: [Candidate]) -> Match {
		let candidates = all.filter { !key($0.name).isEmpty }
		var words = words(wanted)
		if words.count > 1, let first = words.first, articles.contains(first) { words.removeFirst() }
		var scope = candidates
		if words.count > 1, let last = words.last {
			if ["category", "envelope"].contains(last) {
				scope = scope.filter { !$0.isGroup }
				words.removeLast()
			} else if last == "group" {
				scope = scope.filter(\.isGroup)
				words.removeLast()
			}
		}
		if let split = words.lastIndex(of: "in"), split > 0, split < words.count - 1 {
			let name = words[..<split].joined(separator: " ")
			let group = words[(split + 1)...].joined(separator: " ")
			let exact = scope.filter { !$0.isGroup && key($0.name) == name && $0.group.map(key) == group }
			if exact.count == 1 { return .found(exact[0]) }
		}
		let asked = words.joined(separator: " ")
		let exact = scope.filter { key($0.name) == asked }
		if exact.count == 1 { return .found(exact[0]) }
		if exact.count > 1 { return .several(exact.filter { !$0.isGroup } + exact.filter(\.isGroup)) }
		return .notFound(closest: closest(to: asked, in: candidates))
	}

	/// What a tool says when `match` found no single category or group.
	static func text(_ match: Match, wanted: String) -> String? {
		let asked = wanted.trimmingCharacters(in: .whitespacesAndNewlines)
		switch match {
		case .found:
			return nil
		case .several(let candidates):
			return "More than one category or group is called \"\(asked)\": \(candidates.map(\.described).joined(separator: ", ")). Ask which one."
		case .notFound(let closest) where closest.isEmpty:
			return "No category or group is called \"\(asked)\"."
		case .notFound(let closest) where closest.count == 1:
			return "No category or group is called \"\(asked)\". The closest name is \(closest[0])."
		case .notFound(let closest):
			return "No category or group is called \"\(asked)\". The closest names are \(closest.joined(separator: ", "))."
		}
	}

	/// Tiers: the name starts with what was asked, holds it, or is held by it;
	/// then any other name at most `maximumEdits` away. Within a tier, fewest
	/// edits first, then by name.
	static func closest(to asked: String, in candidates: [Candidate]) -> [String] {
		let ranked = candidates.compactMap { candidate -> (tier: Int, edits: Int, name: String)? in
			let name = key(candidate.name)
			guard !name.isEmpty else { return nil }
			let edits = distance(asked, name)
			let tier = if asked.isEmpty { 3 } else if name.hasPrefix(asked) { 0 } else if name.contains(asked) { 1 } else if asked.contains(name) { 2 } else { 3 }
			guard tier < 3 || edits <= maximumEdits else { return nil }
			return (tier, edits, candidate.name)
		}
		.sorted { ($0.tier, $0.edits, $0.name.lowercased()) < ($1.tier, $1.edits, $1.name.lowercased()) }
		var names: [String] = []
		for entry in ranked where !names.contains(entry.name) {
			names.append(entry.name)
			if names.count == maximumSuggestions { break }
		}
		return names
	}

	private static let articles: Set<String> = ["the", "my", "our"]

	/// A name's words with case and accents folded, "&" read as "and", and
	/// each word made singular.
	private static func words(_ text: String) -> [String] {
		text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
			.replacingOccurrences(of: "&", with: " and ")
			.split(whereSeparator: { !$0.isLetter && !$0.isNumber })
			.map { singular(String($0)) }
	}

	/// The words compared, with a leading "the", "my" or "our" dropped.
	static func key(_ name: String) -> String {
		var words = words(name)
		if words.count > 1, let first = words.first, articles.contains(first) { words.removeFirst() }
		return words.joined(separator: " ")
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
