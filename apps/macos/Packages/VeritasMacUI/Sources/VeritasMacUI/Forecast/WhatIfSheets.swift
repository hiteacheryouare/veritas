import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The fields of one what-if event, for each `ScenarioChange` kind (FC-3).
/// Register amounts and a goal's budget amount each have their own field,
/// so switching kinds never reads an amount at the wrong scale.
private struct EventFields: View {
	let model: WhatIfsModel
	@Binding var draft: ScenarioDraft
	let registerAmount: AmountFieldModel
	let budgetAmount: AmountFieldModel
	let cancel: () -> Void

	var body: some View {
		GroupedRows {
			LabeledRow("Kind") {
				Picker("Kind", selection: $draft.kind) {
					ForEach(ScenarioDraft.Kind.allCases, id: \.self) { Text($0.title).tag($0) }
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("whatIf.kind")
			}
			switch draft.kind {
			case .oneTime:
				divider
				LabeledRow("Date") { datePicker }
				accountRow
				categoryRow(model.expenseCategories + model.incomeCategories, optional: true)
				directionRow(down: "Money out", up: "Money in")
				amountRow(registerAmount)
			case .recurring:
				divider
				LabeledRow("Starts") { datePicker }
				divider
				LabeledRow("Repeats") {
					HStack(spacing: Spacing.s2) {
						Text("Every")
						TextField("Count", value: $draft.cadenceCount, format: .number)
							.textFieldStyle(.roundedBorder)
							.frame(width: 44)
							.multilineTextAlignment(.trailing)
							.accessibilityIdentifier("whatIf.cadenceCount")
						Picker("Unit", selection: $draft.cadenceUnit) {
							Text(draft.cadenceCount == 1 ? "month" : "months").tag(ScenarioDraft.CadenceUnit.months)
							Text(draft.cadenceCount == 1 ? "week" : "weeks").tag(ScenarioDraft.CadenceUnit.weeks)
						}
						.labelsHidden()
						.fixedSize()
					}
				}
				endRow
				accountRow
				categoryRow(model.expenseCategories + model.incomeCategories, optional: true)
				directionRow(down: "Money out", up: "Money in")
				amountRow(registerAmount)
			case .categoryPercent:
				categoryRow(model.expenseCategories, optional: false)
				directionRow(down: "Decrease", up: "Increase")
				percentRow
				startRow
				endRow
			case .incomeChange:
				divider
				LabeledRow("Income") {
					Picker("Income", selection: $draft.categoryID) {
						Text("All income").tag(String?.none)
						categoryOptions(model.incomeCategories)
					}
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("whatIf.income")
				}
				accountRow
				divider
				LabeledRow("By") {
					Picker("By", selection: $draft.incomeByPercent) {
						Text("Amount").tag(false)
						Text("Percent").tag(true)
					}
					.pickerStyle(.segmented)
					.labelsHidden()
					.fixedSize()
				}
				directionRow(down: "Decrease", up: "Increase")
				if draft.incomeByPercent { percentRow } else { amountRow(registerAmount, caption: "a month") }
				startRow
				endRow
			case .goalContribution:
				divider
				LabeledRow("Goal") {
					Picker("Goal", selection: $draft.goalID) {
						Text("Choose").tag(String?.none)
						ForEach(model.activeGoals, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
					}
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("whatIf.goal")
				}
				amountRow(budgetAmount, caption: "a month")
				startRow
				endRow
			}
		}
	}

	private var divider: some View {
		Divider().padding(.leading, Spacing.s4)
	}

	private var datePicker: some View {
		DatePicker(
			"Date",
			selection: Binding(get: { CivilDate.date(draft.date) ?? .now }, set: { draft.date = CivilDate.text($0) }),
			in: (CivilDate.date(model.today) ?? .now)...,
			displayedComponents: .date
		)
		.labelsHidden()
		.accessibilityIdentifier("whatIf.date")
	}

	@ViewBuilder private var accountRow: some View {
		divider
		LabeledRow("Account") {
			Picker("Account", selection: $draft.accountID) {
				Text("Choose").tag(String?.none)
				ForEach(model.accounts, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("whatIf.account")
		}
	}

	@ViewBuilder private func categoryRow(_ groups: [EnvelopeGroup], optional: Bool) -> some View {
		divider
		LabeledRow("Category") {
			Picker("Category", selection: $draft.categoryID) {
				Text(optional ? "None" : "Choose").tag(String?.none)
				categoryOptions(groups)
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("whatIf.category")
		}
	}

	@ViewBuilder private func categoryOptions(_ groups: [EnvelopeGroup]) -> some View {
		ForEach(groups, id: \.id) { group in
			Section(group.name) {
				ForEach(group.envelopes, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
			}
		}
	}

	@ViewBuilder private func directionRow(down: String, up: String) -> some View {
		divider
		LabeledRow("Direction") {
			Picker("Direction", selection: $draft.direction) {
				Text(down).tag(ScenarioDraft.Direction.down)
				Text(up).tag(ScenarioDraft.Direction.up)
			}
			.pickerStyle(.segmented)
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("whatIf.direction")
		}
	}

	@ViewBuilder private func amountRow(_ field: AmountFieldModel, caption: String? = nil) -> some View {
		divider
		LabeledRow("Amount") {
			HStack(spacing: Spacing.s2) {
				AmountTextField(field: field, takesFocus: false) { exit in
					if exit == .cancel { cancel() }
				}
				.frame(width: 140, height: 26)
				if let caption {
					Text(caption).foregroundStyle(.textSecondary)
				}
			}
		}
	}

	@ViewBuilder private var percentRow: some View {
		divider
		LabeledRow("Percent") {
			HStack(spacing: Spacing.s2) {
				TextField("10", text: $draft.percentText)
					.textFieldStyle(.roundedBorder)
					.multilineTextAlignment(.trailing)
					.frame(width: 80)
					.accessibilityIdentifier("whatIf.percent")
				Text("%")
			}
		}
	}

	@ViewBuilder private var startRow: some View {
		divider
		LabeledRow("From") {
			Picker("From", selection: $draft.startMonth) {
				ForEach(model.months, id: \.self) { Text(ReportPeriod.monthYear($0)).tag($0) }
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("whatIf.from")
		}
	}

	@ViewBuilder private var endRow: some View {
		divider
		LabeledRow("Through") {
			Picker("Through", selection: $draft.endMonth) {
				Text("No end").tag(Month?.none)
				ForEach(model.months, id: \.self) { Text(ReportPeriod.monthYear($0)).tag(Month?.some($0)) }
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("whatIf.through")
		}
	}
}

/// The amount fields and the draft an event editor works on.
@MainActor
private struct EventForm {
	var draft: ScenarioDraft
	let registerAmount: AmountFieldModel
	let budgetAmount: AmountFieldModel

	init(model: WhatIfsModel, draft: ScenarioDraft) {
		self.draft = draft
		let register = draft.kind.scale == .register ? draft.magnitude : 0
		let budget = draft.kind.scale == .budget ? draft.magnitude : 0
		registerAmount = AmountFieldModel(amount: register, style: model.numberStyle, reader: model.reader(.register))
		budgetAmount = AmountFieldModel(amount: budget, style: model.numberStyle, reader: model.reader(.budget))
	}

	/// The change to send, or the line to show under the fields.
	func change() async -> Result<ScenarioChange, ScenarioDraft.Problem> {
		var magnitude: Int64 = 0
		if draft.usesAmountField {
			let field = draft.kind.scale == .budget ? budgetAmount : registerAmount
			guard let amount = await field.commit() else {
				return .failure(ScenarioDraft.Problem(message: field.problem ?? "Enter an amount."))
			}
			magnitude = amount
		}
		do {
			return .success(try draft.change(magnitude: magnitude))
		} catch {
			return .failure(error)
		}
	}
}

/// Adds an event to a what-if, or changes one. The core checks it: a date
/// before today, an end before the start and a bad percent come back here.
struct EventSheet: View {
	let model: WhatIfsModel
	let editing: EventEditing
	@State private var form: EventForm
	@State private var problem: String?
	@Environment(\.dismiss) private var dismiss

	init(model: WhatIfsModel, editing: EventEditing) {
		self.model = model
		self.editing = editing
		let draft = editing.event.map { ScenarioDraft($0.change, today: model.today) } ?? model.newDraft()
		_form = State(initialValue: EventForm(model: model, draft: draft))
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text(editing.event == nil ? "Add event to \(editing.scenario.name)" : "Edit event")
				.font(.text(.title, weight: .semibold))
			if editing.event?.inThePast == true {
				Text("This event is in the past. Move it to today or later, or remove it.")
					.foregroundStyle(.textSecondary)
			}
			EventFields(model: model, draft: $form.draft, registerAmount: form.registerAmount, budgetAmount: form.budgetAmount) { dismiss() }
			if let problem {
				ProblemNote(text: problem).accessibilityIdentifier("whatIf.problem")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button(editing.event == nil ? "Add event" : "Save", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.accessibilityIdentifier("whatIf.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 520)
		.containerIdentifier("sheet.whatIfEvent")
	}

	private func save() {
		Task {
			problem = nil
			switch await form.change() {
			case .failure(let failure):
				problem = failure.message
			case .success(let change):
				let show: (String) -> Void = { problem = $0 }
				let done = if let event = editing.event {
					await model.edit(event.id, in: editing.scenario.id, to: change, inline: show)
				} else {
					await model.add(change, to: editing.scenario.id, inline: show)
				}
				if done { dismiss() }
			}
		}
	}
}

/// "Add what-if": a name and its first event. A what-if with no events
/// would draw on top of the baseline, so it starts with one.
struct NewWhatIfSheet: View {
	let model: WhatIfsModel
	@State private var newWhatIf: NewWhatIf
	@State private var form: EventForm
	@FocusState private var isNameFocused: Bool
	@Environment(\.dismiss) private var dismiss

	init(model: WhatIfsModel) {
		self.model = model
		_newWhatIf = State(initialValue: NewWhatIf(model: model))
		_form = State(initialValue: EventForm(model: model, draft: model.newDraft()))
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("New what-if")
				.font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("Name") {
					TextField("Name", text: $newWhatIf.name)
						.textFieldStyle(.roundedBorder)
						.labelsHidden()
						.frame(width: 260)
						.focused($isNameFocused)
						.accessibilityIdentifier("whatIf.name")
				}
			}
			EventFields(model: model, draft: $form.draft, registerAmount: form.registerAmount, budgetAmount: form.budgetAmount) { close() }
			if let problem = newWhatIf.problem {
				ProblemNote(text: problem).accessibilityIdentifier("whatIf.problem")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel, action: close)
					.keyboardShortcut(.cancelAction)
				Button("Add what-if", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(newWhatIf.trimmedName.isEmpty)
					.accessibilityIdentifier("whatIf.create")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 520)
		.defaultFocus($isNameFocused, true)
		.task { if model.names == .empty { await model.load() } }
		.containerIdentifier("sheet.newWhatIf")
	}

	private func save() {
		Task {
			newWhatIf.show(nil)
			switch await form.change() {
			case .failure(let failure):
				newWhatIf.show(failure.message)
			case .success(let change):
				if await newWhatIf.save(change) != nil { dismiss() }
			}
		}
	}

	private func close() {
		Task {
			await newWhatIf.cancel()
			dismiss()
		}
	}
}

struct RenameWhatIfSheet: View {
	let model: WhatIfsModel
	let scenario: ScenarioSummary
	@State private var name: String
	@State private var problem: String?
	@Environment(\.dismiss) private var dismiss

	init(model: WhatIfsModel, scenario: ScenarioSummary) {
		self.model = model
		self.scenario = scenario
		_name = State(initialValue: scenario.name)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Rename \(scenario.name)")
				.font(.text(.title, weight: .semibold))
			TextField("Name", text: $name)
				.textFieldStyle(.roundedBorder)
				.accessibilityIdentifier("whatIf.rename.name")
			if let problem {
				ProblemNote(text: problem)
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Rename") {
					Task {
						problem = nil
						if await model.rename(scenario.id, to: name.trimmingCharacters(in: .whitespaces), inline: { problem = $0 }) { dismiss() }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
				.accessibilityIdentifier("whatIf.rename.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 400)
		.containerIdentifier("sheet.renameWhatIf")
	}
}
