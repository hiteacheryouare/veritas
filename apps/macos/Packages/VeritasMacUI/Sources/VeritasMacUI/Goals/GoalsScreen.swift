import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Goals as cards with rings, and the archive of closed goals (R-GOAL-3, R-GOAL-5).
struct GoalsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var goals: GoalsScreenModel
	@State private var isCreating = false
	@State private var closing: GoalSummary?
	@State private var retargeting: GoalSummary?

	init(workspace: WorkspaceModel, goals: GoalsScreenModel? = nil) {
		self.workspace = workspace
		_goals = State(initialValue: goals ?? GoalsScreenModel(
			workspace: workspace,
			reader: CoreAmountReader(budgetDecimalPlaces: workspace.budgetDecimalPlaces)
		))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s6) {
				LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: Spacing.s4, alignment: .top), count: 4), spacing: Spacing.s4) {
					ForEach(goals.open) { goal in
						LargeGoalCard(goal: goal, format: goals.format) { closing = goal }
							.contextMenu { menu(for: goal) }
					}
				}
				if !goals.archive.isEmpty {
					VStack(alignment: .leading, spacing: Spacing.s2) {
						SectionTitle("Archive")
						GroupedRows {
							ForEach(goals.archive) { goal in
								ArchivedGoalRow(goal: goal, format: goals.format)
									.contextMenu { menu(for: goal) }
								if goal.id != goals.archive.last?.id {
									Divider().padding(.leading, Spacing.s4)
								}
							}
						}
					}
				}
			}
			.padding(Spacing.s5)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
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
		.task(id: workspace.changeCount) { await goals.load() }
		.containerIdentifier("screen.goals")
	}

	@ViewBuilder private func menu(for goal: GoalSummary) -> some View {
		if goal.isOpen {
			Button("Close Goal…") { closing = goal }
			if goal.canRetarget {
				Button("Change Target…") { retargeting = goal }
			}
		} else {
			Button("Reopen Goal") { Task { await goals.reopen(goal) } }
		}
	}
}

private struct LargeGoalCard: View {
	let goal: GoalSummary
	let format: MoneyFormat
	let close: () -> Void

	private var words: GoalWords { GoalWords(goal, format: format) }

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			GoalRing(goal: goal, tone: words.tone, size: Size.ringLarge, lineWidth: 9, showsPercent: true)
				.padding(.bottom, Spacing.s3)
			Text(goal.name)
				.font(.text(.heading, weight: .semibold))
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
		.overlay(RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped))
		.accessibilityElement(children: .contain)
		.accessibilityLabel("\(goal.name), \(format.wholeCurrency(goal.shownProgress)) of \(format.wholeCurrency(goal.target)), \(words.note)")
		.accessibilityIdentifier("goal.\(goal.id)")
	}
}

private struct ArchivedGoalRow: View {
	let goal: GoalSummary
	let format: MoneyFormat

	var body: some View {
		let words = GoalWords(goal, format: format)
		HStack {
			Text(goal.name)
				.font(.text(.body, weight: .medium))
				.foregroundStyle(.textPrimary)
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
		.contentShape(.rect)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel([goal.name, words.note, words.detail].compactMap(\.self).joined(separator: ", "))
		.accessibilityIdentifier("goal.\(goal.id)")
	}
}
