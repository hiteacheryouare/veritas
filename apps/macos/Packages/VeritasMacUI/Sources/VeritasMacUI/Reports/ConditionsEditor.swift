import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Actual's filters for reports and widgets: field, operator and value,
/// matched all together or any one (contract v14's `[RuleCondition]`).
struct ConditionsEditor: View {
	@Binding var conditions: [RuleCondition]
	@Binding var matchAny: Bool
	let names: BudgetNames
	var style: NumberStyle = .commaDot

	var body: some View {
		if conditions.count > 1 {
			Picker("Match", selection: $matchAny) {
				Text("All filters").tag(false)
				Text("Any filter").tag(true)
			}
			.pickerStyle(.segmented)
			.accessibilityIdentifier("filters.match")
		}
		ForEach(Array(conditions.indices), id: \.self) { index in
			let condition = conditions[index]
			if FilterChoices.isEditable(condition) {
				// The value gets its own full-width line: beside two pickers it had no room in the 340pt report editor.
				VStack(alignment: .leading, spacing: Spacing.s1) {
					HStack(spacing: Spacing.s2) {
						Picker("Field", selection: Binding(get: { condition.field }, set: { set(index, FilterChoices.setting($0, of: condition, today: WorkspaceModel.today())) })) {
							ForEach(FilterChoices.all, id: \.field) { Text(FilterWords.field($0.field)).tag($0.field) }
						}
						.labelsHidden()
						.frame(width: 130)
						Picker("Operator", selection: Binding(get: { condition.op }, set: { set(index, FilterChoices.setting($0, of: condition, today: WorkspaceModel.today())) })) {
							ForEach(FilterChoices.choice(for: condition.field)?.ops ?? [], id: \.op) { Text(FilterWords.op($0.op, on: condition.field)).tag($0.op) }
						}
						.labelsHidden()
						.frame(width: 130)
						Spacer(minLength: 0)
						removeButton(index)
					}
					FilterValueEditor(
						value: Binding(get: { conditions[safe: index]?.value ?? .nothing }, set: { value in if conditions.indices.contains(index) { conditions[index].value = value } }),
						field: condition.field,
						kind: FilterChoices.choice(for: condition.field)?.valueKind(for: condition.op),
						names: names,
						style: style
					)
					.id("\(index).\(condition.field).\(condition.op)")
				}
			} else {
				HStack(spacing: Spacing.s2) {
					Text(FilterWords.stored(condition))
						.foregroundStyle(.textSecondary)
						.help("Change this filter in Actual.")
					Spacer(minLength: 0)
					removeButton(index)
				}
			}
		}
		Button {
			conditions.append(FilterChoices.starter)
		} label: {
			Label("Add filter", systemImage: "plus")
		}
		.buttonStyle(.borderless)
		.accessibilityIdentifier("filters.add")
	}

	private func set(_ index: Int, _ condition: RuleCondition) {
		guard conditions.indices.contains(index) else { return }
		conditions[index] = condition
	}

	private func removeButton(_ index: Int) -> some View {
		Button {
			if conditions.indices.contains(index) { conditions.remove(at: index) }
		} label: {
			Image(systemName: "minus.circle")
		}
		.buttonStyle(.borderless)
		.accessibilityLabel("Remove filter")
	}
}

/// The value part of a filter, chosen by the kind of value its op takes.
private struct FilterValueEditor: View {
	@Binding var value: RuleValue
	let field: RuleField
	let kind: RuleValueKind?
	let names: BudgetNames
	let style: NumberStyle

	var body: some View {
		switch value {
		case .id(let id):
			IdPicker(selection: Binding(get: { id }, set: { value = .id(id: $0) }), field: field, names: names)
		case .ids(let ids):
			IdList(ids: Binding(get: { ids }, set: { value = .ids(ids: $0) }), field: field, names: names)
		case .text(let text):
			TextField("Value", text: Binding(get: { text }, set: { value = .text(text: $0) }), prompt: Text("Value"))
				.labelsHidden()
				.frame(maxWidth: .infinity)
				.accessibilityIdentifier("filters.value")
		case .texts(let texts):
			TextList(texts: Binding(get: { texts }, set: { value = .texts(texts: $0) }))
		case .amount(let amount):
			FilterAmountField(amount: amount, style: style) { value = .amount(amount: $0) }
		case .date, .month, .year:
			DateValue(value: $value, allowsMonthOrYear: kind == .dateMonthOrYear)
		case .bool(let flag):
			Picker("Value", selection: Binding(get: { flag }, set: { value = .bool(value: $0) })) {
				Text("true").tag(true)
				Text("false").tag(false)
			}
			.labelsHidden()
			.fixedSize()
		case .amountRange, .percent, .nothing, .raw:
			EmptyView()
		}
	}
}

/// An amount on the register's scale, read through the core as it's typed.
private struct FilterAmountField: View {
	@State private var field: AmountFieldModel
	let write: (Int64) -> Void

	init(amount: Int64, style: NumberStyle, write: @escaping (Int64) -> Void) {
		_field = State(initialValue: AmountFieldModel(amount: amount, style: style, reader: CoreAmountReader()))
		self.write = write
	}

	var body: some View {
		AmountTextField(field: field, takesFocus: false) { _ in }
			.frame(width: 130, height: 24)
			.task(id: field.text) {
				guard !field.isUntouched, let amount = await field.commit(), !Task.isCancelled else { return }
				write(amount)
			}
	}
}

/// A date filter. `is` on a date can also name a whole month or year, as
/// Actual's filter editor does.
private struct DateValue: View {
	@Binding var value: RuleValue
	let allowsMonthOrYear: Bool

	private enum Unit: Hashable {
		case day, month, year
	}

	private var unit: Unit {
		switch value {
		case .month: .month
		case .year: .year
		default: .day
		}
	}

	private var thisMonth: Month { Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1) }

	var body: some View {
		HStack(spacing: Spacing.s2) {
			if allowsMonthOrYear {
				Picker("Unit", selection: Binding(get: { unit }, set: setUnit)) {
					Text("Day").tag(Unit.day)
					Text("Month").tag(Unit.month)
					Text("Year").tag(Unit.year)
				}
				.labelsHidden()
				.fixedSize()
			}
			switch value {
			case .date(let day):
				DatePicker("Date", selection: Binding(get: { Self.date(day) ?? .now }, set: { value = .date(date: WorkspaceModel.today($0)) }), displayedComponents: .date)
					.labelsHidden()
					.fixedSize()
			case .month(let month):
				Picker("Month", selection: Binding(get: { Month(date: month) ?? thisMonth }, set: { value = .month(month: String($0.firstDay.prefix(7))) })) {
					ForEach(ReportPeriod(first: Month(year: thisMonth.year - 10, month: 1), last: thisMonth.adding(months: 12)).months.reversed(), id: \.self) {
						Text(ReportPeriod.monthYear($0)).tag($0)
					}
				}
				.labelsHidden()
				.fixedSize()
			case .year(let year):
				Stepper(String(year), value: Binding(get: { Int(year) }, set: { value = .year(year: Int32($0)) }), in: 1900...2200)
					.fixedSize()
			default:
				EmptyView()
			}
		}
	}

	private func setUnit(_ unit: Unit) {
		let today = WorkspaceModel.today()
		switch unit {
		case .day: value = .date(date: today)
		case .month: value = .month(month: String(today.prefix(7)))
		case .year: value = .year(year: thisMonth.year)
		}
	}

	static func date(_ day: String) -> Date? {
		CivilDate.date(day)
	}
}

/// A payee, account, category or category group, from the budget's names.
private struct IdPicker: View {
	@Binding var selection: String?
	let field: RuleField
	let names: BudgetNames

	var body: some View {
		Picker(FilterWords.field(field), selection: $selection) {
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
				ForEach(names.filterPayees, id: \.id) { Text($0.name).tag(String?.some($0.id)) }
			}
		}
		.labelsHidden()
		.frame(maxWidth: 240, alignment: .leading)
		.accessibilityIdentifier("filters.choice")
	}
}

/// `is one of` on payees, accounts, categories or groups: the chosen names, and a menu to add more.
private struct IdList: View {
	@Binding var ids: [String]
	let field: RuleField
	let names: BudgetNames

	private var choices: [(id: String, name: String)] {
		switch field {
		case .account: names.accounts.map { ($0.id, $0.name) }
		case .category: names.categoryGroups.flatMap(\.envelopes).map { ($0.id, $0.name) }
		case .categoryGroup: names.categoryGroups.map { ($0.id, $0.name) }
		default: names.filterPayees.map { ($0.id, $0.name) }
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
			.accessibilityIdentifier("filters.addValue")
		}
	}
}

/// `is one of` on text: each entry stays whole, commas included.
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
				.accessibilityIdentifier("filters.addValue")
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

private extension BudgetNames {
	/// A filter picks real payees; an account's transfer payee isn't offered.
	var filterPayees: [PayeeDetail] { payees.filter { $0.transferAccountId == nil } }
}

