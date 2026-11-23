import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The Budget screen (R-BUD-1): To Budget, then each group's envelopes with
/// their drain bars, budgeted, spent and available.
struct BudgetScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var screen: BudgetScreenModel
	@State private var isMovingBudget = false
	@State private var isAddingGroup = false
	@State private var showsBar = true
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, screen: BudgetScreenModel? = nil) {
		self.workspace = workspace
		_screen = State(initialValue: screen ?? BudgetScreenModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: 0) {
				if let sheet = screen.sheet {
					if !screen.isTracking || (!workspace.isDense && !screen.goals.isEmpty) {
						HStack(alignment: .center, spacing: Spacing.s4) {
							if !screen.isTracking {
								ToBudgetHeader(sheet: sheet, dense: workspace.isDense, screen: screen)
							}
							Spacer(minLength: Spacing.s4)
							if !workspace.isDense {
								ForEach(screen.goals.prefix(2)) { goal in
									GoalCard(goal: goal, format: screen.format)
								}
							}
						}
						.padding(.bottom, workspace.isDense ? Spacing.s3 : Spacing.s6)
					}
					if workspace.isDense {
						GroupedRows {
							EnvelopeColumns(dense: true)
								.frame(height: 26)
							groups(sheet)
						}
					} else {
						EnvelopeColumns(dense: false)
						groups(sheet)
					}
					Button {
						isAddingGroup = true
					} label: {
						Label("New group", systemImage: "plus")
					}
					.disabled(screen.isReadOnly)
					.padding(.top, Spacing.s3)
					.popover(isPresented: $isAddingGroup, arrowEdge: .bottom) {
						NamePrompt(title: "New group", action: "Add group") { name in
							await screen.createGroup(named: name)
						}
					}
					.accessibilityIdentifier("budget.newGroup")
					if let note = sheet.mixedScaleNote {
						MixedScaleNote(text: note)
							.padding(.top, Spacing.s4)
					}
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: .infinity, alignment: .leading)
			.contentTransition(.opacity)
			.motion(.fast, value: screen.sheet?.month)
			.onGeometryChange(for: Bool.self) { $0.size.width - 2 * Spacing.s5 >= EnvelopeLayout.widthWithBar } action: { showsBar = $0 }
			.environment(\.showsDrainBar, showsBar)
		}
		.overlay { if screen.sheet == nil { ScreenState(kind: .loading) } }
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.focusedSceneValue(\.budgetMonths, screen.editing == nil ? BudgetMonthActions(
			canGoBack: screen.canGoBack,
			canGoForward: screen.canGoForward,
			previous: { Task { await screen.previousMonth() } },
			next: { Task { await screen.nextMonth() } }
		) : nil)
		.onKeyPress(keys: [.upArrow, .downArrow]) { press in
			guard screen.editing == nil, let action = BudgetKeys.action(for: press.key, modifiers: press.modifiers) else {
				return .ignored
			}
			switch action {
			case .moveSelection(let step): screen.moveSelection(by: step)
			case .moveToGroup(let step): screen.moveSelectionToGroup(by: step)
			}
			return .handled
		}
		.onKeyPress(.return) {
			guard screen.editing == nil, let selection = screen.selection else { return .ignored }
			screen.beginEditing(selection)
			return .handled
		}
		.onKeyPress(characters: CharacterSet(charactersIn: "0123456789.,+-*/^(")) { press in
			guard screen.editing == nil, let selection = screen.selection else { return .ignored }
			screen.beginEditing(selection)
			screen.editing?.type(press.characters)
			return .handled
		}
		.navigationTitle(screen.sheet.map { Self.title($0.month) } ?? "Budget")
		.toolbar {
			ToolbarItem(placement: .navigation) {
				MonthStepper(screen: screen, isEditing: screen.editing != nil)
			}
			ToolbarItem(placement: .primaryAction) {
				if !screen.isTracking {
					Button {
						isMovingBudget = true
					} label: {
						Label("Move budget", systemImage: "arrow.left.arrow.right")
					}
					.labelStyle(.titleAndIcon)
					.disabled(screen.sheet == nil || screen.isReadOnly)
					.accessibilityIdentifier("budget.moveBudget")
				}
			}
		}
		.sheet(isPresented: $isMovingBudget) {
			MoveBudgetSheet(screen: screen, from: screen.selection)
		}
		.task(id: workspace.changeCount) { await screen.load() }
		// Closing the amount field would leave keyboard focus with the sidebar.
		// The field takes first responder through AppKit, so SwiftUI may still
		// think the screen has focus: clear it, then set it on the next turn.
		.onChange(of: screen.editingID) { _, id in
			guard id == nil else { return }
			hasKeyFocus = false
			Task { @MainActor in hasKeyFocus = true }
		}
		.containerIdentifier("screen.budget")
	}

	private func groups(_ sheet: BudgetMonth) -> some View {
		ForEach(screen.groups, id: \.id) { group in
			EnvelopeGroupView(group: group, screen: screen, dense: workspace.isDense)
		}
	}

	static func title(_ month: Month) -> String {
		guard let date = month.date() else { return "Budget" }
		return date.formatted(.dateTime.month(.wide).year())
	}
}

private struct MonthStepper: View {
	let screen: BudgetScreenModel
	let isEditing: Bool

	var body: some View {
		ControlGroup {
			Button {
				Task { await screen.previousMonth() }
			} label: {
				Label("Previous month", systemImage: "chevron.left")
			}
			.disabled(!screen.canGoBack || isEditing)
			.accessibilityIdentifier("budget.previousMonth")
			Button {
				Task { await screen.nextMonth() }
			} label: {
				Label("Next month", systemImage: "chevron.right")
			}
			.disabled(!screen.canGoForward || isEditing)
			.accessibilityIdentifier("budget.nextMonth")
		}
		.help("Previous or next month (⌘← ⌘→)")
	}
}

/// To Budget, the hero figure. Its menu holds part of it for next month.
private struct ToBudgetHeader: View {
	let sheet: BudgetMonth
	let dense: Bool
	let screen: BudgetScreenModel
	@State private var isHolding = false

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
			HStack(spacing: Spacing.s2) {
				Image(systemName: tone.symbol ?? "tray.and.arrow.down")
					.font(.system(size: dense ? 15 : 26, weight: .semibold))
				Text(screen.format.currency(sheet.toBudget, on: .budget))
					.font(.figures(dense ? .title : .display, weight: .bold))
					.lineLimit(1)
					.fixedSize()
			}
			.foregroundStyle(Color(tone.text))
			Text("To Budget")
				.font(.text(dense ? .body : .heading, weight: .semibold))
				.fixedSize()
			if sheet.forNextMonth > 0 {
				Text("\(screen.format.amount(sheet.forNextMonth, on: .budget)) held for next month")
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
			}
		}
		.contentShape(.rect)
		.contextMenu {
			Button("Hold for Next Month…") { isHolding = true }
				.disabled(sheet.toBudget <= 0 || screen.isReadOnly)
			Button("Reset Hold") { Task { await screen.resetHold() } }
				.disabled(sheet.forNextMonth == 0 || screen.isReadOnly)
		}
		.popover(isPresented: $isHolding, arrowEdge: .bottom) {
			HoldPopover(screen: screen, toBudget: sheet.toBudget)
		}
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("budget.toBudget")
	}

	private var tone: VeritasDesign.MoneyTone {
		sheet.toBudgetTone.design
	}
}

struct EnvelopeColumns: View {
	let dense: Bool

	var body: some View {
		EnvelopeGrid(dense: dense) {
			Text("Envelope")
		} bar: {
			Color.clear
		} budgeted: {
			Text("Budgeted")
		} spent: {
			Text("Spent")
		} available: {
			Text("Available")
		}
		.font(.text(.caption))
		.foregroundStyle(.textTertiary)
		.lineLimit(1)
		.padding(.bottom, dense ? 0 : Spacing.s1)
	}
}

/// The column layout shared by the header, group heads and rows.
struct EnvelopeGrid<Name: View, Bar: View, Budgeted: View, Spent: View, Available: View>: View {
	let dense: Bool
	@ViewBuilder let name: Name
	@ViewBuilder let bar: Bar
	@ViewBuilder let budgeted: Budgeted
	@ViewBuilder let spent: Spent
	@ViewBuilder let available: Available

	@Environment(\.showsDrainBar) private var showsBar

	var body: some View {
		HStack(spacing: Spacing.s4) {
			name
				.frame(minWidth: EnvelopeLayout.nameMinimum, maxWidth: .infinity, alignment: .leading)
				.layoutPriority(1)
			if !dense, showsBar {
				bar.frame(minWidth: 100, maxWidth: 160)
			}
			budgeted.frame(width: 110, alignment: .trailing)
			spent.frame(width: 110, alignment: .trailing)
			available.frame(width: 150, alignment: .trailing)
		}
		.padding(.horizontal, Spacing.s4)
	}
}

enum EnvelopeLayout {
	static let nameMinimum: CGFloat = 160
	/// The narrowest the table can be and still draw the bar beside a readable name.
	static let widthWithBar: CGFloat = nameMinimum + 100 + 110 + 110 + 150 + 4 * Spacing.s4 + 2 * Spacing.s4
}

extension EnvironmentValues {
	/// False when the Budget table is too narrow for the drain bar and a readable name.
	@Entry var showsDrainBar = true
}

/// CLS-12.10's fixed line under a 0-place budget's envelope numbers.
struct MixedScaleNote: View {
	let text: String

	var body: some View {
		Text(text)
			.font(.text(.caption))
			.foregroundStyle(.textSecondary)
			.fixedSize(horizontal: false, vertical: true)
			.accessibilityIdentifier("mixedScaleNote")
	}
}
