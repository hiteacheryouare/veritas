import AppKit
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

extension GoalIcon {
	/// The picker's symbols this Mac can draw.
	static var drawableSymbols: [String] {
		symbols.filter(isDrawable)
	}

	static func isDrawable(_ name: String) -> Bool {
		NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil
	}
}

/// A goal's icon: an SF Symbol drawn as an image, an emoji drawn as text. A
/// symbol name this Mac doesn't have draws nothing.
struct GoalIconView: View {
	let icon: GoalIcon
	var size: CGFloat = 17

	var body: some View {
		switch icon {
		case .symbol(let name):
			if GoalIcon.isDrawable(name) {
				Image(systemName: name)
					.font(.system(size: size, weight: .medium))
					.foregroundStyle(.textSecondary)
			}
		case .emoji(let emoji):
			Text(emoji).font(.system(size: size))
		}
	}
}

/// R-GOAL-1: pick a symbol from the grid or type one emoji. The core checks
/// the choice, and its refusal shows under the field.
struct GoalIconSheet: View {
	let goals: GoalsScreenModel
	let goal: GoalSummary
	@State private var symbol: String?
	@State private var emoji: String
	@State private var problem: String?
	@FocusState private var isEmojiFocused: Bool
	@Environment(\.dismiss) private var dismiss

	init(goals: GoalsScreenModel, goal: GoalSummary) {
		self.goals = goals
		self.goal = goal
		switch GoalIcon(goal.icon) {
		case .symbol(let name):
			_symbol = State(initialValue: name)
			_emoji = State(initialValue: "")
		case .emoji(let text):
			_emoji = State(initialValue: text)
		case nil:
			_emoji = State(initialValue: "")
		}
	}

	private var choice: String {
		emoji.isEmpty ? symbol ?? "" : emoji
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Icon for \(goal.name)")
				.font(.text(.title, weight: .semibold))
			LazyVGrid(columns: Array(repeating: GridItem(.fixed(40), spacing: Spacing.s2), count: 8), spacing: Spacing.s2) {
				ForEach(GoalIcon.drawableSymbols, id: \.self) { name in
					SymbolTile(name: name, isSelected: emoji.isEmpty && symbol == name) {
						symbol = name
						emoji = ""
						problem = nil
					}
				}
			}
			HStack(spacing: Spacing.s2) {
				TextField("Emoji", text: $emoji)
					.textFieldStyle(.roundedBorder)
					.frame(width: 90)
					.focused($isEmojiFocused)
					.onChange(of: emoji) { problem = nil }
					.accessibilityIdentifier("goalIcon.emoji")
				Button("Emoji & Symbols") {
					isEmojiFocused = true
					NSApp.orderFrontCharacterPalette(nil)
				}
				.accessibilityIdentifier("goalIcon.palette")
				Spacer()
				Button("No icon") {
					symbol = nil
					emoji = ""
					problem = nil
				}
				.disabled(choice.isEmpty)
				.accessibilityIdentifier("goalIcon.none")
			}
			if let problem {
				ProblemNote(text: problem)
					.accessibilityIdentifier("goalIcon.problem")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Save icon", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(goals.isReadOnly)
					.accessibilityIdentifier("goalIcon.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 440)
		.containerIdentifier("sheet.goalIcon")
	}

	private func save() {
		Task {
			if await goals.setIcon(goal, to: choice, inline: { problem = $0 }) { dismiss() }
		}
	}
}

private struct SymbolTile: View {
	let name: String
	let isSelected: Bool
	let action: () -> Void

	var body: some View {
		if GoalIcon.isDrawable(name) {
			button
		}
	}

	private var button: some View {
		Button(action: action) {
			Image(systemName: name)
				.font(.system(size: 17))
				.frame(width: 40, height: 36)
				.background(isSelected ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
				.overlay {
					RoundedRectangle(cornerRadius: Radius.control)
						.strokeBorder(isSelected ? Color.focusRing : .clear, lineWidth: 1.5)
				}
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.accessibilityAddTraits(isSelected ? .isSelected : [])
		.accessibilityIdentifier("goalIcon.symbol.\(name)")
	}
}
