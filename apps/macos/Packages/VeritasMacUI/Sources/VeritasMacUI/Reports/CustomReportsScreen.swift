import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The standard reports and the custom reports saved in Actual (R-REP-4,
/// R-REP-5). A custom report can be edited here and is saved back in
/// Actual's format.
struct CustomReportsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: CustomReportsModel
	@State private var drilldown: DrilldownModel?
	@State private var isChoosingRange = false

	init(workspace: WorkspaceModel, model: CustomReportsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? CustomReportsModel(workspace: workspace))
	}

	var body: some View {
		HStack(spacing: 0) {
			ReportList(model: model)
				.frame(width: 230)
			Divider()
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s4) {
					switch model.isCreating ? .custom(id: "") : model.choice {
					case .standard:
						StandardReportView(model: model.standard) { drilldown = $0 }
					case .custom:
						if let view = model.shown {
							CustomReportDetail(view: view, format: model.format)
						} else {
							ProgressView().frame(maxWidth: .infinity, minHeight: 200)
						}
					}
				}
				.padding(.top, Spacing.s2)
				.padding(.horizontal, Spacing.s5)
				.padding(.bottom, Spacing.s6)
				.frame(maxWidth: .infinity, alignment: .leading)
			}
			if let draft = model.draft {
				Divider()
				CustomReportEditor(model: model, draft: draft)
					.frame(width: 340)
			}
		}
		.background(.surfaceWindow)
		.navigationTitle(title)
		.toolbar { toolbar }
		.sheet(item: $drilldown) { DrilldownSheet(model: $0) }
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.customReports")
	}

	private var title: String {
		switch model.choice {
		case .standard(let kind): kind.title
		case .custom(let id): model.isCreating ? "New report" : (model.items.first { $0.id == id }?.name ?? "Custom reports")
		}
	}

	@ToolbarContentBuilder private var toolbar: some ToolbarContent {
		switch model.choice {
		case .standard:
			let standard = model.standard
			ToolbarItem(placement: .navigation) {
				PeriodStepper(canGoForward: standard.canGoForward, previous: { Task { await standard.previous() } }, next: { Task { await standard.next() } }, id: "standard")
			}
			ToolbarItem(placement: .primaryAction) {
				SpanPicker(selection: standard.span, choices: MonthSpan.allCases, title: \.title, id: "standard.range") { span in
					if span == .custom {
						isChoosingRange = true
					} else {
						Task { await standard.choose(span) }
					}
				}
				.popover(isPresented: $isChoosingRange, arrowEdge: .bottom) {
					MonthRangePicker(period: standard.period, thisMonth: standard.thisMonth) { first, last in
						Task { await standard.setCustom(first: first, last: last) }
					}
				}
			}
		case .custom:
			ToolbarItem(placement: .primaryAction) {
				Button {
					Task { await model.edit() }
				} label: {
					Label("Edit report", systemImage: "slider.horizontal.3")
				}
				.labelStyle(.titleAndIcon)
				.disabled(model.isEditing || model.isReadOnly || model.view?.settings == nil)
				.accessibilityIdentifier("customReport.edit")
			}
		}
		if model.canCreate {
			ToolbarItem(placement: .primaryAction) {
				Button {
					Task { await model.startNew() }
				} label: {
					Label("New report", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(model.isEditing)
				.accessibilityIdentifier("customReport.new")
			}
		}
	}
}

private struct ReportList: View {
	let model: CustomReportsModel

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: 2) {
				heading("Standard")
				ForEach(StandardReport.allCases, id: \.self) { kind in
					row(kind.title, .standard(kind), id: "reports.standard.\(kind.rawValue)")
				}
				heading("Saved in Actual").padding(.top, Spacing.s4)
				ForEach(model.items) { item in
					row(item.name, .custom(id: item.id), id: "reports.custom.\(item.id)")
				}
				if model.items.isEmpty {
					Text("No custom reports").foregroundStyle(.textSecondary).padding(.horizontal, Spacing.s3)
				}
			}
			.padding(Spacing.s3)
		}
		.background(.surfaceSidebar)
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("reports.list")
	}

	private func heading(_ text: String) -> some View {
		Text(text)
			.font(.text(.caption, weight: .semibold))
			.foregroundStyle(.textTertiary)
			.padding(.horizontal, Spacing.s3)
			.padding(.bottom, Spacing.s1)
			.accessibilityAddTraits(.isHeader)
	}

	private func row(_ title: String, _ choice: ReportChoice, id: String) -> some View {
		let isCurrent = model.choice == choice
		return Button {
			Task { await model.show(choice) }
		} label: {
			Text(title)
				.lineLimit(1)
				.frame(maxWidth: .infinity, alignment: .leading)
				.padding(.horizontal, Spacing.s3)
				.frame(height: 28)
				.background(isCurrent ? Color.accentSoft : .clear, in: .rect(cornerRadius: Radius.control))
				.foregroundStyle(isCurrent ? Color.accent : Color.textPrimary)
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.accessibilityAddTraits(isCurrent ? .isSelected : [])
		.accessibilityIdentifier(id)
	}
}

/// A custom report drawn the way its settings say: a graph, then the table.
struct CustomReportDetail: View {
	let view: CustomReportView
	let format: MoneyFormat
	var compact = false

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			if let data = view.data {
				if !compact {
					Hero(amount: data.total, caption: Self.range(view.settings), format: format)
				}
				if compact {
					CustomReportChart(data: data, settings: view.settings, format: format)
				} else {
					Card {
						CustomReportChart(data: data, settings: view.settings, format: format)
							.frame(height: 260)
					}
				}
				if !compact {
					CustomReportTable(data: data, settings: view.settings, format: format)
				}
			} else if let problem = view.problem {
				Text(problem)
					.foregroundStyle(.textSecondary)
					.frame(maxWidth: .infinity, minHeight: compact ? 40 : 160)
					.accessibilityIdentifier("customReport.problem")
			}
		}
		.containerIdentifier("customReport.detail")
	}

	static func range(_ settings: CustomReportSettings) -> String {
		if !settings.dateStatic, let name = settings.dateRange { return name }
		return "\(settings.startDate) to \(settings.endDate)"
	}
}

struct CustomReportChart: View {
	let data: CustomReportData
	let settings: CustomReportSettings
	let format: MoneyFormat

	private var graph: ReportGraph { ReportGraph(settings.graphType) }

	/// Payments are stored negative; the graph draws them upward, as money spent.
	private func plotted(_ amount: Int64) -> Double {
		let spent = settings.balanceType == .payment || settings.balanceType == .netPayment
		return Double(spent ? -amount : amount) / 100
	}

	private struct Point: Identifiable {
		let id: String
		let interval: String
		let group: String
		let amount: Double
	}

	private var points: [Point] {
		data.intervals.flatMap { interval in
			interval.stacked.map { stack in
				let name = data.groups.first { $0.id == stack.key || $0.name == stack.key }?.name ?? stack.key
				return Point(id: "\(interval.interval.interval).\(stack.key)", interval: ReportPeriod.interval(interval.interval.interval), group: name, amount: plotted(stack.amount))
			}
		}
	}

	var body: some View {
		if settings.totalMode || graph.drawsTotals {
			switch graph {
			case .donut:
				Chart(data.groups) { group in
					SectorMark(angle: .value("Amount", abs(Double(group.amount))), innerRadius: .ratio(0.6), angularInset: 1)
						.foregroundStyle(by: .value("Group", group.name))
				}
			case .table:
				EmptyView()
			default:
				Chart(data.groups) { group in
					BarMark(x: .value("Group", group.name), y: .value("Amount", plotted(group.amount)))
						.foregroundStyle(Color.chartExpense)
						.clipShape(.rect(cornerRadius: 3))
				}
			}
		} else {
			Chart(points) { point in
				switch graph {
				case .line:
					LineMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.group))
				case .area:
					AreaMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.group))
				default:
					BarMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.group))
				}
			}
		}
	}
}

private struct CustomReportTable: View {
	let data: CustomReportData
	let settings: CustomReportSettings
	let format: MoneyFormat

	var body: some View {
		let intervals = settings.totalMode ? [] : data.intervals.map(\.interval.interval)
		Grid(alignment: .trailing, horizontalSpacing: 0, verticalSpacing: 0) {
			GridRow {
				Text(settings.groupBy.title).gridColumnAlignment(.leading)
					.font(.text(.caption)).foregroundStyle(.textTertiary).padding(.leading, Spacing.s4)
				ForEach(intervals, id: \.self) { Text(ReportPeriod.interval($0)).font(.text(.caption)).foregroundStyle(.textTertiary).padding(.horizontal, Spacing.s3) }
				Text("Total").font(.text(.caption)).foregroundStyle(.textTertiary).padding(.horizontal, Spacing.s4)
			}
			.frame(height: 30)
			ForEach(data.groups) { group in
				Divider()
				GridRow {
					Text(group.name).lineLimit(1).fixedSize().padding(.leading, Spacing.s4).padding(.trailing, Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
					if !settings.totalMode {
						ForEach(Array(group.intervals.enumerated()), id: \.offset) { _, cell in
							ReportAmount(amount: cell.amount, format: format).padding(.horizontal, Spacing.s3)
						}
					}
					ReportAmount(amount: group.amount, format: format, weight: .semibold).padding(.horizontal, Spacing.s4)
				}
				.frame(minHeight: 34)
			}
			Divider().overlay(Color.textTertiary)
			GridRow {
				Text("Total").font(.text(.body, weight: .bold)).frame(maxWidth: .infinity, alignment: .leading).padding(.leading, Spacing.s4)
				if !settings.totalMode {
					ForEach(Array(data.intervals.enumerated()), id: \.offset) { _, interval in
						ReportAmount(amount: interval.interval.amount, format: format, weight: .bold).padding(.horizontal, Spacing.s3)
					}
				}
				ReportAmount(amount: data.total, format: format, weight: .bold).padding(.horizontal, Spacing.s4)
			}
			.frame(minHeight: 36)
		}
		.padding(.vertical, Spacing.s1)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("customReport.table")
	}
}

/// Actual's report settings, previewed as they change. Filters stay as
/// Actual wrote them.
private struct CustomReportEditor: View {
	let model: CustomReportsModel
	@State private var draft: ReportDraft
	@State private var fixedFirst: Month
	@State private var fixedLast: Month

	init(model: CustomReportsModel, draft: ReportDraft) {
		self.model = model
		_draft = State(initialValue: draft)
		_fixedFirst = State(initialValue: Month(date: draft.settings.startDate) ?? Month(year: 2026, month: 1))
		_fixedLast = State(initialValue: Month(date: draft.settings.endDate) ?? Month(year: 2026, month: 1))
	}

	var body: some View {
		Form {
			Section {
				TextField("Name", text: $draft.settings.name)
					.accessibilityIdentifier("customReport.name")
				Picker("Show", selection: $draft.settings.totalMode) {
					Text("Totals").tag(true)
					Text("Over time").tag(false)
				}
				.accessibilityIdentifier("customReport.mode")
				Picker("Group by", selection: $draft.settings.groupBy) {
					ForEach([ReportGrouping.category, .group, .payee, .account, .interval], id: \.self) { Text($0.title).tag($0) }
				}
				.accessibilityIdentifier("customReport.groupBy")
				Picker("Balance", selection: $draft.settings.balanceType) {
					ForEach([ReportBalanceKind.payment, .deposit, .net, .netPayment, .netDeposit], id: \.self) { Text($0.title).tag($0) }
				}
				.accessibilityIdentifier("customReport.balance")
				Picker("Interval", selection: $draft.settings.interval) {
					ForEach([ReportIntervalUnit.daily, .weekly, .monthly, .yearly], id: \.self) { Text($0.title).tag($0) }
				}
				Picker("Sort", selection: $draft.settings.sortBy) {
					ForEach([ReportSortOrder.descending, .ascending, .name, .budget], id: \.self) { Text($0.title).tag($0) }
				}
				Picker("Graph", selection: Binding(get: { ReportGraph(draft.settings.graphType) }, set: { draft.settings.graphType = $0.rawValue })) {
					ForEach(ReportGraph.allCases.filter { draft.settings.totalMode ? $0.drawsTotals : true }, id: \.self) { Text($0.title).tag($0) }
				}
				.accessibilityIdentifier("customReport.graph")
			}
			Section("Dates") {
				Picker("Range", selection: Binding(get: { draft.settings.dateStatic ? "" : (draft.settings.dateRange ?? "") }, set: { name in
					if name.isEmpty {
						model.setFixedRange(first: fixedFirst, last: fixedLast)
					} else {
						model.setLiveRange(name)
					}
					draft = model.draft ?? draft
				})) {
					ForEach(ReportDateRanges.names, id: \.self) { Text($0).tag($0) }
					Text("Fixed dates").tag("")
				}
				.accessibilityIdentifier("customReport.range")
				if draft.settings.dateStatic {
					MonthRow(title: "From", month: $fixedFirst) { applyFixed() }
					MonthRow(title: "To", month: $fixedLast) { applyFixed() }
				}
			}
			Section("Options") {
				Toggle("Show empty rows", isOn: $draft.settings.showEmpty)
				Toggle("Show off-budget accounts", isOn: $draft.settings.showOffBudget)
				Toggle("Show hidden categories", isOn: $draft.settings.showHidden)
				Toggle("Show uncategorized", isOn: $draft.settings.showUncategorized)
				Toggle("Include the current interval", isOn: $draft.settings.includeCurrent)
				Toggle("Trim empty intervals", isOn: $draft.settings.trimIntervals)
			}
			Section("Filters") {
				if model.canEditFilters {
					ConditionsEditor(conditions: $draft.conditions, matchAny: $draft.conditionsOr)
				} else if draft.settings.conditionCount > 0 {
					Text(draft.settings.conditionCount == 1 ? "1 filter" : "\(draft.settings.conditionCount) filters")
						.foregroundStyle(.textSecondary)
				} else {
					Text("No filters").foregroundStyle(.textSecondary)
				}
			}
			Section {
				HStack {
					Button("Cancel", role: .cancel) { model.cancelEditing() }
						.keyboardShortcut(.cancelAction)
					Spacer()
					Button(model.isCreating ? "Create report" : "Save report") { Task { await model.save() } }
						.keyboardShortcut(.defaultAction)
						.disabled(draft.settings.name.trimmingCharacters(in: .whitespaces).isEmpty)
						.accessibilityIdentifier("customReport.save")
				}
			}
		}
		.formStyle(.grouped)
		.onChange(of: draft) {
			guard model.draft != nil else { return }
			model.draft = draft
			Task { await model.refreshPreview() }
		}
		.containerIdentifier("customReport.editor")
	}

	private func applyFixed() {
		model.draft = draft
		model.setFixedRange(first: fixedFirst, last: fixedLast)
		draft = model.draft ?? draft
	}
}

private struct MonthRow: View {
	let title: String
	@Binding var month: Month
	let changed: () -> Void

	private var choices: [Month] {
		let now = Month(date: WorkspaceModel.today()) ?? month
		return ReportPeriod(first: Month(year: now.year - 5, month: 1), last: now).months.reversed()
	}

	var body: some View {
		Picker(title, selection: $month) {
			ForEach(choices, id: \.self) { Text(ReportPeriod.monthYear($0)).tag($0) }
		}
		.onChange(of: month) { changed() }
	}
}
