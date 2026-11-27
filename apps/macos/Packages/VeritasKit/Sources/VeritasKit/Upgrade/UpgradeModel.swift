import Foundation
import Observation
import VeritasCore

/// "Upgrade to Veritas features" (docs/finance/migration.md MIG-1): the
/// offer after an import, then Overview, Review and Confirm. Nothing is
/// written until Apply on the Confirm step.
@MainActor
@Observable
public final class UpgradeModel: Identifiable {
	public enum Step: Hashable, Sendable {
		/// MIG-1.1's offer after an import: "Not now" or "Show me".
		case offer
		case overview
		case review
		case confirm
	}

	/// What a Revert needs from the user before it writes (MIG-4.7, MIG-5.6).
	public enum Question: Identifiable {
		case kind(key: String, change: MigrationKindChange, effects: [MigrationEffect], replaces: CategoryKind?)
		case group(key: String, message: String)

		public var id: String {
			switch self {
			case .kind(let key, _, _, _), .group(let key, _): key
			}
		}
	}

	public var step: Step
	public private(set) var suggestions: [MigrationSuggestion]?
	public private(set) var applied: [AppliedMigration] = []
	public var ticked: Set<String> = []
	public var showsDismissed = false
	public var consentShowGoalsGroup = false
	public var writesNoteInActual: Set<String> = []
	/// (c) The kind the user picked, when it differs from the proposal.
	public var kinds: [String: CategoryKind] = [:]
	/// (d) The name and icon the user typed or picked. An empty name or icon
	/// sets none.
	public var displayNames: [String: String] = [:]
	public var icons: [String: String] = [:]
	public private(set) var targets: [String: AmountFieldModel] = [:]
	public private(set) var isApplying = false
	public private(set) var problem: String?
	public var question: Question?
	/// What the last Revert left alone and why.
	public private(set) var revertNotes: [String] = []

	@ObservationIgnored private let workspace: WorkspaceModel
	@ObservationIgnored private let today: String

	public init(workspace: WorkspaceModel, offering: Bool, today: String = WorkspaceModel.today()) {
		self.workspace = workspace
		self.today = today
		step = offering ? .offer : .overview
	}

	public nonisolated var id: ObjectIdentifier { ObjectIdentifier(self) }

	public static let nothingToUpgrade = "Nothing to upgrade right now."
	public static let nothingChanges = "Nothing changes until you confirm."

	public var visible: [MigrationSuggestion] {
		(suggestions ?? []).filter { showsDismissed || !$0.dismissed }
	}

	public var hasDismissed: Bool {
		(suggestions ?? []).contains(where: \.dismissed)
	}

	/// MIG-1.3's Overview: one line per conversion with its count.
	public var counts: [(conversion: MigrationConversion, count: Int)] {
		let order: [MigrationConversion] = [.report, .goal, .kind, .bill]
		return order.compactMap { conversion in
			let count = visible.filter { $0.conversion == conversion }.count
			return count > 0 ? (conversion, count) : nil
		}
	}

	public var tickedSuggestions: [MigrationSuggestion] {
		visible.filter { ticked.contains($0.key) }
	}

	public func load() async {
		let today = today
		guard let found = await workspace.report({ try await $0.migrationSuggestions(today: today) }) else { return }
		applied = await workspace.report { try await $0.appliedMigrations() } ?? []
		let first = suggestions == nil
		suggestions = found
		for suggestion in found where suggestion.conversion == .goal && targets[suggestion.key] == nil {
			targets[suggestion.key] = AmountFieldModel(
				amount: suggestion.proposal.target ?? 0,
				style: workspace.numberStyle,
				reader: workspace.budgetAmountReader
			)
		}
		ticked = ticked.intersection(found.map(\.key))
		if first {
			selectSuggested()
		}
	}

	/// MIG-1.4: pre-ticked items only, never "shown" ones.
	public func selectSuggested() {
		ticked = []
		for suggestion in visible where suggestion.confidence == .preTicked && !suggestion.dismissed {
			if isTickable(suggestion) {
				ticked.insert(suggestion.key)
			}
		}
	}

	public func isTickable(_ suggestion: MigrationSuggestion) -> Bool {
		guard suggestion.blocked == nil else { return false }
		guard let other = suggestion.blockedBy else { return true }
		return !ticked.contains(other)
	}

	/// The reason an item can't be ticked right now, shown beside it.
	public func blockedReason(_ suggestion: MigrationSuggestion) -> String? {
		if let blocked = suggestion.blocked { return blocked }
		if let other = suggestion.blockedBy, ticked.contains(other) { return "Rename one of them first." }
		return nil
	}

	public func toggle(_ suggestion: MigrationSuggestion) {
		if ticked.contains(suggestion.key) {
			ticked.remove(suggestion.key)
		} else if isTickable(suggestion) {
			ticked.insert(suggestion.key)
		}
	}

	/// (c) The kind Apply will set: the user's pick, else the proposal.
	public func kind(for suggestion: MigrationSuggestion) -> CategoryKind {
		kinds[suggestion.key] ?? suggestion.proposal.kind ?? .debtPrincipal
	}

	/// (d) The name Apply will set: what the user typed, else the proposal.
	public func displayName(for suggestion: MigrationSuggestion) -> String {
		displayNames[suggestion.key] ?? suggestion.proposal.displayName ?? ""
	}

	/// (d) The icon Apply will set; empty is none.
	public func icon(for suggestion: MigrationSuggestion) -> String {
		icons[suggestion.key] ?? suggestion.proposal.icon ?? ""
	}

	public var needsGoalsGroupConsent: Bool {
		tickedSuggestions.contains(where: \.asksToShowGoalsGroup)
	}

	public var canApply: Bool {
		!ticked.isEmpty && !isApplying && (!needsGoalsGroupConsent || consentShowGoalsGroup)
	}

	/// "Don't suggest again", or with "Show dismissed", bringing it back.
	/// Not an undo step (MIG-1.8).
	public func setDismissed(_ suggestion: MigrationSuggestion, _ dismissed: Bool) async {
		let key = suggestion.key
		ticked.remove(key)
		await workspace.change { try await $0.setMigrationDismissed(key, dismissed: dismissed) }
		await load()
	}

	/// MIG-1.5: every ticked item as one command and one undo step.
	@discardableResult
	public func apply() async -> Bool {
		guard canApply else { return false }
		isApplying = true
		defer { isApplying = false }
		problem = nil
		var items: [MigrationItem] = []
		for suggestion in tickedSuggestions {
			var edits = MigrationEdits(target: nil, targetDate: nil, writeNoteInActual: false, kind: nil, displayName: nil, icon: nil)
			switch suggestion.conversion {
			case .goal:
				guard let target = await targets[suggestion.key]?.commit(), target > 0 else {
					problem = "Enter a target for \(suggestion.subjectName)."
					return false
				}
				edits.target = target
				edits.writeNoteInActual = writesNoteInActual.contains(suggestion.key)
			case .kind:
				edits.kind = kinds[suggestion.key]
			case .bill:
				edits.displayName = displayNames[suggestion.key]
				edits.icon = icons[suggestion.key]
			case .report:
				break
			}
			items.append(MigrationItem(key: suggestion.key, edits: edits))
		}
		let selection = MigrationSelection(items: items, consentShowGoalsGroup: consentShowGoalsGroup)
		let today = today
		let done: Void? = await workspace.change({ try await $0.applyMigration(selection, today: today) }, inline: { problem = $0 })
		guard done != nil else { return false }
		ticked = []
		await load()
		return true
	}

	/// The live, non-income groups a goal can go back into (MIG-4.7).
	public func liveGroups() async -> [EnvelopeGroup] {
		let today = today
		guard let month = Month(date: today),
			let sheet = await workspace.report({ try await $0.budgetMonth(month, today: today) }) else { return [] }
		return sheet.groups.filter { !$0.isIncome }
	}

	/// One item's Revert. A kind change or a lost group asks first.
	public func revert(_ key: String, groupID: String? = nil, confirmed: Bool = false) async {
		let today = today
		guard let outcome = await workspace.change({
			try await $0.revertMigration(key: key, groupID: groupID, confirmed: confirmed, today: today)
		}) else { return }
		switch outcome {
		case .reverted(_, let notes):
			revertNotes = notes
			question = nil
			await load()
		case .chooseGroup(let message):
			question = .group(key: key, message: message)
		case .confirmKind(let change, let effects, let replaces):
			question = .kind(key: key, change: change, effects: effects, replaces: replaces)
		}
	}
}
