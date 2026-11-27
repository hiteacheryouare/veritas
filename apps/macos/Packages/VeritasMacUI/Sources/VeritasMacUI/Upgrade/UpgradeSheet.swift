import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// "Upgrade to Veritas features" (docs/finance/migration.md MIG-1.3): the
/// offer after an import, then Overview, Review and Confirm. Only Confirm
/// has the button that writes.
struct UpgradeSheet: View {
	@Bindable var model: UpgradeModel
	let format: MoneyFormat
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s5) {
			Text("Upgrade to Veritas features")
				.font(.text(.title, weight: .semibold))
			switch model.step {
			case .offer: offer
			case .overview: overview
			case .review: review
			case .confirm: confirm
			}
		}
		.padding(Spacing.s5)
		.frame(width: 560)
		.frame(minHeight: 260, alignment: .top)
		.task { await model.load() }
		.sheet(item: $model.question) { question in
			UpgradeQuestionSheet(model: model, question: question, format: format)
		}
		.containerIdentifier("sheet.upgrade")
	}

	// MIG-1.1

	private var offer: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Veritas can move some of how you used Actual onto its own features: goals, the Net worth card, debt and savings categories, and bill names.")
				.fixedSize(horizontal: false, vertical: true)
			Text(UpgradeModel.nothingChanges)
				.foregroundStyle(.textSecondary)
			HStack {
				Spacer()
				Button("Not Now", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
					.accessibilityIdentifier("upgrade.notNow")
				Button("Show Me") { model.step = .overview }
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.accessibilityIdentifier("upgrade.showMe")
			}
		}
	}

	// MIG-1.3 (1)

	private var overview: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			if model.suggestions == nil {
				ProgressView().frame(maxWidth: .infinity)
			} else if model.counts.isEmpty {
				Text(UpgradeModel.nothingToUpgrade)
					.foregroundStyle(.textSecondary)
					.accessibilityIdentifier("upgrade.nothing")
			} else {
				GroupedRows {
					ForEach(Array(model.counts.enumerated()), id: \.element.conversion) { index, line in
						if index > 0 { Divider().padding(.leading, Spacing.s4) }
						LabeledRow(Self.overviewLine(line.conversion, count: line.count)) { EmptyView() }
					}
				}
				Text(UpgradeModel.nothingChanges)
					.foregroundStyle(.textSecondary)
			}
			if model.hasDismissed {
				Toggle("Show dismissed", isOn: $model.showsDismissed)
					.toggleStyle(.checkbox)
					.accessibilityIdentifier("upgrade.showDismissed")
			}
			if !model.applied.isEmpty {
				applied
			}
			HStack {
				Spacer()
				Button(model.counts.isEmpty ? "Close" : "Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				if !model.counts.isEmpty {
					Button("Review") { model.step = .review }
						.buttonStyle(.borderedProminent)
						.keyboardShortcut(.defaultAction)
						.accessibilityIdentifier("upgrade.review")
				}
			}
		}
	}

	/// MIG-1.5: each upgraded item keeps its own Revert.
	private var applied: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text("Upgraded")
				.font(.text(.body, weight: .semibold))
			GroupedRows {
				ForEach(Array(model.applied.enumerated()), id: \.element.key) { index, item in
					if index > 0 { Divider().padding(.leading, Spacing.s4) }
					LabeledRow(Self.conversionName(item.conversion)) {
						if item.closed {
							Text("Closed").foregroundStyle(.textSecondary)
						} else {
							Button("Revert") { Task { await model.revert(item.key) } }
								.accessibilityIdentifier("upgrade.revert.\(item.key)")
						}
					}
				}
			}
			ForEach(model.revertNotes, id: \.self) { note in
				ProblemNote(text: note)
			}
		}
	}

	// MIG-1.3 (2)

	private var review: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s3) {
					ForEach(model.visible, id: \.key) { suggestion in
						UpgradeRow(model: model, suggestion: suggestion)
					}
				}
			}
			.frame(maxHeight: 420)
			HStack {
				Button("Select Suggested") { model.selectSuggested() }
					.accessibilityIdentifier("upgrade.selectSuggested")
				Spacer()
				Button("Back") { model.step = .overview }
				Button("Continue") { Task { await model.continueToConfirm() } }
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(model.ticked.isEmpty)
					.accessibilityIdentifier("upgrade.continue")
			}
		}
	}

	// MIG-1.3 (3), MIG-2.4

	private var confirm: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s4) {
					ForEach(model.confirming, id: \.key) { suggestion in
						VStack(alignment: .leading, spacing: Spacing.s1) {
							Text(suggestion.subjectName)
								.font(.text(.body, weight: .semibold))
							ForEach(suggestion.confirmLines, id: \.self) { line in
								Text(line).fixedSize(horizontal: false, vertical: true)
							}
							if let preview = suggestion.goalPreview {
								ForEach(UpgradeWords.goalLines(preview, format: format), id: \.self) { line in
									Text(line).fixedSize(horizontal: false, vertical: true)
								}
							}
							if let change = suggestion.kindChange {
								Text(UpgradeWords.kindChange(change, format: format, reverting: false))
									.fixedSize(horizontal: false, vertical: true)
							}
							ForEach(suggestion.effects, id: \.report) { effect in
								Text(UpgradeWords.effect(effect, format: format))
									.foregroundStyle(.textSecondary)
							}
							if model.writesNoteInActual.contains(suggestion.key) {
								Text("Actual also gets a #goal line and this target in each month's goal.")
									.foregroundStyle(.textSecondary)
							}
						}
						.accessibilityElement(children: .combine)
						.accessibilityIdentifier("upgrade.confirm.\(suggestion.key)")
					}
				}
				.frame(maxWidth: .infinity, alignment: .leading)
			}
			.frame(maxHeight: 360)
			if model.needsGoalsGroupConsent {
				Toggle("Show the Goals group again", isOn: $model.consentShowGoalsGroup)
					.toggleStyle(.checkbox)
					.accessibilityIdentifier("upgrade.showGoalsGroup")
			}
			if let problem = model.problem {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Back") { model.step = .review }
				Button("Apply") {
					Task {
						if await model.apply() { dismiss() }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.disabled(!model.canApply)
				.accessibilityIdentifier("upgrade.apply")
			}
		}
	}

	static func conversionName(_ conversion: MigrationConversion) -> String {
		switch conversion {
		case .report: "Net worth card"
		case .goal: "Goal"
		case .kind: "Debt or savings category"
		case .bill: "Bill name"
		}
	}

	static func overviewLine(_ conversion: MigrationConversion, count: Int) -> String {
		switch conversion {
		case .report: count == 1 ? "1 report shown as the Net worth card" : "\(count) reports shown as the Net worth card"
		case .goal: count == 1 ? "1 envelope made a goal" : "\(count) envelopes made goals"
		case .kind: count == 1 ? "1 category marked as debt or savings" : "\(count) categories marked as debt or savings"
		case .bill: count == 1 ? "1 bill named" : "\(count) bills named"
		}
	}
}

/// One suggestion on the Review step: its box, why, and what the user sets.
private struct UpgradeRow: View {
	let model: UpgradeModel
	let suggestion: MigrationSuggestion

	/// MIG-6.3: any symbol the user likes, the proposed one first.
	static func icons(including proposed: String?) -> [String] {
		var seen = Set<String>()
		return ([proposed].compactMap(\.self) + ["arrow.down.circle", "banknote"] + GoalIcon.symbols)
			.filter { seen.insert($0).inserted }
	}

	private var isTicked: Bool { model.ticked.contains(suggestion.key) }

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Toggle(isOn: Binding(get: { isTicked }, set: { _ in model.toggle(suggestion) })) {
				VStack(alignment: .leading, spacing: 2) {
					Text(suggestion.subjectName).font(.text(.body, weight: .semibold))
					Text(UpgradeSheet.conversionName(suggestion.conversion))
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
				}
			}
			.toggleStyle(.checkbox)
			.disabled(!model.isTickable(suggestion) && !isTicked)
			.accessibilityIdentifier("upgrade.tick.\(suggestion.key)")
			VStack(alignment: .leading, spacing: Spacing.s1) {
				Text("Why we suggest this")
					.font(.text(.caption, weight: .semibold))
					.foregroundStyle(.textTertiary)
				ForEach(suggestion.signals, id: \.code) { signal in
					Text(signal.text).fixedSize(horizontal: false, vertical: true)
				}
				if !suggestion.conflicts.isEmpty {
					Text("These disagree, so check the target: \(suggestion.conflicts.joined(separator: ", ")).")
						.foregroundStyle(.textSecondary)
				}
				if let reason = model.blockedReason(suggestion) {
					ProblemNote(text: reason)
				}
			}
			.padding(.leading, Spacing.s5)
			if suggestion.conversion == .goal, isTicked, let field = model.targets[suggestion.key] {
				HStack {
					Text("Target")
					AmountTextField(field: field, takesFocus: false) { _ in }
						.frame(width: 140, height: 26)
						.accessibilityIdentifier("upgrade.target.\(suggestion.key)")
					if let date = suggestion.proposal.targetDate {
						Text("by \(UpgradeWords.day(date))").foregroundStyle(.textSecondary)
					}
				}
				.padding(.leading, Spacing.s5)
				if let problem = field.problem {
					ProblemNote(text: problem).padding(.leading, Spacing.s5)
				}
				if !suggestion.targetInActual {
					Toggle("Also show this target in Actual", isOn: Binding(
						get: { model.writesNoteInActual.contains(suggestion.key) },
						set: { on in
							if on { model.writesNoteInActual.insert(suggestion.key) } else { model.writesNoteInActual.remove(suggestion.key) }
						}
					))
					.toggleStyle(.checkbox)
					.padding(.leading, Spacing.s5)
					.accessibilityIdentifier("upgrade.noteInActual.\(suggestion.key)")
				}
			}
			if suggestion.conversion == .kind, isTicked {
				Picker("Kind", selection: Binding(get: { model.kind(for: suggestion) }, set: { model.kinds[suggestion.key] = $0 })) {
					Text("Debt principal").tag(CategoryKind.debtPrincipal)
					Text("Savings").tag(CategoryKind.savings)
				}
				.pickerStyle(.radioGroup)
				.horizontalRadioGroupLayout()
				.padding(.leading, Spacing.s5)
				.accessibilityIdentifier("upgrade.kind.\(suggestion.key)")
			}
			if suggestion.conversion == .bill, isTicked {
				HStack {
					Text("Name in Veritas")
					TextField("Name in Veritas", text: Binding(get: { model.displayName(for: suggestion) }, set: { model.displayNames[suggestion.key] = $0 }))
						.textFieldStyle(.roundedBorder)
						.labelsHidden()
						.frame(width: 220)
						.accessibilityIdentifier("upgrade.billName.\(suggestion.key)")
				}
				.padding(.leading, Spacing.s5)
				HStack {
					Text("Icon")
					Picker("Icon", selection: Binding(get: { model.icon(for: suggestion) }, set: { model.icons[suggestion.key] = $0 })) {
						Text("None").tag("")
						ForEach(UpgradeRow.icons(including: suggestion.proposal.icon), id: \.self) { symbol in
							Image(systemName: symbol).tag(symbol)
						}
					}
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("upgrade.billIcon.\(suggestion.key)")
				}
				.padding(.leading, Spacing.s5)
			}
			HStack {
				Spacer()
				Button(suggestion.dismissed ? "Suggest Again" : "Don't Suggest Again") {
					Task { await model.setDismissed(suggestion, !suggestion.dismissed) }
				}
				.buttonStyle(.link)
				.font(.text(.caption))
				.accessibilityIdentifier("upgrade.dismiss.\(suggestion.key)")
			}
		}
		.padding(Spacing.s3)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
	}
}

/// What a Revert asks before it writes: MIG-5.6's reversed confirm, or
/// MIG-4.7's group picker.
struct UpgradeQuestionSheet: View {
	let model: UpgradeModel
	let question: UpgradeModel.Question
	let format: MoneyFormat
	@State private var groups: [EnvelopeGroup] = []
	@State private var group: String?
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			switch question {
			case .kind(let key, let change, let effects, let replaces):
				Text("Revert this category's kind?")
					.font(.text(.title, weight: .semibold))
				if let replaces {
					Text("You set it to \(UpgradeWords.kindName(replaces)) since. Reverting replaces that.")
				}
				Text(UpgradeWords.kindChange(change, format: format, reverting: true))
					.fixedSize(horizontal: false, vertical: true)
				ForEach(effects, id: \.report) { effect in
					Text(UpgradeWords.effect(effect, format: format)).foregroundStyle(.textSecondary)
				}
				HStack {
					Spacer()
					Button("Cancel", role: .cancel) { model.question = nil }
						.keyboardShortcut(.cancelAction)
					Button("Revert") { Task { await model.revert(key, confirmed: true) } }
						.buttonStyle(.borderedProminent)
						.keyboardShortcut(.defaultAction)
						.accessibilityIdentifier("upgrade.confirmRevert")
				}
			case .group(let key, let message):
				Text(message).fixedSize(horizontal: false, vertical: true)
				Picker("Group", selection: $group) {
					Text("Choose a group").tag(String?.none)
					ForEach(groups, id: \.id) { group in
						Text(group.name).tag(String?.some(group.id))
					}
				}
				.accessibilityIdentifier("upgrade.chooseGroup")
				HStack {
					Spacer()
					Button("Cancel", role: .cancel) { model.question = nil }
						.keyboardShortcut(.cancelAction)
					Button("Move It There") {
						guard let group else { return }
						Task { await model.revert(key, groupID: group) }
					}
					.buttonStyle(.borderedProminent)
					.disabled(group == nil)
				}
			}
		}
		.padding(Spacing.s5)
		.frame(width: 460)
		.task {
			if case .group = question {
				groups = await model.liveGroups()
			}
		}
	}
}
