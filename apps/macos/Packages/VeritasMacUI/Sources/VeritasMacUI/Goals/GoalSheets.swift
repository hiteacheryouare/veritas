import AppKit
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// R-GOAL-1: a name, a target, an optional date, and an optional icon and color.
struct NewGoalSheet: View {
	let goals: GoalsScreenModel
	@State private var name = ""
	@State private var target: AmountFieldModel
	@State private var hasDate = true
	@State private var date = Calendar.current.date(byAdding: .month, value: 6, to: .now) ?? .now
	@State private var icon: String?
	@State private var color = Color(ColorToken.moneyIncomeFill)
	@State private var usesColor = false
	@FocusState private var isNameFocused: Bool
	@Environment(\.dismiss) private var dismiss

	static let icons = ["laptopcomputer", "airplane", "car", "house", "gift", "heart", "graduationcap", "umbrella"]

	init(goals: GoalsScreenModel) {
		self.goals = goals
		_target = State(initialValue: AmountFieldModel(amount: 0, style: goals.numberStyle, reader: goals.reader))
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text("New goal")
				.font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("Name") {
					TextField("Name", text: $name)
						.textFieldStyle(.roundedBorder)
						.labelsHidden()
						.frame(width: 240)
						.focused($isNameFocused)
						.accessibilityIdentifier("newGoal.name")
				}
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Target") {
					AmountTextField(field: target, takesFocus: false) { exit in
						if exit == .cancel { dismiss() }
					}
					.frame(width: 140, height: 26)
				}
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Target date") {
					HStack {
						Toggle("Target date", isOn: $hasDate).labelsHidden()
						DatePicker("Target date", selection: $date, in: Date.now..., displayedComponents: .date)
							.labelsHidden()
							.disabled(!hasDate)
					}
				}
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Icon") {
					Picker("Icon", selection: $icon) {
						Text("None").tag(String?.none)
						ForEach(Self.icons, id: \.self) { symbol in
							Image(systemName: symbol).tag(String?.some(symbol))
						}
					}
					.labelsHidden()
					.fixedSize()
				}
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Color") {
					HStack {
						Toggle("Color", isOn: $usesColor).labelsHidden()
						ColorPicker("Color", selection: $color, supportsOpacity: false)
							.labelsHidden()
							.disabled(!usesColor)
					}
				}
			}
			if let problem = target.problem {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Create goal", action: create)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
					.accessibilityIdentifier("newGoal.create")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 500)
		.defaultFocus($isNameFocused, true)
		.containerIdentifier("sheet.newGoal")
	}

	private func create() {
		Task {
			guard let amount = await target.commit() else { return }
			let hex = usesColor ? Self.hex(color) : nil
			if await goals.create(name: name.trimmingCharacters(in: .whitespaces), target: amount, date: hasDate ? date : nil, icon: icon, color: hex) {
				dismiss()
			}
		}
	}

	/// `#rrggbb`, as Actual stores colors.
	static func hex(_ color: Color) -> String? {
		guard let rgb = NSColor(color).usingColorSpace(.sRGB) else { return nil }
		let parts = [rgb.redComponent, rgb.greenComponent, rgb.blueComponent].map { Int(($0 * 255).rounded()) }
		return String(format: "#%02x%02x%02x", parts[0], parts[1], parts[2])
	}
}

/// R-GOAL-4 and D66: release the leftover to an envelope as a budget move,
/// optionally move later months' budget too, then hide the goal.
struct CloseGoalSheet: View {
	let goals: GoalsScreenModel
	let goal: GoalSummary
	@State private var destinations: [EnvelopeRow]
	@State private var destination: String?
	@State private var preview: GoalClosePreview?
	@State private var alsoMoveLater = false
	@Environment(\.dismiss) private var dismiss

	init(goals: GoalsScreenModel, goal: GoalSummary, destinations: [EnvelopeRow] = [], preview: GoalClosePreview? = nil) {
		self.goals = goals
		self.goal = goal
		_destinations = State(initialValue: destinations)
		_preview = State(initialValue: preview)
	}

	private var destinationName: String {
		destinations.first { $0.id == destination }?.name ?? "To Budget"
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			VStack(alignment: .leading, spacing: Spacing.s2) {
				Text("Close \(goal.name)?")
					.font(.text(.title, weight: .semibold))
				Text("It leaves the budget but stays in reports, Wrapped and the archive.")
					.foregroundStyle(.textSecondary)
			}
			VStack(alignment: .leading, spacing: Spacing.s3) {
				if let preview {
					Label {
						Text(preview.leftover >= 0
							? "\(goals.format.currency(preview.leftover)) is left in this goal."
							: "This goal is \(goals.format.currency(-preview.leftover)) over.")
					} icon: {
						Image(systemName: preview.leftover >= 0 ? "checkmark.circle" : "exclamationmark.triangle")
					}
					.font(.text(.body, weight: .semibold))
					.foregroundStyle(preview.leftover >= 0 ? Color.textPrimary : Color(ColorToken.moneyOverspent))
				}
				HStack {
					Text(preview.map { $0.leftover >= 0 ? "Move it to" : "Cover it from" } ?? "Move it to")
						.foregroundStyle(.textSecondary)
					EnvelopePicker(title: "Destination", selection: $destination, envelopes: destinations, format: goals.format)
				}
				if let preview, let before = beforeAvailable {
					HStack(spacing: Spacing.s2) {
						Text("\(destinationName) available")
						Spacer()
						Text(goals.format.amount(before)).font(.figures(.body, weight: .semibold))
						Image(systemName: "arrow.right").imageScale(.small)
						Text(goals.format.amount(preview.destinationAvailableAfter)).font(.figures(.body, weight: .semibold))
					}
					.foregroundStyle(.textSecondary)
				}
				if let preview, preview.destinationAvailableAfter < 0 {
					ProblemNote(text: "\(destinationName) will be \(goals.format.currency(-preview.destinationAvailableAfter)) over.")
						.accessibilityIdentifier("closeGoal.warning")
				}
			}
			.padding(Spacing.s4)
			.frame(maxWidth: .infinity, alignment: .leading)
			.background(.surfaceHover, in: .rect(cornerRadius: Radius.section))
			if let later = preview?.laterBudget, !later.isEmpty {
				Toggle(isOn: $alsoMoveLater) {
					VStack(alignment: .leading, spacing: 2) {
						Text("Also move those to \(destinationName)")
						Text(later.map { "\(GoalWords.monthYear($0.month)) \(goals.format.amount($0.amount))" }.joined(separator: ", "))
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
					}
				}
				.toggleStyle(.checkbox)
				.accessibilityIdentifier("closeGoal.alsoMoveLater")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Close goal") {
					Task {
						if await goals.close(goal, releaseTo: destination, alsoMoveLater: alsoMoveLater) { dismiss() }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.accessibilityIdentifier("closeGoal.close")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 520)
		.task {
			destinations = await goals.destinations()
			preview = await goals.preview(close: goal, releaseTo: destination)
		}
		.onChange(of: destination) {
			Task { preview = await goals.preview(close: goal, releaseTo: destination) }
		}
		.containerIdentifier("sheet.closeGoal")
	}

	private var beforeAvailable: Int64? {
		guard let preview else { return nil }
		return preview.destinationAvailableAfter - preview.leftover
	}
}

/// A new target or date on an open goal; a closed one is reopened first.
struct RetargetGoalSheet: View {
	let goals: GoalsScreenModel
	let goal: GoalSummary
	@State private var target: AmountFieldModel
	@State private var hasDate: Bool
	@State private var date: Date
	@Environment(\.dismiss) private var dismiss

	init(goals: GoalsScreenModel, goal: GoalSummary) {
		self.goals = goals
		self.goal = goal
		_target = State(initialValue: AmountFieldModel(amount: goal.target, style: goals.numberStyle, reader: goals.reader))
		_hasDate = State(initialValue: goal.targetMonth != nil)
		_date = State(initialValue: goal.targetMonth.flatMap(GoalWords.date) ?? .now)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text("Change target for \(goal.name)")
				.font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("Target") {
					AmountTextField(field: target) { exit in
						if exit == .cancel { dismiss() } else { save() }
					}
					.frame(width: 140, height: 26)
				}
				Divider().padding(.leading, Spacing.s4)
				LabeledRow("Target date") {
					HStack {
						Toggle("Target date", isOn: $hasDate).labelsHidden()
						DatePicker("Target date", selection: $date, displayedComponents: .date)
							.labelsHidden()
							.disabled(!hasDate)
					}
				}
			}
			if let problem = target.problem {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Save target", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
			}
		}
		.padding(Spacing.s5)
		.frame(width: 460)
		.containerIdentifier("sheet.retargetGoal")
	}

	private func save() {
		Task {
			guard let amount = await target.commit() else { return }
			if await goals.retarget(goal, target: amount, date: hasDate ? date : nil) { dismiss() }
		}
	}
}
