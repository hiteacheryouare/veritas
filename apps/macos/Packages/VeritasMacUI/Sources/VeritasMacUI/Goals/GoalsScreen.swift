import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Goals as cards with rings, and the archive of closed goals (R-GOAL-3, R-GOAL-5).
/// Arrow keys move between goals, Return opens the selected one, and the Goal
/// menu holds every goal action (design/interaction.md).
struct GoalsScreen: View {
	static let columns = 4

	@Bindable var workspace: WorkspaceModel
	@State private var goals: GoalsScreenModel
	@State private var isCreating = false
	@State private var closing: GoalSummary?
	@State private var retargeting: GoalSummary?
	@State private var choosingIcon: GoalSummary?
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, goals: GoalsScreenModel? = nil) {
		self.workspace = workspace
		_goals = State(initialValue: goals ?? GoalsScreenModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s6) {
				LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: Spacing.s4, alignment: .top), count: Self.columns), spacing: Spacing.s4) {
					ForEach(goals.open) { goal in
						LargeGoalCard(goal: goal, format: goals.format, isSelected: goals.selection == goal.id) { closing = goal }
							.onTapGesture { select(goal) }
							.contextMenu { menu(for: goal) }
					}
				}
				if !goals.archive.isEmpty {
					VStack(alignment: .leading, spacing: Spacing.s2) {
						SectionTitle("Archive")
						GroupedRows {
							ForEach(goals.archive) { goal in
								ArchivedGoalRow(goal: goal, format: goals.format, isSelected: goals.selection == goal.id)
									.onTapGesture { select(goal) }
									.contextMenu { menu(for: goal) }
								if goal.id != goals.archive.last?.id {
									Divider().padding(.leading, Spacing.s4)
								}
							}
						}
					}
				}
				if let note = workspace.mixedScaleNote {
					MixedScaleNote(text: note)
				}
			}
			.padding(Spacing.s5)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow]) { press in
			guard press.modifiers.isDisjoint(with: BudgetKeys.chordKeys), let move = Self.move(for: press.key) else { return .ignored }
			goals.moveSelection(move, columns: Self.columns)
			return .handled
		}
		.onKeyPress(.return) {
			guard let goal = goals.selectedGoal, open(goal) else { return .ignored }
			return .handled
		}
		.focusedSceneValue(\.goalActions, actions)
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Button {
					isCreating = true
				} label: {
					Label("New goal", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(goals.isReadOnly)
				.accessibilityIdentifier("goals.new")
			}
		}
		.sheet(isPresented: $isCreating) { NewGoalSheet(goals: goals) }
		.sheet(item: $closing) { goal in CloseGoalSheet(goals: goals, goal: goal) }
		.sheet(item: $retargeting) { goal in RetargetGoalSheet(goals: goals, goal: goal) }
		.sheet(item: $choosingIcon) { goal in GoalIconSheet(goals: goals, goal: goal) }
		.task(id: workspace.changeCount) { await goals.load() }
		// Reached from another screen with ⌘4, the sidebar still holds the
		// keys; defaultFocus only applies when the window first opens.
		.onAppear { Task { @MainActor in hasKeyFocus = true } }
		.containerIdentifier("screen.goals")
	}

	private func select(_ goal: GoalSummary) {
		goals.selection = goal.id
		hasKeyFocus = true
	}

	/// Return on a goal opens its target and date; a goal that can't be
	/// re-targeted opens the close sheet, and a closed one does nothing.
	@discardableResult
	private func open(_ goal: GoalSummary) -> Bool {
		if goal.canRetarget {
			retargeting = goal
		} else if goal.isOpen {
			closing = goal
		} else {
			return false
		}
		return true
	}

	private var actions: GoalActions {
		let goal = goals.selectedGoal
		let writable = !goals.isReadOnly
		return GoalActions(
			close: goal.flatMap { goal in writable && goal.isOpen ? { closing = goal } : nil },
			changeTarget: goal.flatMap { goal in writable && goal.canRetarget ? { retargeting = goal } : nil },
			changeIcon: goal.flatMap { goal in writable ? { choosingIcon = goal } : nil },
			reopen: goal.flatMap { goal in writable && !goal.isOpen ? { Task { await goals.reopen(goal) } } : nil }
		)
	}

	static func move(for key: KeyEquivalent) -> GoalsScreenModel.Move? {
		switch key {
		case .leftArrow: .left
		case .rightArrow: .right
		case .upArrow: .up
		case .downArrow: .down
		default: nil
		}
	}

	@ViewBuilder private func menu(for goal: GoalSummary) -> some View {
		Group { goalMenu(for: goal) }
			.disabled(goals.isReadOnly)
	}

	@ViewBuilder private func goalMenu(for goal: GoalSummary) -> some View {
		if goal.isOpen {
			Button("Close Goal…") { closing = goal }
			if goal.canRetarget {
				Button("Change Target…") { retargeting = goal }
			}
		} else {
			Button("Reopen Goal") { Task { await goals.reopen(goal) } }
		}
		Button("Change Icon…") { choosingIcon = goal }
	}
}

/// The selected goal's actions, published for the window so the Goal menu
/// reaches them from the keyboard. A `nil` action is disabled.
struct GoalActions {
	let close: (() -> Void)?
	let changeTarget: (() -> Void)?
	let changeIcon: (() -> Void)?
	let reopen: (() -> Void)?
}

extension FocusedValues {
	@Entry var goalActions: GoalActions?
}

private struct LargeGoalCard: View {
	let goal: GoalSummary
	let format: MoneyFormat
	let isSelected: Bool
	let close: () -> Void

	private var words: GoalWords { GoalWords(goal, format: format) }

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			GoalRing(goal: goal, tone: words.tone, size: Size.ringLarge, lineWidth: 9, showsPercent: true)
				.padding(.bottom, Spacing.s3)
			HStack(spacing: Spacing.s2) {
				if let icon = GoalIcon(goal.icon) {
					GoalIconView(icon: icon)
						.accessibilityIdentifier("goal.\(goal.id).icon.\(icon.stored)")
				}
				Text(goal.name)
					.font(.text(.heading, weight: .semibold))
			}
			GoalProgress(goal: goal, format: format)
				.padding(.top, 2)
				.padding(.bottom, Spacing.s2)
			Label {
				Text(words.note)
			} icon: {
				Image(systemName: words.tone.symbol ?? "checkmark.circle")
			}
			.font(.text(.body, weight: .medium))
			.foregroundStyle(Color(words.tone.text))
			if let detail = words.detail {
				Text(detail)
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
					.padding(.top, Spacing.s1)
			}
			if goal.state == .reached || goal.state == .reachedSpent {
				Button(action: close) {
					Label("Close goal", systemImage: "archivebox")
				}
				.controlSize(.small)
				.padding(.top, Spacing.s3)
				.accessibilityIdentifier("goal.\(goal.id).close")
			}
		}
		.padding(Spacing.s4)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay(RoundedRectangle(cornerRadius: Radius.section).strokeBorder(isSelected ? Color.focusRing : Color.edgeGrouped, lineWidth: isSelected ? 2 : 1))
		.motion(.fast, value: isSelected)
		.contentShape(.rect)
		.accessibilityElement(children: .contain)
		.accessibilityLabel("\(goal.name), \(format.wholeCurrency(goal.progressAmount, on: .budget)) of \(format.wholeCurrency(goal.target, on: .budget)), \(words.note)")
		.accessibilityAddTraits(isSelected ? .isSelected : [])
		.accessibilityIdentifier("goal.\(goal.id)")
	}
}

private struct ArchivedGoalRow: View {
	let goal: GoalSummary
	let format: MoneyFormat
	let isSelected: Bool

	var body: some View {
		let words = GoalWords(goal, format: format)
		HStack {
			HStack(spacing: Spacing.s2) {
				if let icon = GoalIcon(goal.icon) {
					GoalIconView(icon: icon, size: 13)
				}
				Text(goal.name)
					.font(.text(.body, weight: .medium))
					.foregroundStyle(.textPrimary)
			}
			.frame(maxWidth: .infinity, alignment: .leading)
			Text(words.note)
				.frame(width: 200, alignment: .leading)
			Text(words.detail ?? "")
				.font(.figures(.body))
				.frame(width: 160, alignment: .trailing)
		}
		.foregroundStyle(.textSecondary)
		.padding(.horizontal, Spacing.s4)
		.frame(height: Size.row)
		.background(isSelected ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear))
		.motion(.fast, value: isSelected)
		.contentShape(.rect)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel([goal.name, words.note, words.detail].compactMap(\.self).joined(separator: ", "))
		.accessibilityAddTraits(isSelected ? .isSelected : [])
		.accessibilityIdentifier("goal.\(goal.id)")
	}
}
