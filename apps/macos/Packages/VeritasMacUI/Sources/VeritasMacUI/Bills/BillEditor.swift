import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// R-BILL-1 and R-BILL-2: one bill's payee, money, schedule and posting.
/// Fields edit a copy; Save hands it to the model, which signs the amounts
/// and shows the core's `Invalid` message here.
struct BillEditor: View {
	let model: BillsModel
	@State private var editing: BillEditing
	@State private var amountKind: BillAmountKind
	@State private var low: AmountFieldModel
	@State private var high: AmountFieldModel
	@FocusState private var isNameFocused: Bool

	static let weekdays: [BillWeekday] = [.sunday, .monday, .tuesday, .wednesday, .thursday, .friday, .saturday]

	init(model: BillsModel, editing: BillEditing) {
		self.model = model
		_editing = State(initialValue: editing)
		let amount = editing.draft.amount.magnitudes
		_amountKind = State(initialValue: amount.kind)
		_low = State(initialValue: AmountFieldModel(amount: amount.low, style: model.numberStyle, reader: model.reader))
		_high = State(initialValue: AmountFieldModel(amount: amount.high, style: model.numberStyle, reader: model.reader))
	}

	private var title: String {
		if editing.isNew { return editing.isIncome ? "New income" : "New bill" }
		return editing.id.flatMap(model.bill)?.name ?? "Edit bill"
	}

	private var customRule: Bool {
		editing.detail?.bill.customRule ?? editing.id.flatMap(model.bill)?.customRule ?? false
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text(title)
				.font(.text(.title, weight: .semibold))
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s4) {
					who
					money
					schedule
					if let next = editing.detail?.nextDates, !next.isEmpty {
						GroupedRows {
							LabeledRow("Next") {
								Text(next.map(BillWords.day).joined(separator: ", "))
									.font(.figures(.body))
									.foregroundStyle(.textSecondary)
									.accessibilityIdentifier("billEditor.nextDates")
							}
						}
					}
				}
			}
			.scrollBounceBehavior(.basedOnSize)
			if customRule {
				Text("Saving keeps the extra conditions and actions on this bill's rule.")
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
					.accessibilityIdentifier("billEditor.customRule")
			}
			if let problem = model.editing?.problem ?? low.problem ?? high.problem {
				ProblemNote(text: problem)
					.accessibilityIdentifier("billEditor.problem")
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { model.cancelEditing() }
					.keyboardShortcut(.cancelAction)
					.accessibilityIdentifier("billEditor.cancel")
				Button(editing.isNew ? "Create" : "Save", action: save)
					.buttonStyle(.borderedProminent)
					.keyboardShortcut(.defaultAction)
					.disabled(model.isReadOnly)
					.accessibilityIdentifier("billEditor.save")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 580, height: 760)
		.defaultFocus($isNameFocused, editing.isNew)
		.containerIdentifier("sheet.billEditor")
	}

	// MARK: Who

	private var who: some View {
		GroupedRows {
			LabeledRow("Name") {
				TextField("Name", text: Binding(get: { editing.draft.name ?? "" }, set: { editing.draft.name = $0 }))
					.textFieldStyle(.roundedBorder)
					.labelsHidden()
					.frame(width: 260)
					.focused($isNameFocused)
					.accessibilityIdentifier("billEditor.name")
			}
			divider
			LabeledRow("Type") {
				Picker("Type", selection: $editing.isIncome) {
					Text("Bill").tag(false)
					Text("Income").tag(true)
				}
				.pickerStyle(.segmented)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("billEditor.type")
			}
			divider
			LabeledRow("Payee") {
				Picker("Payee", selection: $editing.draft.payeeId) {
					Text("None").tag(String?.none)
					ForEach(model.names.payees, id: \.id) { payee in
						Text(payee.name).tag(String?.some(payee.id))
					}
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("billEditor.payee")
			}
			divider
			LabeledRow("Account") {
				Picker("Account", selection: $editing.draft.accountId) {
					Text("None").tag(String?.none)
					ForEach(accounts, id: \.id) { account in
						Text(account.name).tag(String?.some(account.id))
					}
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("billEditor.account")
			}
			divider
			LabeledRow("Category") {
				Picker("Category", selection: $editing.draft.categoryId) {
					Text("None").tag(String?.none)
					ForEach(model.names.categoryGroups, id: \.id) { group in
						Section(group.name) {
							ForEach(group.envelopes, id: \.id) { envelope in
								Text(envelope.name).tag(String?.some(envelope.id))
							}
						}
					}
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("billEditor.category")
			}
		}
	}

	/// Open accounts, plus the bill's own account if it has since closed.
	private var accounts: [AccountDetail] {
		let open = model.names.openAccounts
		guard let id = editing.draft.accountId, !open.contains(where: { $0.id == id }),
			let current = model.names.accounts.first(where: { $0.id == id })
		else { return open }
		return open + [current]
	}

	// MARK: Money

	private var money: some View {
		GroupedRows {
			LabeledRow("Amount") {
				HStack(spacing: Spacing.s2) {
					Picker("Amount", selection: $amountKind) {
						Text("Exactly").tag(BillAmountKind.exact)
						Text("About").tag(BillAmountKind.about)
						Text("Between").tag(BillAmountKind.between)
					}
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("billEditor.amountKind")
					amountField(low, identifier: "billEditor.amount")
					if amountKind == .between {
						Text("and").foregroundStyle(.textSecondary)
						amountField(high, identifier: "billEditor.amountHigh")
					}
				}
			}
			divider
			LabeledRow("Posting") {
				Picker("Posting", selection: $editing.draft.autoPost) {
					Text("Auto-post").tag(true)
					Text("Confirm first").tag(false)
				}
				.pickerStyle(.segmented)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("billEditor.posting")
			}
			if editing.draft.autoPost && amountKind != .exact {
				divider
				Label {
					Text("Auto-post records the estimate as a real transaction.")
				} icon: {
					Image(systemName: "exclamationmark.triangle")
						.imageScale(.small)
				}
				.font(.text(.body))
				.foregroundStyle(.moneyLow)
				.padding(.horizontal, Spacing.s4)
				.frame(maxWidth: .infinity, minHeight: Size.row, alignment: .leading)
				.accessibilityIdentifier("billEditor.estimate")
			}
		}
	}

	private func amountField(_ field: AmountFieldModel, identifier: String) -> some View {
		AmountTextField(field: field, takesFocus: false) { exit in
			if exit == .cancel { model.cancelEditing() }
		}
		.frame(width: 120, height: Size.controlSmall)
		.accessibilityIdentifier(identifier)
	}

	// MARK: Schedule

	private var schedule: some View {
		GroupedRows {
			LabeledRow("Schedule") {
				HStack(spacing: Spacing.s3) {
					Text(BillWords.schedule(editing.draft.date))
						.foregroundStyle(.textSecondary)
						.lineLimit(1)
					Picker("Schedule", selection: $editing.repeats) {
						Text("Once").tag(false)
						Text("Repeats").tag(true)
					}
					.pickerStyle(.segmented)
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("billEditor.repeats")
				}
			}
			divider
			LabeledRow(editing.repeats ? "Starts" : "Date") {
				datePicker(Binding(get: { editing.firstDate }, set: { editing.firstDate = $0 }), identifier: "billEditor.date")
			}
			if let rule = editing.rule {
				divider
				LabeledRow("Every") {
					HStack(spacing: Spacing.s2) {
						countField(Binding(get: { rule.interval }, set: { editing.rule?.interval = $0 }), identifier: "billEditor.interval")
						Picker("Every", selection: Binding(get: { rule.frequency }, set: { setFrequency($0) })) {
							ForEach([BillFrequency.daily, .weekly, .monthly, .yearly], id: \.self) { frequency in
								Text(BillWords.unit(frequency, count: rule.interval)).tag(frequency)
							}
						}
						.labelsHidden()
						.fixedSize()
						.accessibilityIdentifier("billEditor.frequency")
					}
				}
				if rule.frequency == .monthly {
					divider
					patterns(rule)
				}
				divider
				LabeledRow("Weekends") {
					Picker("Weekends", selection: Binding(get: { rule.weekend }, set: { editing.rule?.weekend = $0 })) {
						Text("Keep the date").tag(BillWeekend?.none)
						Text("Friday before").tag(BillWeekend?.some(.fridayBefore))
						Text("Monday after").tag(BillWeekend?.some(.mondayAfter))
					}
					.labelsHidden()
					.fixedSize()
					.accessibilityIdentifier("billEditor.weekend")
				}
				divider
				LabeledRow("Ends") {
					ends(rule)
				}
			}
		}
	}

	private func setFrequency(_ frequency: BillFrequency) {
		editing.rule?.frequency = frequency
		if frequency != .monthly { editing.rule?.patterns = [] }
	}

	private func patterns(_ rule: BillRepeat) -> some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack {
				Text("On")
				Spacer(minLength: Spacing.s4)
				Button {
					editing.rule?.patterns.append(.day(day: Int8(rule.start.suffix(2)) ?? 1))
				} label: {
					Label("Add a day", systemImage: "plus")
				}
				.controlSize(.small)
				.accessibilityIdentifier("billEditor.addPattern")
			}
			ForEach(Array(rule.patterns.enumerated()), id: \.offset) { index, _ in
				PatternRow(pattern: Binding(
					get: { editing.rule?.patterns[safe: index] ?? .day(day: 1) },
					set: { editing.rule?.patterns[index] = $0 }
				)) {
					editing.rule?.patterns.remove(at: index)
				}
				.accessibilityIdentifier("billEditor.pattern.\(index)")
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s2)
		.frame(minHeight: Size.row)
	}

	private enum EndKind: Hashable {
		case never
		case after
		case onDate
	}

	private func ends(_ rule: BillRepeat) -> some View {
		let kind: EndKind = switch rule.end {
		case .never: .never
		case .afterOccurrences: .after
		case .onDate: .onDate
		}
		return HStack(spacing: Spacing.s2) {
			switch rule.end {
			case .afterOccurrences(let count):
				countField(Binding(get: { count }, set: { editing.rule?.end = .afterOccurrences(count: $0) }), identifier: "billEditor.endCount")
				Text(count == 1 ? "time" : "times").foregroundStyle(.textSecondary)
			case .onDate(let date):
				datePicker(Binding(get: { date }, set: { editing.rule?.end = .onDate(date: $0) }), identifier: "billEditor.endDate")
			case .never:
				EmptyView()
			}
			Picker("Ends", selection: Binding(get: { kind }, set: { setEnd($0, rule: rule) })) {
				Text("Never").tag(EndKind.never)
				Text("After").tag(EndKind.after)
				Text("On a date").tag(EndKind.onDate)
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("billEditor.end")
		}
	}

	private func setEnd(_ kind: EndKind, rule: BillRepeat) {
		switch kind {
		case .never: editing.rule?.end = .never
		case .after: editing.rule?.end = .afterOccurrences(count: 12)
		case .onDate: editing.rule?.end = .onDate(date: rule.start)
		}
	}

	// MARK: Parts

	private var divider: some View {
		Divider().padding(.leading, Spacing.s4)
	}

	private func datePicker(_ text: Binding<String>, identifier: String) -> some View {
		DatePicker(
			"Date",
			selection: Binding(get: { CivilDate.date(text.wrappedValue) ?? .now }, set: { text.wrappedValue = CivilDate.text($0) }),
			displayedComponents: .date
		)
		.labelsHidden()
		.fixedSize()
		.accessibilityIdentifier(identifier)
	}

	private func countField(_ value: Binding<UInt32>, identifier: String) -> some View {
		HStack(spacing: 2) {
			TextField("Count", value: value, format: .number)
				.textFieldStyle(.roundedBorder)
				.multilineTextAlignment(.trailing)
				.labelsHidden()
				.frame(width: 48)
				.accessibilityIdentifier(identifier)
			Stepper("Count", value: value, in: 1...999)
				.labelsHidden()
		}
	}

	private func save() {
		Task {
			guard let lowAmount = await low.commit() else { return }
			var highAmount = lowAmount
			if amountKind == .between {
				guard let amount = await high.commit() else { return }
				highAmount = amount
			}
			var saving = editing
			saving.draft.amount = BillAmount(kind: amountKind, low: lowAmount, high: highAmount)
			model.editing = saving
			await model.save()
		}
	}
}

/// One monthly day: a day of the month, the last day, or the nth weekday.
private struct PatternRow: View {
	@Binding var pattern: BillRepeatPattern
	let remove: () -> Void

	private enum Kind: Hashable {
		case day
		case lastDay
		case weekday
	}

	private var kind: Kind {
		switch pattern {
		case .day(let day): day == -1 ? .lastDay : .day
		case .weekday: .weekday
		}
	}

	var body: some View {
		HStack(spacing: Spacing.s2) {
			Picker("Kind", selection: Binding(get: { kind }, set: setKind)) {
				Text("Day").tag(Kind.day)
				Text("Last day").tag(Kind.lastDay)
				Text("Weekday").tag(Kind.weekday)
			}
			.labelsHidden()
			.fixedSize()
			switch pattern {
			case .day(let day) where day > 0:
				Picker("Day", selection: Binding(get: { day }, set: { pattern = .day(day: $0) })) {
					ForEach(Int8(1)...31, id: \.self) { day in
						Text(BillWords.ordinal(Int(day))).tag(day)
					}
				}
				.labelsHidden()
				.fixedSize()
			case .weekday(let weekday, let nth):
				Picker("Which", selection: Binding(get: { nth }, set: { pattern = .weekday(weekday: weekday, nth: $0) })) {
					ForEach([Int8(1), 2, 3, 4, 5], id: \.self) { nth in
						Text(BillWords.ordinal(Int(nth))).tag(nth)
					}
					Text("Last").tag(Int8(-1))
				}
				.labelsHidden()
				.fixedSize()
				Picker("Weekday", selection: Binding(get: { weekday }, set: { pattern = .weekday(weekday: $0, nth: nth) })) {
					ForEach(BillEditor.weekdays, id: \.self) { weekday in
						Text(BillWords.weekday(weekday)).tag(weekday)
					}
				}
				.labelsHidden()
				.fixedSize()
			default:
				EmptyView()
			}
			Spacer()
			Button(action: remove) {
				Image(systemName: "minus.circle")
			}
			.buttonStyle(.borderless)
			.accessibilityLabel("Remove")
		}
	}

	private func setKind(_ kind: Kind) {
		switch kind {
		case .day: pattern = .day(day: 1)
		case .lastDay: pattern = .day(day: -1)
		case .weekday: pattern = .weekday(weekday: .monday, nth: 1)
		}
	}
}

private extension Array {
	subscript(safe index: Int) -> Element? {
		indices.contains(index) ? self[index] : nil
	}
}
