import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The brand accent, until #115's accent token lands and replaces it.
private let brandAccent = Color(.sRGB, red: 0x23 / 255, green: 0x16 / 255, blue: 0x9C / 255)

/// The Budget screen's To Budget figure as a centred hero card
/// (docs/finance/to-budget.md TB-1.1). Clicking it opens the quick actions
/// (TB-1.2); after one runs, its result line offers Undo (TB-2.4).
struct ToBudgetCard: View {
	let sheet: BudgetMonth
	let dense: Bool
	let screen: BudgetScreenModel
	let moveToCategory: () -> Void
	@State private var isShowingActions = false
	@State private var isHovering = false

	private var state: ToBudgetState {
		screen.toBudgetActions?.state ?? (sheet.toBudget < 0 ? .overbudgeted : sheet.toBudget == 0 ? .zero : .positive)
	}

	private var tone: VeritasDesign.MoneyTone { sheet.toBudgetTone.design }

	var body: some View {
		VStack(spacing: Spacing.s3) {
			Button {
				isShowingActions = true
			} label: {
				card
			}
			.buttonStyle(.plain)
			.onHover { isHovering = $0 }
			.popover(isPresented: $isShowingActions, arrowEdge: .bottom) {
				ToBudgetActionsPopover(screen: screen, moveToCategory: {
					isShowingActions = false
					moveToCategory()
				})
			}
			.accessibilityLabel("\(ToBudgetWords.label(state)) \(screen.format.currency(sheet.toBudget, on: .budget))")
			.accessibilityHint("Shows ways to assign it")
			.accessibilityIdentifier("budget.toBudget")
			.tourAnchor(.toBudget)
			if let result = screen.quickResult {
				ToBudgetResultLine(text: result, screen: screen)
					.transition(.opacity)
			}
		}
		.frame(maxWidth: .infinity)
		// TODO(#114): use Motion.settle and .motionTransition(.reveal) once ws/motion lands.
		.animation(.spring(duration: 0.32, bounce: 0), value: screen.quickResult)
	}

	private var card: some View {
		VStack(spacing: dense ? Spacing.s1 : Spacing.s2) {
			Text(ToBudgetWords.label(state))
				.font(.text(dense ? .body : .heading, weight: .semibold))
				.foregroundStyle(.textSecondary)
			HStack(spacing: Spacing.s2) {
				Image(systemName: tone.symbol ?? "tray.and.arrow.down")
					.font(.system(size: dense ? 18 : 30, weight: .semibold))
				Text(screen.format.currency(sheet.toBudget, on: .budget))
					.font(.figures(dense ? .title : .showcase, weight: .bold))
					.lineLimit(1)
					.minimumScaleFactor(0.5)
			}
			.foregroundStyle(Color(tone.text))
			if sheet.forNextMonth > 0 {
				Text("\(screen.format.amount(sheet.forNextMonth, on: .budget)) held for next month")
					.font(.text(.body))
					.foregroundStyle(.textSecondary)
			}
			Image(systemName: "chevron.down")
				.font(.system(size: 11, weight: .semibold))
				.foregroundStyle(.textTertiary)
				.accessibilityHidden(true)
		}
		.padding(.vertical, dense ? Spacing.s3 : Spacing.s5)
		.padding(.horizontal, Spacing.s6)
		.frame(minWidth: dense ? 260 : 360)
		.background(brandAccent.opacity(isHovering ? 0.1 : 0.06), in: .rect(cornerRadius: Radius.section))
		.overlay {
			RoundedRectangle(cornerRadius: Radius.section)
				.strokeBorder(brandAccent.opacity(isHovering ? 0.45 : 0.25), lineWidth: 1)
		}
		.contentShape(.rect(cornerRadius: Radius.section))
	}
}

/// "Assigned $412.00 to 3 envelopes" with Undo (TB-2.4).
private struct ToBudgetResultLine: View {
	let text: String
	let screen: BudgetScreenModel

	var body: some View {
		HStack(spacing: Spacing.s3) {
			Text(text)
				.font(.text(.body))
				.foregroundStyle(.textSecondary)
				.accessibilityIdentifier("budget.quickResult")
			Button("Undo") { Task { await screen.undoQuickAction() } }
				.buttonStyle(.link)
				.accessibilityIdentifier("budget.quickResult.undo")
			Button {
				screen.dismissQuickResult()
			} label: {
				Image(systemName: "xmark")
					.font(.system(size: 10, weight: .semibold))
			}
			.buttonStyle(.borderless)
			.help("Dismiss")
			.accessibilityLabel("Dismiss")
		}
	}
}

/// The quick actions (TB-1.2): each row says what it would do, or why it
/// can't (TB-1.3). Opening it writes nothing.
struct ToBudgetActionsPopover: View {
	let screen: BudgetScreenModel
	let moveToCategory: () -> Void
	@State private var isHolding = false
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		Group {
			if isHolding, let actions = screen.toBudgetActions {
				HoldPopover(screen: screen, toBudget: actions.hold.defaultAmount)
			} else {
				rows
			}
		}
		// TODO(#114): use .motionTransition(.fade) once ws/motion lands.
		.animation(.spring(duration: 0.32, bounce: 0), value: isHolding)
	}

	private var rows: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			if let actions = screen.toBudgetActions {
				let format = screen.format
				let readOnly = screen.isReadOnly
				ToBudgetActionRow(
					title: "Assign to underfunded",
					subtitle: ToBudgetWords.subtitle(actions.assign, counting: .envelopes, format: format),
					notes: ToBudgetWords.notes(actions.assign),
					fills: actions.assign.fills,
					format: format,
					isEnabled: actions.assign.block == nil && !readOnly,
					id: "toBudget.assign"
				) {
					dismiss()
					Task { await screen.assignToUnderfunded() }
				}
				ToBudgetActionRow(
					title: "Fund goals' monthly",
					subtitle: ToBudgetWords.subtitle(actions.fundGoals, counting: .goals, format: format),
					notes: ToBudgetWords.notes(actions.fundGoals),
					fills: actions.fundGoals.fills,
					format: format,
					isEnabled: actions.fundGoals.block == nil && !readOnly,
					id: "toBudget.fundGoals"
				) {
					dismiss()
					Task { await screen.fundGoalsMonthly() }
				}
				if actions.hold.offered {
					ToBudgetActionRow(
						title: "Hold for next month\u{2026}",
						subtitle: ToBudgetWords.hold(actions.hold, format: format),
						isEnabled: actions.hold.block == nil && !readOnly,
						id: "toBudget.hold"
					) {
						isHolding = true
					}
				}
				Divider().padding(.vertical, Spacing.s1)
				if actions.toBudget > 0 {
					ToBudgetActionRow(
						title: "Move to a category",
						subtitle: "Choose an envelope and an amount",
						isEnabled: !readOnly,
						id: "toBudget.move",
						action: moveToCategory
					)
				}
				if actions.hold.canReset {
					ToBudgetActionRow(
						title: "Reset next month's buffer",
						subtitle: ToBudgetWords.reset(actions.hold, format: format),
						isEnabled: !readOnly,
						id: "toBudget.reset"
					) {
						dismiss()
						Task { await screen.resetHold() }
					}
				}
			} else if screen.toBudgetActionsFailed {
				HStack(spacing: Spacing.s3) {
					Text("Couldn't load these actions.")
						.font(.text(.body))
						.foregroundStyle(.textSecondary)
					Spacer()
					Button("Try Again") { Task { await screen.reloadToBudgetActions() } }
						.accessibilityIdentifier("toBudget.retry")
				}
				.padding(Spacing.s2)
			} else {
				ScreenState(kind: .loading)
					.frame(height: 120)
			}
		}
		.padding(Spacing.s3)
		.frame(width: 360)
		.containerIdentifier("popover.toBudget")
	}
}

/// One quick action: its title, what it would do, its notes, and the list a
/// tap away (TB-3.5).
private struct ToBudgetActionRow: View {
	let title: String
	let subtitle: String
	var notes: [String] = []
	var fills: [ToBudgetFill] = []
	var format = MoneyFormat()
	let isEnabled: Bool
	let id: String
	let action: () -> Void
	@State private var showsList = false

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
				Button(action: action) {
					VStack(alignment: .leading, spacing: 2) {
						Text(title)
							.font(.text(.body, weight: .semibold))
							.foregroundStyle(isEnabled ? .textPrimary : .textTertiary)
						Text(subtitle)
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
							.fixedSize(horizontal: false, vertical: true)
						ForEach(notes, id: \.self) { note in
							Text(note)
								.font(.text(.caption))
								.foregroundStyle(.textTertiary)
						}
					}
					.frame(maxWidth: .infinity, alignment: .leading)
					.contentShape(.rect)
				}
				.buttonStyle(.plain)
				.disabled(!isEnabled)
				.accessibilityIdentifier(id)
				if !fills.isEmpty {
					Button {
						showsList.toggle()
					} label: {
						Image(systemName: showsList ? "chevron.up" : "chevron.down")
							.font(.system(size: 10, weight: .semibold))
					}
					.buttonStyle(.borderless)
					.help(showsList ? "Hide the list" : "Show the list")
					.accessibilityLabel(showsList ? "Hide the list" : "Show the list")
				}
			}
			if showsList {
				VStack(alignment: .leading, spacing: 2) {
					ForEach(Array(fills.enumerated()), id: \.offset) { _, fill in
						HStack {
							Text(fill.name)
							Spacer()
							Text(ToBudgetWords.fill(fill, format: format))
								.font(.figures(.caption))
						}
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
					}
				}
				.padding(.leading, Spacing.s2)
			}
		}
		.padding(.vertical, Spacing.s1)
		.padding(.horizontal, Spacing.s2)
	}
}
