import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The sentence editor: When rows, Then rows, numbered split lines and the
/// stage. The core checks the draft on Save and on Apply to existing; its
/// message shows above the buttons.
struct RuleEditorSheet: View {
	@Bindable var model: RulesModel
	@State private var amounts = AmountSlots()

	private var editing: RuleEditing {
		model.editing ?? RuleEditing(id: nil, draft: RuleDraft(stage: .default, conditionsOp: .and, conditions: [], actions: []))
	}

	private var today: String { WorkspaceModel.today() }

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			HStack {
				Text(editing.isNew ? "New rule" : "Edit rule")
					.font(.text(.title, weight: .semibold))
				Spacer()
				Picker("Stage", selection: draft(\.stage)) {
					ForEach(RulesScreen.stages, id: \.self) { stage in
						Text(RuleWords.stage(stage)).tag(stage)
					}
				}
				.pickerStyle(.segmented)
				.fixedSize()
				.accessibilityIdentifier("ruleEditor.stage")
			}
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s4) {
					conditions
					actions
					ForEach(editing.splitLines, id: \.self) { line in
						splitLine(line)
					}
				}
				.padding(.bottom, Spacing.s1)
			}
			.frame(maxHeight: 520)
			.fixedSize(horizontal: false, vertical: true)
			if let problem = editing.problem {
				ProblemNote(text: problem)
					.accessibilityIdentifier("ruleEditor.problem")
			}
			HStack {
				Button("Apply to existing…", action: preview)
					.disabled(model.isPreviewing)
					.accessibilityIdentifier("ruleEditor.applyToExisting")
				Spacer()
				Button("Cancel", role: .cancel) { model.cancelEditing() }
					.keyboardShortcut(.cancelAction)
					.accessibilityIdentifier("ruleEditor.cancel")
				Button("Save", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.accessibilityIdentifier("ruleEditor.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 680)
		.sheet(isPresented: Binding(get: { model.applying != nil }, set: { if !$0 { model.applying = nil } })) {
			RuleApplySheet(model: model)
		}
		.containerIdentifier("sheet.ruleEditor")
	}

	// MARK: When

	private var conditions: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(spacing: Spacing.s2) {
				Text("When")
					.font(.text(.heading, weight: .semibold))
				Picker("Match", selection: draft(\.conditionsOp)) {
					Text("all of these match").tag(RuleConditionsOp.and)
					Text("any of these match").tag(RuleConditionsOp.or)
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("ruleEditor.conditionsOp")
			}
			GroupedRows {
				ForEach(Array(editing.draft.conditions.enumerated()), id: \.offset) { index, condition in
					if index > 0 { Divider().padding(.leading, Spacing.s4) }
					conditionRow(index, condition)
				}
			}
			Button {
				change { $0.addCondition() }
			} label: {
				Label("Condition", systemImage: "plus")
			}
			.buttonStyle(.link)
			.accessibilityIdentifier("ruleEditor.addCondition")
		}
	}

	private func conditionRow(_ index: Int, _ condition: RuleCondition) -> some View {
		HStack(spacing: Spacing.s2) {
			Picker("Field", selection: Binding(get: { condition.field }, set: { field in change { $0.setField(field, ofCondition: index, today: today) } })) {
				ForEach(Self.including(condition.field, in: RuleMenus.conditionFields), id: \.self) { field in
					Text(RuleWords.field(field)).tag(field)
				}
			}
			.labelsHidden()
			.frame(width: 150, alignment: .leading)
			.accessibilityIdentifier("ruleEditor.condition.\(index).field")
			Picker("Op", selection: Binding(get: { condition.op }, set: { op in change { $0.setOp(op, ofCondition: index, today: today) } })) {
				ForEach(Self.including(condition.op, in: RuleMenus.ops(for: condition.field)), id: \.self) { op in
					Text(RuleWords.op(op, on: condition.field)).tag(op)
				}
			}
			.labelsHidden()
			.frame(width: 160, alignment: .leading)
			.accessibilityIdentifier("ruleEditor.condition.\(index).op")
			RuleValueEditor(
				value: conditionValue(index), field: condition.field, model: model, amounts: amounts, key: "condition.\(index)"
			)
			.frame(maxWidth: .infinity, alignment: .leading)
			.accessibilityIdentifier("ruleEditor.condition.\(index).value")
			removeButton("ruleEditor.condition.\(index).remove") { change { $0.removeCondition(at: index) } }
				.disabled(editing.draft.conditions.count == 1)
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s1)
		.frame(minHeight: Size.row)
	}

	// MARK: Then

	private var actions: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			Text("Then")
				.font(.text(.heading, weight: .semibold))
			let indices = editing.actionIndices(line: 0)
			if !indices.isEmpty {
				GroupedRows {
					ForEach(indices, id: \.self) { index in
						if index != indices.first { Divider().padding(.leading, Spacing.s4) }
						actionRow(index, choices: RuleMenus.actions)
					}
				}
			}
			HStack(spacing: Spacing.s4) {
				Button {
					change { $0.addAction(line: 0, today: today) }
				} label: {
					Label("Action", systemImage: "plus")
				}
				.accessibilityIdentifier("ruleEditor.addAction")
				Button {
					change { $0.addSplitLine() }
				} label: {
					Label("Split", systemImage: "plus")
				}
				.accessibilityIdentifier("ruleEditor.addSplit")
			}
			.buttonStyle(.link)
		}
	}

	/// A row can redraw with its old index after Save or Cancel clears the draft.
	@ViewBuilder private func actionRow(_ index: Int, choices: [RuleMenus.ActionKind]) -> some View {
		if let action = editing.draft.actions[safe: index] {
			actionRow(index, action: action, choices: choices)
		}
	}

	private func actionRow(_ index: Int, action: RuleAction, choices: [RuleMenus.ActionKind]) -> some View {
		let kind = RuleMenus.ActionKind(action)
		return HStack(spacing: Spacing.s2) {
			Picker("Action", selection: Binding(get: { kind }, set: { kind in if let kind { change { $0.setKind(kind, ofAction: index, today: today) } } })) {
				ForEach(Self.including(kind, in: choices), id: \.self) { choice in
					Text(RuleWords.action(choice)).tag(Optional(choice))
				}
			}
			.labelsHidden()
			.frame(width: 318, alignment: .leading)
			.accessibilityIdentifier("ruleEditor.action.\(index).kind")
			RuleValueEditor(
				value: actionValue(index), field: action.op == .set ? action.field : .notes, model: model, amounts: amounts, key: "action.\(index)"
			)
			.frame(maxWidth: .infinity, alignment: .leading)
			.accessibilityIdentifier("ruleEditor.action.\(index).value")
			removeButton("ruleEditor.action.\(index).remove") { change { $0.removeAction(at: index) } }
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s1)
		.frame(minHeight: Size.row)
	}

	// MARK: Split lines

	private func splitLine(_ line: UInt32) -> some View {
		let indices = editing.actionIndices(line: line)
		let allocation = indices.first.map { editing.draft.actions[$0] }
		return VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack {
				Text("Split \(line)")
					.font(.text(.heading, weight: .semibold))
				Spacer()
				Button("Remove split \(line)") { change { $0.removeSplitLine(line) } }
					.buttonStyle(.link)
					.accessibilityIdentifier("ruleEditor.split.\(line).remove")
			}
			GroupedRows {
				if let first = indices.first, let allocation, allocation.op == .setSplitAmount {
					HStack(spacing: Spacing.s2) {
						Text(RuleWords.action(RuleActionOp.setSplitAmount))
						Picker("Allocation", selection: Binding(
							get: { RuleMenus.Allocation(allocation.value) },
							set: { choice in change { $0.setAllocation(choice, line: line) } }
						)) {
							ForEach(RuleMenus.allocations, id: \.self) { choice in
								Text(RuleWords.allocation(choice)).tag(choice)
							}
						}
						.labelsHidden()
						.fixedSize()
						.accessibilityIdentifier("ruleEditor.split.\(line).allocation")
						RuleValueEditor(value: actionValue(first), field: .amount, model: model, amounts: amounts, key: "action.\(first)")
							.accessibilityIdentifier("ruleEditor.split.\(line).share")
						Spacer(minLength: 0)
					}
					.padding(.horizontal, Spacing.s4)
					.frame(minHeight: Size.row)
				}
				ForEach(indices.dropFirst(), id: \.self) { index in
					Divider().padding(.leading, Spacing.s4)
					actionRow(index, choices: RuleMenus.splitLineActions)
				}
			}
			Button {
				change { $0.addAction(line: line, today: today) }
			} label: {
				Label("Action", systemImage: "plus")
			}
			.buttonStyle(.link)
			.accessibilityIdentifier("ruleEditor.split.\(line).addAction")
		}
		.containerIdentifier("ruleEditor.split.\(line)")
	}

	// MARK: Plumbing

	private func removeButton(_ identifier: String, action: @escaping () -> Void) -> some View {
		Button(action: action) {
			Image(systemName: "minus.circle")
				.accessibilityLabel("Remove")
		}
		.buttonStyle(.borderless)
		.foregroundStyle(.textSecondary)
		.accessibilityIdentifier(identifier)
	}

	/// A change that adds, removes or retypes a row: amount fields are rebuilt
	/// from the draft, which they already hold.
	private func change(_ edit: (inout RuleEditing) -> Void) {
		guard var editing = model.editing else { return }
		edit(&editing)
		amounts.reset()
		model.editing = editing
	}

	private func draft<T>(_ path: WritableKeyPath<RuleDraft, T>) -> Binding<T> {
		Binding(get: { editing.draft[keyPath: path] }, set: { model.editing?.draft[keyPath: path] = $0 })
	}

	private func conditionValue(_ index: Int) -> Binding<RuleValue> {
		Binding(
			get: { model.editing?.draft.conditions[safe: index]?.value ?? .nothing },
			set: { value in if model.editing?.draft.conditions.indices.contains(index) == true { model.editing?.draft.conditions[index].value = value } }
		)
	}

	private func actionValue(_ index: Int) -> Binding<RuleValue> {
		Binding(
			get: { model.editing?.draft.actions[safe: index]?.value ?? .nothing },
			set: { value in if model.editing?.draft.actions.indices.contains(index) == true { model.editing?.draft.actions[index].value = value } }
		)
	}

	/// The menu's choices, plus the stored one when Actual's menu wouldn't list it.
	static func including<T: Equatable>(_ current: T?, in choices: [T]) -> [T] {
		guard let current, !choices.contains(current) else { return choices }
		return choices + [current]
	}

	private func save() {
		Task {
			guard await amounts.commitAll() else { return }
			await model.save()
		}
	}

	private func preview() {
		Task {
			guard await amounts.commitAll(), let draft = model.editing?.draft else { return }
			await model.preview(.draft(draft: draft))
		}
	}
}

extension Array {
	subscript(safe index: Int) -> Element? {
		indices.contains(index) ? self[index] : nil
	}
}

/// The editor's amount fields, kept across renders so typing isn't lost.
/// Each writes its amount into the draft as it reads, and Save reads them all
/// through the core first.
@MainActor
@Observable
final class AmountSlots {
	@ObservationIgnored private var fields: [String: AmountFieldModel] = [:]
	@ObservationIgnored private var writers: [String: (Int64) -> Void] = [:]

	func field(_ key: String, amount: Int64, model: RulesModel, write: @escaping (Int64) -> Void) -> AmountFieldModel {
		writers[key] = write
		if let field = fields[key] { return field }
		let field = AmountFieldModel(amount: amount, style: model.numberStyle, reader: model.reader)
		fields[key] = field
		return field
	}

	func reset() {
		fields = [:]
		writers = [:]
	}

	/// False when a field can't be read; that field shows why.
	func commitAll() async -> Bool {
		var readable = true
		for (key, field) in fields.sorted(by: { $0.key < $1.key }) {
			if let amount = await field.commit() {
				writers[key]?(amount)
			} else {
				readable = false
			}
		}
		return readable
	}
}

/// The value part of a When or Then row, chosen by the value's kind.
struct RuleValueEditor: View {
	@Binding var value: RuleValue
	let field: RuleField?
	let model: RulesModel
	let amounts: AmountSlots
	let key: String

	var body: some View {
		switch value {
		case .id(let id):
			IdPicker(selection: Binding(get: { id }, set: { value = .id(id: $0) }), field: field, names: model.names)
		case .ids(let ids):
			IdList(ids: Binding(get: { ids }, set: { value = .ids(ids: $0) }), field: field, names: model.names)
		case .text(let text):
			TextField("Text", text: Binding(get: { text }, set: { value = .text(text: $0) }))
				.textFieldStyle(.roundedBorder)
				.labelsHidden()
		case .texts(let texts):
			TextList(texts: Binding(get: { texts }, set: { value = .texts(texts: $0) }))
		case .amount(let amount):
			AmountValueField(slots: amounts, key: key, amount: amount, model: model) { value = .amount(amount: $0) }
		case .amountRange(let low, let high):
			HStack(spacing: Spacing.s2) {
				AmountValueField(slots: amounts, key: "\(key).low", amount: low, model: model) { new in
					if case .amountRange(_, let high) = value { value = .amountRange(num1: new, num2: high) }
				}
				Text("and").foregroundStyle(.textSecondary).fixedSize()
				AmountValueField(slots: amounts, key: "\(key).high", amount: high, model: model) { new in
					if case .amountRange(let low, _) = value { value = .amountRange(num1: low, num2: new) }
				}
			}
		case .date(let date):
			dayPicker(date)
		case .month(let month):
			dayPicker("\(month)-01")
		case .year(let year):
			dayPicker(String(format: "%04d-01-01", year))
		case .bool(let flag):
			Picker("Value", selection: Binding(get: { flag }, set: { value = .bool(value: $0) })) {
				Text("true").tag(true)
				Text("false").tag(false)
			}
			.labelsHidden()
			.fixedSize()
		case .percent(let percent):
			HStack(spacing: Spacing.s1) {
				TextField("Percent", value: Binding(get: { percent }, set: { value = .percent(percent: $0) }), format: .number)
					.textFieldStyle(.roundedBorder)
					.labelsHidden()
					.multilineTextAlignment(.trailing)
					.frame(width: 70)
				Text("%").foregroundStyle(.textSecondary)
			}
		case .nothing:
			EmptyView()
		case .raw(let json):
			Text(json)
				.font(.text(.caption))
				.foregroundStyle(.textSecondary)
				.lineLimit(1)
		}
	}

	private func dayPicker(_ day: String) -> some View {
		DatePicker("Date", selection: Binding(
			get: { Self.date(day) ?? .now },
			set: { value = .date(date: WorkspaceModel.today($0)) }
		), displayedComponents: .date)
		.labelsHidden()
		.fixedSize()
	}

	static func date(_ day: String) -> Date? {
		CivilDate.date(day)
	}
}

private struct AmountValueField: View {
	let slots: AmountSlots
	let key: String
	let amount: Int64
	let model: RulesModel
	let write: (Int64) -> Void

	var body: some View {
		let field = slots.field(key, amount: amount, model: model, write: write)
		AmountTextField(field: field, takesFocus: false) { _ in }
			.frame(width: 110, height: 24)
			.task(id: field.text) {
				guard !field.isUntouched, let read = try? await model.reader.evaluate(field.text, style: model.numberStyle), !Task.isCancelled else { return }
				write(read)
			}
	}
}

/// A payee, account, category or category group, from the budget's names.
private struct IdPicker: View {
	@Binding var selection: String?
	let field: RuleField?
	let names: BudgetNames

	var body: some View {
		Picker(RuleWords.field(field ?? .payee), selection: $selection) {
			Text("Choose…").tag(String?.none)
			Divider()
			switch field {
			case .account:
				ForEach(names.accounts, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
			case .category:
				ForEach(names.categoryGroups, id: \.id) { group in
					Section(group.name) {
						ForEach(group.envelopes, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
					}
				}
			case .categoryGroup:
				ForEach(names.categoryGroups, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
			default:
				ForEach(names.payees, id: \.id) { Text(names.payeeLabel($0)).tag(String?.some($0.id)) }
			}
		}
		.labelsHidden()
		.frame(maxWidth: 240, alignment: .leading)
	}
}

/// `one of` on payees, accounts, categories or groups: the chosen names, and a menu to add more.
private struct IdList: View {
	@Binding var ids: [String]
	let field: RuleField?
	let names: BudgetNames

	private var choices: [(id: String, name: String)] {
		switch field {
		case .account: names.accounts.map { ($0.id, $0.name) }
		case .category: names.categoryGroups.flatMap(\.envelopes).map { ($0.id, $0.name) }
		case .categoryGroup: names.categoryGroups.map { ($0.id, $0.name) }
		default: names.payees.map { ($0.id, names.payeeLabel($0)) }
		}
	}

	var body: some View {
		FlowTokens(items: ids.map { id in (id, choices.first { $0.id == id }?.name ?? id) }) { id in
			ids.removeAll { $0 == id }
		} adder: {
			Menu("Add") {
				ForEach(choices.filter { !ids.contains($0.id) }, id: \.id) { choice in
					Button(choice.name) { ids.append(choice.id) }
				}
			}
			.fixedSize()
			.accessibilityIdentifier("ruleEditor.addValue")
		}
	}
}

/// `one of` on text: each entry stays whole, commas included.
private struct TextList: View {
	@Binding var texts: [String]
	@State private var typed = ""

	var body: some View {
		FlowTokens(items: texts.map { ($0, $0) }) { text in
			texts.removeAll { $0 == text }
		} adder: {
			TextField("Add", text: $typed)
				.textFieldStyle(.roundedBorder)
				.labelsHidden()
				.frame(width: 110)
				.onSubmit {
					let entry = typed.trimmingCharacters(in: .whitespaces)
					if !entry.isEmpty, !texts.contains(entry) { texts.append(entry) }
					typed = ""
				}
				.accessibilityIdentifier("ruleEditor.addValue")
		}
	}
}

private struct FlowTokens<Adder: View>: View {
	let items: [(id: String, name: String)]
	let remove: (String) -> Void
	@ViewBuilder let adder: Adder

	var body: some View {
		WrappingRow(spacing: Spacing.s1) {
			ForEach(items, id: \.id) { item in
				Button {
					remove(item.id)
				} label: {
					HStack(spacing: 2) {
						Text(item.name).lineLimit(1)
						Image(systemName: "xmark").imageScale(.small).foregroundStyle(.textSecondary)
					}
				}
				.buttonStyle(.bordered)
				.controlSize(.small)
				.fixedSize()
				.accessibilityLabel("Remove \(item.name)")
			}
			adder
		}
	}
}

/// Left to right, starting a new line when the next item doesn't fit.
private struct WrappingRow: Layout {
	let spacing: CGFloat

	func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
		let frames = place(subviews, width: proposal.width ?? .infinity)
		let width = frames.map(\.maxX).max() ?? 0
		return CGSize(width: proposal.width.map { min($0, width) } ?? width, height: frames.map(\.maxY).max() ?? 0)
	}

	func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
		for (subview, frame) in zip(subviews, place(subviews, width: bounds.width)) {
			subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
		}
	}

	private func place(_ subviews: Subviews, width: CGFloat) -> [CGRect] {
		var frames: [CGRect] = []
		var origin = CGPoint.zero
		var lineHeight: CGFloat = 0
		for subview in subviews {
			let size = subview.sizeThatFits(.unspecified)
			if origin.x > 0, origin.x + size.width > width {
				origin = CGPoint(x: 0, y: origin.y + lineHeight + spacing)
				lineHeight = 0
			}
			frames.append(CGRect(origin: origin, size: CGSize(width: min(size.width, width), height: size.height)))
			origin.x += size.width + spacing
			lineHeight = max(lineHeight, size.height)
		}
		return frames
	}
}
