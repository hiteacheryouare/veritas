import Foundation
import VeritasCore

/// Finds the category or group a question names (docs/finance/assistant.md
/// AI-3.6). The core's `matchCategoryName` does the matching; this words the
/// answer when there's no single match, so the app never guesses.
enum AssistantNames {
	typealias Candidate = NameCandidate

	static func match(_ wanted: String, in candidates: [NameCandidate]) -> NameMatch {
		(try? matchCategoryName(wanted: wanted, candidates: candidates)) ?? .notFound(closest: [])
	}

	/// AI-3.6's name key, from the core.
	static func key(_ name: String) -> String {
		(try? categoryNameKey(name: name)) ?? name
	}

	/// What a tool says when `match` found no single category or group.
	static func text(_ match: NameMatch, wanted: String) -> String? {
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
}

extension NameCandidate {
	/// A group, or a category whose group doesn't matter here.
	init(id: String, name: String, isGroup: Bool) {
		self.init(id: id, name: name, isGroup: isGroup, group: nil)
	}

	/// "the category Food in Everyday", "the category group Food".
	var described: String {
		if isGroup { return "the category group \(name)" }
		return group.map { "the category \(name) in \($0)" } ?? "the category \(name)"
	}
}
