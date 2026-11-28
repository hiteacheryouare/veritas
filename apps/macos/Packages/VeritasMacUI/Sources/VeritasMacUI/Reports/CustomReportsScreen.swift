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
			if !model.isEditing {
				ReportList(model: model)
					.frame(width: 230)
				Divider()
			}
			CustomReportPane(model: model) { drilldown = $0 }
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
			ToolbarItem(placement: .primaryAction) { CustomReportEditButton(model: model) }
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

/// The report page itself: the standard report or the custom report the
/// model shows, with the custom report's editor beside it while editing.
struct CustomReportPane: View {
	let model: CustomReportsModel
	let open: (DrilldownModel) -> Void

	var body: some View {
		HStack(spacing: 0) {
			ScrollView {
				VStack(alignment: .leading, spacing: Spacing.s4) {
					switch model.isCreating ? .custom(id: "") : model.choice {
					case .standard:
						StandardReportView(model: model.standard, open: open)
					case .custom:
						if let view = model.shown {
							CustomReportDetail(view: view, format: model.format)
						} else if model.isEditing, let problem = model.previewProblem {
							Text("Couldn't preview: \(problem)")
								.foregroundStyle(.textSecondary)
								.frame(maxWidth: .infinity, minHeight: 200)
								.accessibilityIdentifier("customReport.previewProblem")
						} else {
							ScreenState(kind: .loading).frame(minHeight: 200)
						}
					}
				}
				.motion(.fast, value: model.shown == nil)
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
	}
}

struct CustomReportEditButton: View {
	let model: CustomReportsModel

	var body: some View {
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

/// A custom report drawn the way its settings say: a graph, then the table.
struct CustomReportDetail: View {
	let view: CustomReportView
	let format: MoneyFormat
	var compact = false

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			if let data = view.data {
				if compact {
					CustomReportChart(data: data, settings: view.settings, format: format)
				} else {
					Hero(amount: data.total, caption: Self.range(view.settings), format: format)
					Card {
						if CustomReportChart.colorsGroups(view.settings) {
							let chart = CustomReportChart(data: data, settings: view.settings, format: format, expanded: true)
							let legend = ReportLegend(series: ReportSeries.all(data, balance: view.settings.balanceType))
							ViewThatFits(in: .horizontal) {
								HStack(alignment: .top, spacing: Spacing.s4) {
									chart.frame(minWidth: Self.chartMinimum)
									legend.frame(width: 200)
								}
								.frame(height: 300)
								VStack(alignment: .leading, spacing: Spacing.s3) {
									chart.frame(height: 220)
									legend.frame(height: 96)
								}
							}
						} else {
							CustomReportChart(data: data, settings: view.settings, format: format, expanded: true)
								.frame(height: 300)
						}
					}
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

	/// Narrower than this, the legend moves under the graph.
	static let chartMinimum: CGFloat = 320

	static func range(_ settings: CustomReportSettings) -> String {
		if !settings.dateStatic, let name = settings.dateRange { return name }
		return "\(settings.startDate) to \(settings.endDate)"
	}
}

/// The graph draws the report's groups only, under the core's names, each
/// colored by its place in the legend as Actual colors it. A card shows no
/// legend, as Actual's card shows none.
struct CustomReportChart: View {
	let data: CustomReportData
	let settings: CustomReportSettings
	let format: MoneyFormat
	var expanded = false

	private var graph: ReportGraph { ReportGraph(settings.graphType) }
	private var series: [ReportSeries] { ReportSeries.all(data, balance: settings.balanceType) }

	/// Whether color tells the groups apart, so the page needs a legend.
	static func colorsGroups(_ settings: CustomReportSettings) -> Bool {
		let graph = ReportGraph(settings.graphType)
		return graph == .donut || !(settings.totalMode || graph.drawsTotals)
	}

	private struct Point: Identifiable {
		let id: String
		let interval: String
		let series: String
		let amount: Double
	}

	private var points: [Point] {
		let keys = data.intervals.map(\.interval.interval)
		let headings = Dictionary(zip(keys, ReportPeriod.intervals(keys)), uniquingKeysWith: { first, _ in first })
		return series.flatMap { group in
			group.points.map { point in
				Point(id: "\(point.interval).\(group.id)", interval: headings[point.interval] ?? ReportPeriod.interval(point.interval), series: group.id, amount: format.chartValue(point.amount, on: .register))
			}
		}
	}

	var body: some View {
		chart
			.chartForegroundStyleScale(domain: series.map(\.id), range: series.map { CardPalette.mark($0.color) })
			.chartLegend(.hidden)
			.chartYAxis {
				if expanded {
					AxisMarks(position: .leading) { value in
						AxisGridLine().foregroundStyle(Color.chartGrid)
						AxisValueLabel {
							if let amount = value.as(Double.self) {
								Text(format.axisLabel(amount)).font(.figures(.caption)).foregroundStyle(.textTertiary)
							}
						}
					}
				}
			}
			.accessibilityIdentifier("customReport.chart")
	}

	@ViewBuilder private var chart: some View {
		if settings.totalMode || graph.drawsTotals {
			switch graph {
			case .donut:
				Chart(series) { group in
					SectorMark(angle: .value("Amount", abs(Double(group.total))), innerRadius: .ratio(0.6), angularInset: 1)
						.foregroundStyle(by: .value("Group", group.id))
				}
			case .table:
				EmptyView()
			default:
				Chart(series) { group in
					BarMark(x: .value("Group", group.name), y: .value("Amount", format.chartValue(group.total, on: .register)))
						.foregroundStyle(by: .value("Group", group.id))
						.clipShape(.rect(cornerRadius: 3))
				}
			}
		} else {
			Chart(points) { point in
				switch graph {
				case .line:
					LineMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.series))
				case .area:
					AreaMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.series))
				default:
					BarMark(x: .value("Interval", point.interval), y: .value("Amount", point.amount))
						.foregroundStyle(by: .value("Group", point.series))
				}
			}
		}
	}
}

/// Actual's legend beside the graph: every group with its color, scrolling
/// when there are more than fit.
struct ReportLegend: View {
	let series: [ReportSeries]

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s2) {
				ForEach(series) { group in
					HStack(spacing: Spacing.s2) {
						Circle().fill(CardPalette.color(group.color))
							.overlay { Circle().strokeBorder(CardPalette.edge(group.color), lineWidth: 1) }
							.frame(width: 10, height: 10)
						Text(group.name).lineLimit(1).truncationMode(.tail)
					}
					.font(.text(.caption))
					.help(group.name)
				}
			}
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.scrollIndicators(.automatic)
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("customReport.legend")
	}
}

private struct CustomReportTable: View {
	let data: CustomReportData
	let settings: CustomReportSettings
	let format: MoneyFormat

	var body: some View {
		let intervals = settings.totalMode ? [] : ReportPeriod.intervals(data.intervals.map(\.interval.interval))
		let headings = intervals + ["Total"]
		let groups = data.groups.map { group in
			line(id: group.id, title: group.name, height: 34, rule: .light, weight: .regular, headings: headings,
				amounts: (settings.totalMode ? [] : group.intervals.map(\.amount)) + [group.amount])
		}
		let total = line(id: "total", title: "Total", height: 36, rule: .strong, weight: .bold, headings: headings,
			amounts: (settings.totalMode ? [] : data.intervals.map(\.interval.amount)) + [data.total])
		PinnedTable(leadingHeading: settings.groupBy.title, headings: headings, lines: groups + [total])
			.accessibilityElement(children: .contain)
			.accessibilityIdentifier("customReport.table")
	}

	/// The last amount is the line's total, set heavier than the intervals.
	private func line(id: String, title: String, height: CGFloat, rule: TableLine.Rule, weight: Font.Weight, headings: [String], amounts: [Int64]) -> TableLine {
		TableLine(
			id: id,
			height: height,
			rule: rule,
			leading: AnyView(
				Text(title)
					.font(.text(.body, weight: weight))
					.padding(.leading, Spacing.s4)
					.padding(.trailing, Spacing.s3)
			),
			cells: amounts.indices.map { index in
				let isTotal = index == amounts.count - 1
				return AnyView(
					ReportAmount(amount: amounts[index], format: format, weight: weight == .bold ? .bold : (isTotal ? .semibold : .medium))
						.padding(.horizontal, Spacing.s4)
						.accessibilityElement(children: .ignore)
						.accessibilityLabel("\(title), \(headings[index]): \(format.amount(amounts[index], on: .register))")
				)
			}
		)
	}
}

/// Actual's report settings, previewed as they change.
struct CustomReportEditor: View {
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
					ConditionsEditor(conditions: $draft.conditions, matchAny: $draft.conditionsOr, names: model.names, style: model.numberStyle)
				} else if draft.settings.conditions.count > 0 {
					Text(draft.settings.conditions.count == 1 ? "1 filter" : "\(draft.settings.conditions.count) filters")
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
