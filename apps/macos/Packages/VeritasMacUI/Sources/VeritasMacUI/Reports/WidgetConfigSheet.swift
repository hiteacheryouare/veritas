import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A widget's settings: what Actual keeps in its meta, per type.
struct WidgetConfigSheet: View {
	let card: DashboardCard
	let reports: [CustomReportItem]
	let save: (WidgetConfig) -> Void
	@State private var config: WidgetConfig
	@State private var months: Int
	@Environment(\.dismiss) private var dismiss

	init(card: DashboardCard, reports: [CustomReportItem], save: @escaping (WidgetConfig) -> Void) {
		self.card = card
		self.reports = reports
		self.save = save
		_config = State(initialValue: card.config)
		let frame = card.config.timeFrame
		_months = State(initialValue: frame.flatMap { frame in frame.startMonth.flatMap { start in frame.endMonth.map { start.months(through: $0) + 1 } } } ?? 6)
	}

	private var kind: WidgetKind { card.kind ?? .markdown }
	private var thisMonth: Month { Month(date: WorkspaceModel.today()) ?? Month(year: 2026, month: 1) }

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			Text(card.title.map { "Configure \($0)" } ?? "Configure \(kind.title.lowercased())")
				.font(.text(.title, weight: .semibold))
				.padding([.horizontal, .top], Spacing.s5)
			Form {
				Section {
					TextField("Name", text: Binding(get: { config.name ?? "" }, set: { config.name = $0.isEmpty ? nil : $0 }), prompt: Text(kind.title))
						.accessibilityIdentifier("widgetConfig.name")
					if !kind.modes.isEmpty {
						Picker("Show", selection: Binding(get: { config.mode ?? kind.modes[0].id }, set: { config.mode = $0 })) {
							ForEach(kind.modes, id: \.id) { Text($0.title).tag($0.id) }
						}
					}
					if kind == .summary {
						Picker("Figure", selection: $config.summaryKind) {
							ForEach(SummaryKind.allCases, id: \.self) { Text($0.title).tag($0) }
						}
						.accessibilityIdentifier("widgetConfig.summary")
					}
					if kind == .formula {
						TextField("Formula", text: Binding(get: { config.formula ?? "" }, set: { config.formula = $0 }), prompt: Text("=1-(SPENT/INCOME)"))
							.font(.mono(.body))
					}
					if kind == .customReport {
						Picker("Report", selection: Binding(get: { config.reportId ?? "" }, set: { config.reportId = $0 })) {
							ForEach(reports) { Text($0.name).tag($0.id) }
						}
					}
				}
				if kind == .markdown {
					Section("Text") {
						TextEditor(text: Binding(get: { config.content ?? "" }, set: { config.content = $0 }))
							.font(.mono(.body))
							.frame(minHeight: 140)
							.accessibilityIdentifier("widgetConfig.text")
						Picker("Align", selection: Binding(get: { config.textAlign ?? "left" }, set: { config.textAlign = $0 })) {
							Text("Left").tag("left")
							Text("Center").tag("center")
							Text("Right").tag("right")
						}
						.pickerStyle(.segmented)
					}
				}
				if kind.hasTimeFrame {
					Section("Range") {
						Picker("Range", selection: Binding(get: { config.timeFrame?.frameMode ?? .slidingWindow }, set: setMode)) {
							ForEach(TimeFrameMode.allCases, id: \.self) { Text($0.title).tag($0) }
						}
						.accessibilityIdentifier("widgetConfig.range")
						switch config.timeFrame?.frameMode ?? .slidingWindow {
						case .slidingWindow:
							Stepper(months == 1 ? "1 month" : "\(months) months", value: $months, in: 1...60)
								.onChange(of: months) { setMode(.slidingWindow) }
						case .fixed, .full:
							MonthChoice(title: "From", month: Binding(get: { config.timeFrame?.startMonth ?? thisMonth }, set: { config.timeFrame?.start = String($0.firstDay.prefix(7)) }), thisMonth: thisMonth)
							if config.timeFrame?.frameMode == .fixed {
								MonthChoice(title: "To", month: Binding(get: { config.timeFrame?.endMonth ?? thisMonth }, set: { config.timeFrame?.end = String($0.firstDay.prefix(7)) }), thisMonth: thisMonth)
							}
						default:
							EmptyView()
						}
					}
				}
				if kind.hasFilters {
					Section("Filters") {
						ConditionsEditor(conditions: $config.conditions, matchAny: $config.conditionsOr)
					}
				}
			}
			.formStyle(.grouped)
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Save") {
					save(config)
					dismiss()
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.accessibilityIdentifier("widgetConfig.save")
			}
			.padding(Spacing.s5)
		}
		.frame(width: 520, height: 600)
		.containerIdentifier("sheet.widgetConfig")
	}

	private func setMode(_ mode: TimeFrameMode) {
		let end = mode == .fixed ? (config.timeFrame?.endMonth ?? thisMonth) : thisMonth
		let start = mode == .slidingWindow ? thisMonth.adding(months: -(months - 1)) : (config.timeFrame?.startMonth ?? thisMonth.adding(months: -5))
		config.timeFrame = WidgetTimeFrame(mode: mode, start: start, end: end)
	}
}

private struct MonthChoice: View {
	let title: String
	@Binding var month: Month
	let thisMonth: Month

	var body: some View {
		Picker(title, selection: $month) {
			ForEach(ReportPeriod(first: Month(year: thisMonth.year - 10, month: 1), last: thisMonth).months.reversed(), id: \.self) {
				Text(ReportPeriod.monthYear($0)).tag($0)
			}
		}
	}
}

/// Actual's filters: field, operator and value, matched all together or any one.
struct ConditionsEditor: View {
	@Binding var conditions: [ReportCondition]
	@Binding var matchAny: Bool

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
			let condition = $conditions[index]
			HStack(spacing: Spacing.s2) {
				Picker("Field", selection: condition.field) {
					ForEach(ReportCondition.fields, id: \.id) { Text($0.title).tag($0.id) }
				}
				.labelsHidden()
				.frame(width: 110)
				Picker("Operator", selection: condition.op) {
					ForEach(ReportCondition.operators(for: condition.wrappedValue.field), id: \.id) { Text($0.title).tag($0.id) }
				}
				.labelsHidden()
				.frame(width: 130)
				TextField("Value", text: condition.text, prompt: Text("Value"))
					.labelsHidden()
					.accessibilityIdentifier("filters.value")
				Button {
					conditions.remove(at: index)
				} label: {
					Image(systemName: "minus.circle")
				}
				.buttonStyle(.borderless)
				.accessibilityLabel("Remove filter")
			}
			.onChange(of: condition.wrappedValue.field) {
				if !ReportCondition.operators(for: condition.wrappedValue.field).contains(where: { $0.id == condition.wrappedValue.op }) { condition.wrappedValue.op = "is" }
			}
		}
		Button {
			conditions.append(ReportCondition(field: "notes", op: "contains", value: "\"\""))
		} label: {
			Label("Add filter", systemImage: "plus")
		}
		.buttonStyle(.borderless)
		.accessibilityIdentifier("filters.add")
	}
}
