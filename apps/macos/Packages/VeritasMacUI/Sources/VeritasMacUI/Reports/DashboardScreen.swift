import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Actual's dashboards on their 12-column grid, every widget drawn natively
/// (R-REP-4, D73). Widgets move, resize, configure and come and go; pages can
/// be made, renamed and deleted.
struct DashboardScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: DashboardModel
	@State private var configuring: DashboardCard?
	@State private var removing: DashboardCard?
	@State private var isNaming = false
	@State private var isRenaming = false
	@State private var isDeleting = false
	@State private var reports: [CustomReportItem] = []
	static let rowHeight: CGFloat = 100
	static let gap: CGFloat = Spacing.s3

	init(workspace: WorkspaceModel, model: DashboardModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? DashboardModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			Group {
				if let sheet = model.sheet {
					if sheet.cards.isEmpty {
						VStack(spacing: Spacing.s3) {
							Text("No widgets on \(sheet.name)")
								.foregroundStyle(.textSecondary)
							if model.canEdit { addMenu.fixedSize() }
						}
						.frame(maxWidth: .infinity, minHeight: 240)
						.containerIdentifier("dashboard.empty")
					} else {
						DashboardGrid(sheet: sheet, model: model, configure: { configuring = $0 }, remove: { removing = $0 })
					}
				} else if !model.sheets.isEmpty || !model.canEdit {
					Text("No dashboards")
						.foregroundStyle(.textSecondary)
						.frame(maxWidth: .infinity, minHeight: 240)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
		}
		.background(.surfaceWindow)
		.navigationTitle(model.sheet?.name ?? "Dashboard")
		.toolbar {
			ToolbarItem(placement: .navigation) { pageMenu }
			ToolbarItemGroup(placement: .primaryAction) {
				if model.canEdit { addMenu }
				Toggle(isOn: $model.isEditing) {
					Label(model.isEditing ? "Done" : "Edit layout", systemImage: model.isEditing ? "checkmark" : "rectangle.3.group")
				}
				.toggleStyle(.button)
				.labelStyle(.titleAndIcon)
				.disabled(!model.canPlace || model.sheet == nil)
				.accessibilityIdentifier("dashboard.edit")
			}
		}
		.sheet(item: $configuring) { card in
			WidgetConfigSheet(card: card, reports: reports, names: model.names, style: model.numberStyle) { config in
				Task { await model.configure(card, config) }
			}
		}
		.confirmationDialog("Remove \(removing?.title ?? "this widget")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } })) {
			Button("Remove", role: .destructive) {
				if let card = removing { Task { await model.remove(card) } }
			}
		}
		.confirmationDialog("Delete \(model.sheet?.name ?? "this dashboard")?", isPresented: $isDeleting) {
			Button("Delete Dashboard", role: .destructive) { Task { await model.deleteDashboard() } }
		} message: {
			Text("Its \(model.sheet?.cards.count ?? 0) widgets go with it.")
		}
		.task(id: workspace.changeCount) {
			await model.load()
			reports = await model.customReports()
		}
		.containerIdentifier("screen.dashboard")
	}

	/// The page switcher, like Actual's "Main ▾".
	private var pageMenu: some View {
		Menu {
			Picker("Dashboard", selection: Binding(get: { model.sheet?.id ?? "" }, set: { model.pageID = $0 })) {
				ForEach(model.sheets) { Text($0.name).tag($0.id) }
			}
			.pickerStyle(.inline)
			if model.canEdit {
				Divider()
				Button("New Dashboard…") { isNaming = true }
				Button("Rename Dashboard…") { isRenaming = true }
					.disabled(model.sheet == nil)
				Button("Delete Dashboard…", role: .destructive) { isDeleting = true }
					.disabled(model.sheets.count < 2)
			}
		} label: {
			Text(model.sheet?.name ?? "Dashboard")
		}
		.fixedSize()
		.accessibilityIdentifier("dashboard.pages")
		.popover(isPresented: $isNaming, arrowEdge: .bottom) {
			NamePrompt(title: "New dashboard", action: "Create") { name in
				await model.createDashboard(named: name)
				return true
			}
		}
		.popover(isPresented: $isRenaming, arrowEdge: .bottom) {
			NamePrompt(title: "Rename dashboard", action: "Rename", name: model.sheet?.name ?? "") { name in
				await model.renameDashboard(to: name)
				return true
			}
		}
	}

	private var addMenu: some View {
		Menu {
			ForEach(WidgetKind.allCases.filter { $0 != .customReport }, id: \.self) { kind in
				Button(kind.title) { Task { await model.add(kind) } }
					.accessibilityIdentifier("dashboard.add.\(kind.rawValue)")
			}
			if !reports.isEmpty {
				Section("Custom reports") {
					ForEach(reports) { report in
						Button(report.name) { Task { await model.add(.customReport, config: .showing(report: report.id)) } }
					}
				}
			}
		} label: {
			Label("Add widget", systemImage: "plus")
		}
		.labelStyle(.titleAndIcon)
		.disabled(model.sheet == nil)
		.accessibilityIdentifier("dashboard.add")
	}
}

private struct DashboardGrid: View {
	let sheet: DashboardSheet
	let model: DashboardModel
	let configure: (DashboardCard) -> Void
	let remove: (DashboardCard) -> Void

	var body: some View {
		GeometryReader { proxy in
			let column = (proxy.size.width - DashboardScreen.gap * 11) / 12
			ZStack(alignment: .topLeading) {
				ForEach(sheet.cards) { card in
					WidgetCard(card: card, model: model, cell: CGSize(width: column + DashboardScreen.gap, height: DashboardScreen.rowHeight + DashboardScreen.gap), configure: configure, remove: remove)
						.frame(width: span(card.width, column), height: height(card.height))
						.offset(x: CGFloat(card.x) * (column + DashboardScreen.gap), y: CGFloat(card.y) * (DashboardScreen.rowHeight + DashboardScreen.gap))
				}
			}
		}
		.frame(height: height(sheet.cards.map { $0.y + $0.height }.max() ?? 0))
		.containerIdentifier("dashboard.grid")
	}

	private func span(_ columns: Int64, _ width: CGFloat) -> CGFloat {
		CGFloat(columns) * width + CGFloat(max(columns - 1, 0)) * DashboardScreen.gap
	}

	private func height(_ rows: Int64) -> CGFloat {
		CGFloat(rows) * DashboardScreen.rowHeight + CGFloat(max(rows - 1, 0)) * DashboardScreen.gap
	}
}

private struct WidgetCard: View {
	let card: DashboardCard
	let model: DashboardModel
	/// One grid step, gap included.
	let cell: CGSize
	let configure: (DashboardCard) -> Void
	let remove: (DashboardCard) -> Void
	@State private var drag: CGSize = .zero

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			WidgetHeader(card: card, format: model.format)
			WidgetBody(card: card, format: model.format)
				.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
		}
		.padding(Spacing.s4)
		.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
		.overlay {
			RoundedRectangle(cornerRadius: Radius.section)
				.strokeBorder(model.isEditing ? Color.accent : Color.edgeGrouped, style: StrokeStyle(lineWidth: model.isEditing ? 1.5 : 1, dash: model.isEditing ? [5, 3] : []))
		}
		.overlay { if model.isEditing { handles } }
		.overlay(alignment: .bottomTrailing) { if model.isEditing { editButtons } }
		.offset(drag)
		.gesture(model.isEditing ? moveGesture : nil)
		.contextMenu { menu }
		.accessibilityElement(children: .contain)
		.accessibilityLabel(card.title ?? card.rawKind)
		.accessibilityIdentifier("widget.\(card.id)")
	}

	private func steps(_ translation: CGSize) -> (columns: Int64, rows: Int64) {
		(Int64((translation.width / cell.width).rounded()), Int64((translation.height / cell.height).rounded()))
	}

	private var moveGesture: some Gesture {
		DragGesture(minimumDistance: 4)
			.onChanged { drag = $0.translation }
			.onEnded { value in
				let (columns, rows) = steps(value.translation)
				drag = .zero
				Task { await model.move(card, columns: columns, rows: rows) }
			}
	}

	/// Edges and corners: each drags its own sides, as Actual's grid does.
	private var handles: some View {
		GeometryReader { proxy in
			let size = proxy.size
			ForEach(Handle.allCases, id: \.self) { handle in
				handle.shape
					.fill(Color.accent)
					.frame(width: handle.isCorner ? 10 : (handle.isVertical ? 6 : 28), height: handle.isCorner ? 10 : (handle.isVertical ? 28 : 6))
					.padding(6)
					.contentShape(.rect)
					.position(handle.point(in: size))
					.gesture(DragGesture(minimumDistance: 2).onEnded { value in
						let (columns, rows) = steps(value.translation)
						Task {
							await model.resize(
								card,
								leading: handle.leading ? columns : 0, top: handle.top ? rows : 0,
								trailing: handle.trailing ? columns : 0, bottom: handle.bottom ? rows : 0
							)
						}
					})
					.accessibilityHidden(true)
			}
		}
	}

	private var editButtons: some View {
		HStack(spacing: 4) {
			if model.canEdit, card.kind != nil {
				Button { configure(card) } label: { Image(systemName: "slider.horizontal.3") }
					.help("Configure")
					.accessibilityLabel("Configure \(card.title ?? card.rawKind)")
					.accessibilityIdentifier("widget.\(card.id).configure")
			}
			if model.canEdit {
				Button { remove(card) } label: { Image(systemName: "xmark") }
					.help("Remove")
					.accessibilityLabel("Remove \(card.title ?? card.rawKind)")
					.accessibilityIdentifier("widget.\(card.id).remove")
			}
		}
		.buttonStyle(.borderless)
		.padding(6)
		.background(.surfaceRaised, in: .rect(cornerRadius: Radius.control))
		.padding(Spacing.s2)
	}

	@ViewBuilder private var menu: some View {
		if model.canEdit, card.kind != nil {
			Button("Configure…") { configure(card) }
		}
		if model.canPlace {
			Menu("Move") {
				Button("Left") { Task { await model.move(card, columns: -1) } }
				Button("Right") { Task { await model.move(card, columns: 1) } }
				Button("Up") { Task { await model.move(card, rows: -1) } }
				Button("Down") { Task { await model.move(card, rows: 1) } }
			}
			Menu("Resize") {
				Button("Wider") { Task { await model.resize(card, columns: 1) } }
				Button("Narrower") { Task { await model.resize(card, columns: -1) } }
				Button("Taller") { Task { await model.resize(card, rows: 1) } }
				Button("Shorter") { Task { await model.resize(card, rows: -1) } }
			}
		}
		if model.canEdit {
			Divider()
			Button("Remove Widget…", role: .destructive) { remove(card) }
		}
	}
}

private enum Handle: CaseIterable {
	case top, bottom, leftEdge, rightEdge, topLeft, topRight, bottomLeft, bottomRight

	var leading: Bool { self == .leftEdge || self == .topLeft || self == .bottomLeft }
	var trailing: Bool { self == .rightEdge || self == .topRight || self == .bottomRight }
	var top: Bool { self == .top || self == .topLeft || self == .topRight }
	var bottom: Bool { self == .bottom || self == .bottomLeft || self == .bottomRight }
	var isCorner: Bool { (leading || trailing) && (top || bottom) }
	var isVertical: Bool { self == .leftEdge || self == .rightEdge }

	var shape: AnyShape {
		isCorner ? AnyShape(Circle()) : AnyShape(Capsule())
	}

	func point(in size: CGSize) -> CGPoint {
		let x: CGFloat = leading ? 0 : (trailing ? size.width : size.width / 2)
		let y: CGFloat = top ? 0 : (bottom ? size.height : size.height / 2)
		return CGPoint(x: x, y: y)
	}
}

/// Title, the range or comparison under it, and the card's headline figure on the right.
private struct WidgetHeader: View {
	let card: DashboardCard
	let format: MoneyFormat

	var body: some View {
		HStack(alignment: .firstTextBaseline) {
			VStack(alignment: .leading, spacing: 2) {
				HStack(spacing: Spacing.s1) {
					if let title = card.title {
						Text(title)
							.font(.text(.body, weight: .semibold))
							.lineLimit(1)
					}
					if let caption = CardWords.caption(card) {
						InfoButton(caption: caption)
					}
				}
				if let subtitle = card.subtitle {
					Text(subtitle)
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
						.lineLimit(1)
				}
			}
			Spacer(minLength: Spacing.s2)
			headline
		}
	}

	@ViewBuilder private var headline: some View {
		switch card.content {
		case .netWorth(_, _, _, let netWorth, let change, _, _, _, _, _):
			VStack(alignment: .trailing, spacing: 2) {
				ReportAmount(amount: netWorth, format: format, weight: .semibold)
					.accessibilityLabel("\(CardWords.netWorth(filtered: card.isFiltered)) \(format.amount(netWorth, on: .register))")
				Signed(amount: change, format: format)
			}
		case .cashFlow(_, _, _, let change):
			Signed(amount: change, format: format, weight: .semibold)
				.accessibilityLabel("Money in less money out \(format.amount(change, on: .register))")
		case .spending(_, _, _, _, _, _, _, let difference):
			// Actual's tones: more than the comparison is the overspent tone.
			Text((difference > 0 ? "+" : "") + format.amount(difference, on: .register))
				.font(.figures(.body, weight: .semibold))
				.foregroundStyle(difference == 0 ? Color.textPrimary : (difference > 0 ? Color.moneyOverspent : Color.moneyAvailable))
		case .balanceForecast(_, _, _, _, true, let points, let low, _, _, _):
			if let ending = points.last {
				VStack(alignment: .trailing, spacing: 2) {
					Text("\(CardWords.endingBalance) \(format.amount(ending.balance, on: .register))")
						.font(.figures(.body, weight: .semibold))
						.foregroundStyle(ending.balance < 0 ? Color.moneyOverspent : Color.textPrimary)
					if let low {
						Text("\(CardWords.lowestBalance) \(format.amount(low.balance, on: .register))")
							.font(.figures(.caption))
							.foregroundStyle(.textSecondary)
					}
				}
			}
		case .calendar(let months):
			if let shown = months.last {
				HStack(spacing: Spacing.s2) {
					Label(format.amount(shown.totalIncome, on: .register), systemImage: "arrow.up")
						.foregroundStyle(Color.moneyAvailable)
						.accessibilityLabel("Money in \(format.amount(shown.totalIncome, on: .register))")
					Label(format.amount(shown.totalExpense, on: .register), systemImage: "arrow.down")
						.foregroundStyle(Color.moneyNeutral)
						.accessibilityLabel("Money out \(format.amount(shown.totalExpense, on: .register))")
				}
				.font(.figures(.caption, weight: .semibold))
				.labelStyle(.titleAndIcon)
			}
		case .ageOfMoney(_, _, _, _, let days, let trend, let insufficientData):
			if let days, !insufficientData {
				VStack(alignment: .trailing, spacing: 2) {
					Text(days == 1 ? "1 day" : "\(days) days").font(.figures(.heading, weight: .semibold)).foregroundStyle(Color.moneyAvailable)
					Text(trend.words)
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
				}
			}
		case .crossover(_, _, _, _, _, _, _, _, let years, _, _):
			VStack(alignment: .trailing, spacing: 2) {
				Text(years.map { $0.formatted(.number.precision(.fractionLength(0...1))) } ?? "N/A")
					.font(.figures(.heading, weight: .semibold))
				Text("Years to crossover")
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
			}
			.accessibilityElement(children: .combine)
		case .monteCarlo(let percent, _, let endAge, _, _):
			VStack(alignment: .trailing, spacing: 2) {
				Text(CardWords.successRate(percent))
					.font(.figures(.heading, weight: .semibold))
				Text(CardWords.successRateLabel(endAge: endAge))
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
			}
			.accessibilityElement(children: .combine)
		default:
			EmptyView()
		}
	}
}

/// "+138.18" or "−61.34", in the tone of its sign.
private struct Signed: View {
	let amount: Int64
	let format: MoneyFormat
	var weight: Font.Weight = .medium

	var body: some View {
		Text((amount > 0 ? "+" : "") + format.amount(amount, on: .register))
			.font(.figures(.body, weight: weight))
			.foregroundStyle(amount < 0 ? Color.moneyOverspent : Color.moneyAvailable)
	}
}

private struct WidgetBody: View {
	let card: DashboardCard
	let format: MoneyFormat

	var body: some View {
		switch card.content {
		case .summary:
			BigFigure(figure: card.summaryFigure ?? .undefined, format: format)
		case .formula(_, let value, _, let fontSize, let staticFontSize):
			FormulaFigure(value: value, fontSize: fontSize, staticFontSize: staticFontSize, format: format)
		case .netWorth(_, _, let points, _, _, _, _, _, _, let series):
			NetWorthChart(points: points, series: series, stacked: card.config.mode == "stacked", filtered: card.isFiltered, format: format)
		case .cashFlow(_, let income, let expense, _):
			PairBars(first: ("Money in", income, Color.moneyIncomeFill), second: ("Money out", expense, Color.chartExpense), format: format)
		case .spending(let mode, let compare, let compareTo, _, _, let days, let headlineDay, _):
			SpendingChart(mode: mode, compare: compare, compareTo: compareTo, days: days, headlineDay: headlineDay, filtered: card.isFiltered, format: format)
		case .sankey(_, _, _, _, let showPercentages, let nodes, let links):
			SankeyCardCanvas(nodes: nodes, links: links, showPercentages: showPercentages, format: format)
		case .balanceForecast(_, let source, _, let zeroBased, let hasForecast, let points, let low, let scheduled, let filtered, let today):
			if hasForecast {
				VStack(alignment: .leading, spacing: Spacing.s1) {
					ForecastChart(points: points, low: low, today: today, zeroBased: zeroBased, format: format)
						.frame(maxHeight: .infinity)
					Text(CardWords.forecastFooter(source: source, scheduled: scheduled, filtered: filtered))
						.font(.text(.caption))
						.foregroundStyle(.textSecondary)
						.lineLimit(2)
				}
			} else {
				Text(CardWords.noForecast)
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
					.multilineTextAlignment(.center)
					.frame(maxWidth: .infinity, maxHeight: .infinity)
			}
		case .monteCarlo(_, _, _, _, let chart):
			MonteCarloChart(points: chart, format: format)
		case .customReport(let report):
			CustomReportDetail(view: report, format: format, compact: true)
		case .markdown(let text, _):
			ScrollView { MarkdownText(text: text) }
		case .calendar(let months):
			if let month = months.last {
				CalendarGrid(month: month, format: format)
			}
		case .crossover(_, _, let points, let crossover, _, _, _, _, _, _, _):
			Chart {
				ForEach(Array(points.enumerated()), id: \.offset) { index, point in
					LineMark(x: .value("Month", index), y: .value("Amount", format.chartValue(point.expenses, on: .register)), series: .value("Line", card.isFiltered ? "Selected categories" : "All non-income categories"))
						.foregroundStyle(Color.chartExpense)
					LineMark(x: .value("Month", index), y: .value("Amount", format.chartValue(point.investmentIncome, on: .register)), series: .value("Line", "Investment income"))
						.foregroundStyle(Color.chartSavings)
				}
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
			.accessibilityLabel(crossover.map { "Investment income covers expenses from \(ReportPeriod.monthYear($0))" } ?? "Investment income against expenses")
		case .budgetAnalysis(_, _, let months, _, _, _, _):
			Chart {
				ForEach(months, id: \.month) { point in
					BarMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Amount", format.chartValue(point.budgeted, on: .register)))
						.position(by: .value("Series", "Budgeted"))
						.foregroundStyle(Color.moneyIncomeFill)
					BarMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Amount", format.chartValue(point.spent, on: .register)))
						.position(by: .value("Series", "Spent"))
						.foregroundStyle(Color.chartExpense)
				}
			}
			.chartYAxis(.hidden)
		case .ageOfMoney(_, _, _, let points, _, _, _):
			Chart(Array(points.enumerated()), id: \.offset) { index, point in
				AreaMark(x: .value("Period", index), y: .value("Days", point.days))
					.foregroundStyle(Color.moneyAvailableFill.opacity(0.18))
				LineMark(x: .value("Period", index), y: .value("Days", point.days))
					.foregroundStyle(Color.moneyAvailable)
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
		case .missingReport:
			Text("This report was deleted.")
				.foregroundStyle(.textSecondary)
				.accessibilityIdentifier("card.missingReport")
		case .problem(let message):
			Text(message)
				.foregroundStyle(.textSecondary)
				.accessibilityIdentifier("card.problem")
		case .unknown:
			EmptyView()
		}
	}
}

/// A summary card's figure, as large as the card allows.
private struct BigFigure: View {
	let figure: CardFigure
	let format: MoneyFormat

	private var text: String {
		switch figure {
		case .money(let amount): format.amount(amount, on: .register)
		case .percent(let value): (value / 100).formatted(.percent.precision(.fractionLength(0...1)))
		case .undefined: ""
		}
	}

	var body: some View {
		GeometryReader { proxy in
			let size = min(proxy.size.height * 0.85, proxy.size.width / (CGFloat(text.count) * 0.6))
			Text(text)
				.font(.system(size: max(size, 14), weight: .semibold, design: .rounded))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.5)
				.foregroundStyle(figure.isNegative ? Color.moneyOverspent : Color.moneyAvailable)
				.frame(maxWidth: .infinity, maxHeight: .infinity)
		}
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("widget.figure")
	}
}

/// What the formula returns, with nothing added (ST-10.10). An error shows
/// Actual's own message in the error colour.
private struct FormulaFigure: View {
	let value: FormulaValue
	let fontSize: FormulaFontSize
	let staticFontSize: Double
	let format: MoneyFormat

	var body: some View {
		let shown = CardWords.formula(value, format: format)
		GeometryReader { proxy in
			let lines = shown.text.split(separator: "\n", omittingEmptySubsequences: false)
			let longest = CGFloat(max(lines.map(\.count).max() ?? 1, 1))
			let fitted = min(proxy.size.height * 0.85 / CGFloat(max(lines.count, 1)), proxy.size.width / (longest * 0.6))
			Text(shown.text)
				.font(.system(size: fontSize == .static ? staticFontSize : max(fitted, 12), weight: .semibold, design: .rounded))
				.monospacedDigit()
				.multilineTextAlignment(.center)
				.minimumScaleFactor(0.5)
				.foregroundStyle(shown.isError ? Color.moneyOverspent : Color.textPrimary)
				.frame(maxWidth: .infinity, maxHeight: .infinity)
		}
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("widget.figure")
	}
}

private struct PairBars: View {
	let first: (String, Int64, Color)
	let second: (String, Int64, Color)
	let format: MoneyFormat

	var body: some View {
		GeometryReader { proxy in
			let largest = CGFloat(max(abs(first.1), abs(second.1), 1))
			let room = max(proxy.size.height - 34, 10)
			HStack(alignment: .bottom, spacing: Spacing.s4) {
				bar(first, height: room * CGFloat(abs(first.1)) / largest)
				bar(second, height: room * CGFloat(abs(second.1)) / largest)
			}
			.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
		}
	}

	private func bar(_ item: (String, Int64, Color), height: CGFloat) -> some View {
		VStack(spacing: 4) {
			VStack(spacing: 0) {
				Text(item.0).font(.text(.caption)).foregroundStyle(.textSecondary)
				Text(format.amount(item.1, on: .register)).font(.figures(.caption, weight: .semibold))
			}
			RoundedRectangle(cornerRadius: 3).fill(item.2).frame(width: 18, height: max(height, 2))
		}
		.accessibilityElement(children: .combine)
	}
}

/// The net worth card: the trend line, or each account stacked, with the
/// hovered period's figures.
private struct NetWorthChart: View {
	let points: [NetWorthCardPoint]
	let series: [NetWorthCardSeries]
	let stacked: Bool
	let filtered: Bool
	let format: MoneyFormat
	@State private var selected: Int?

	var body: some View {
		Chart {
			if stacked {
				ForEach(series, id: \.accountId) { account in
					ForEach(Array(account.points.enumerated()), id: \.offset) { index, balance in
						AreaMark(x: .value("Period", index), y: .value("Balance", format.chartValue(balance, on: .register)), stacking: .standard)
							.foregroundStyle(by: .value("Account", account.accountId))
					}
				}
			} else {
				ForEach(Array(points.enumerated()), id: \.offset) { index, point in
					AreaMark(x: .value("Period", index), y: .value(CardWords.netWorth(filtered: filtered), format.chartValue(point.netWorth, on: .register)))
						.foregroundStyle(Color.moneyIncomeFill.opacity(0.16))
					LineMark(x: .value("Period", index), y: .value(CardWords.netWorth(filtered: filtered), format.chartValue(point.netWorth, on: .register)))
						.foregroundStyle(Color.moneyIncomeFill)
				}
			}
			if let selected, points.indices.contains(selected) {
				RuleMark(x: .value("Period", selected))
					.foregroundStyle(Color.textTertiary)
					.annotation(position: .top, overflowResolution: .init(x: .fit(to: .chart), y: .disabled)) {
						tooltip(points[selected])
					}
			}
		}
		.chartForegroundStyleScale(domain: series.map(\.accountId), range: series.map { CardPalette.color($0.color) })
		.chartLegend(.hidden)
		.chartXSelection(value: $selected)
		.chartXAxis(.hidden)
		.chartYAxis(.hidden)
		.chartYScale(domain: .automatic(includesZero: false))
	}

	private func tooltip(_ point: NetWorthCardPoint) -> some View {
		VStack(alignment: .leading, spacing: 2) {
			Text(CardWords.day(point.start)).font(.text(.caption, weight: .semibold))
			line(CardWords.assets(filtered: filtered), point.assets)
			line(CardWords.liabilities(filtered: filtered), point.debt)
			line("Change", point.change)
			line(CardWords.netWorth(filtered: filtered), point.netWorth)
		}
		.padding(Spacing.s2)
		.background(.surfaceRaised, in: .rect(cornerRadius: Radius.control))
		.overlay { RoundedRectangle(cornerRadius: Radius.control).strokeBorder(Color.edgeControl) }
	}

	private func line(_ title: String, _ amount: Int64) -> some View {
		HStack {
			Text(title).foregroundStyle(.textSecondary)
			Spacer(minLength: Spacing.s3)
			Text(format.amount(amount, on: .register)).monospacedDigit()
		}
		.font(.text(.caption))
	}
}

/// The spending card: this month's running total against the comparison,
/// day by day, with the month-to-date figure labeled as ST-10.10 says.
private struct SpendingChart: View {
	let mode: SpendingCardMode
	let compare: Month
	let compareTo: Month
	let days: [SpendingCardDay]
	let headlineDay: UInt32
	let filtered: Bool
	let format: MoneyFormat

	private var comparison: String {
		switch mode {
		case .singleMonth: ReportPeriod.monthName(compareTo)
		case .average: "Average"
		case .budget: "Budgeted"
		}
	}

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s1) {
			if let today = days.first(where: { $0.day == headlineDay })?.compare {
				HStack(spacing: Spacing.s1) {
					Text(CardWords.spending(filtered: filtered)).foregroundStyle(.textSecondary)
					Text(format.amount(today, on: .register)).font(.figures(.caption, weight: .semibold))
				}
				.font(.text(.caption))
				.lineLimit(1)
				.accessibilityElement(children: .combine)
			}
			Chart {
				ForEach(days, id: \.day) { day in
					if let other = otherValue(day) {
						LineMark(x: .value("Day", Int(day.day)), y: .value("Amount", other), series: .value("Line", comparison))
							.foregroundStyle(Color.chartExpenseSoft)
					}
					if let value = day.compare {
						LineMark(x: .value("Day", Int(day.day)), y: .value("Amount", format.chartValue(value, on: .register)), series: .value("Line", ReportPeriod.monthName(compare)))
							.foregroundStyle(Color.chartExpense)
					}
				}
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
		}
	}

	private func otherValue(_ day: SpendingCardDay) -> Double? {
		switch mode {
		case .singleMonth: day.compareTo.map { format.chartValue($0, on: .register) }
		case .average: format.chartValue(day.average, on: .register)
		case .budget: day.budget.isFinite ? format.chartValue(day.budget, on: .register) : nil
		}
	}
}

/// The balance forecast card's running balance, its lowest point and this month.
private struct ForecastChart: View {
	let points: [ForecastCardPoint]
	let low: ForecastCardPoint?
	let today: UInt32?
	let zeroBased: Bool
	let format: MoneyFormat

	var body: some View {
		Chart {
			ForEach(Array(points.enumerated()), id: \.offset) { index, point in
				LineMark(x: .value("Day", index), y: .value("Balance", format.chartValue(point.balance, on: .register)))
					.foregroundStyle(Color.moneyIncomeFill)
			}
			if let low, let index = points.firstIndex(of: low) {
				PointMark(x: .value("Day", index), y: .value(CardWords.lowestBalance, format.chartValue(low.balance, on: .register)))
					.foregroundStyle(low.balance < 0 ? Color.moneyOverspent : Color.textSecondary)
			}
			if let today {
				RuleMark(x: .value("Day", Int(today)))
					.foregroundStyle(Color.chartGrid)
			}
		}
		.chartXAxis(.hidden)
		.chartYAxis(.hidden)
		.chartYScale(domain: .automatic(includesZero: zeroBased))
	}
}

/// The Monte Carlo fan: p10 to p90 and p25 to p75 shaded, p50 drawn. A
/// point whose balance is NaN or infinite is a gap.
private struct MonteCarloChart: View {
	let points: [MonteCarloCardPoint]
	let format: MoneyFormat

	private struct Run: Identifiable {
		let id: Int
		let points: [MonteCarloCardPoint]
	}

	private var runs: [Run] {
		var runs: [[MonteCarloCardPoint]] = [[]]
		for point in points {
			if [point.p10, point.p25, point.p50, point.p75, point.p90, point.age].allSatisfy(\.isFinite) {
				runs[runs.count - 1].append(point)
			} else if !(runs.last?.isEmpty ?? true) {
				runs.append([])
			}
		}
		return runs.filter { !$0.isEmpty }.enumerated().map { Run(id: $0.offset, points: $0.element) }
	}

	var body: some View {
		Chart {
			ForEach(runs) { run in
				ForEach(run.points, id: \.year) { point in
					AreaMark(x: .value("Age", point.age), yStart: .value("10th percentile", value(point.p10)), yEnd: .value("90th percentile", value(point.p90)), series: .value("Band", "outer.\(run.id)"))
						.foregroundStyle(Color.chartBand)
					AreaMark(x: .value("Age", point.age), yStart: .value("25th percentile", value(point.p25)), yEnd: .value("75th percentile", value(point.p75)), series: .value("Band", "inner.\(run.id)"))
						.foregroundStyle(Color.chartBand)
					LineMark(x: .value("Age", point.age), y: .value("Median", value(point.p50)), series: .value("Band", "median.\(run.id)"))
						.foregroundStyle(Color.moneyIncomeFill)
				}
			}
		}
		.chartXAxis(.hidden)
		.chartYAxis(.hidden)
		.chartXScale(domain: .automatic(includesZero: false))
	}

	private func value(_ minorUnits: Double) -> Double {
		format.chartValue(minorUnits, on: .register)
	}
}

/// The Sankey card's graph as the core lays it out: nodes worded by
/// ST-10.10, ribbons as wide as their links.
private struct SankeyCardCanvas: View {
	let nodes: [SankeyCardNode]
	let links: [SankeyCardLink]
	let showPercentages: Bool
	let format: MoneyFormat

	var body: some View {
		GeometryReader { proxy in
			let layout = SankeyCardLayout(nodes: nodes, links: links, size: proxy.size)
			ZStack(alignment: .topLeading) {
				Canvas { context, _ in
					for ribbon in layout.ribbons {
						let tint = links[ribbon.link].color.map(CardPalette.color) ?? CardPalette.color(nodes[ribbon.source].color)
						context.fill(Self.path(ribbon), with: .color(tint.opacity(0.3)))
					}
					for node in layout.nodes {
						context.fill(Path(roundedRect: node.rect, cornerRadius: 2), with: .color(CardPalette.color(nodes[node.index].color)))
					}
				}
				ForEach(layout.nodes, id: \.index) { placed in
					let node = nodes[placed.index]
					let words = CardWords.sankey(node.label) ?? ""
					let leading = placed.column == layout.columns - 1 && layout.columns > 1
					if placed.rect.height >= 10 {
						HStack(spacing: 4) {
							Text(words).font(.text(.caption, weight: .medium))
							if showPercentages, !node.percentage.isEmpty {
								Text(node.percentage).font(.figures(.caption)).foregroundStyle(.textSecondary)
							}
						}
						.lineLimit(1)
						.fixedSize()
						.frame(width: 160, alignment: leading ? .trailing : .leading)
						.position(x: leading ? placed.rect.minX - 6 - 80 : placed.rect.maxX + 6 + 80, y: placed.rect.midY)
						.allowsHitTesting(false)
					}
					Color.clear
						.frame(width: placed.rect.width + 6, height: max(placed.rect.height, 10))
						.contentShape(.rect)
						.position(x: placed.rect.midX, y: placed.rect.midY)
						.help(help(placed.index))
						.accessibilityElement()
						.accessibilityLabel(showPercentages && !node.percentage.isEmpty ? "\(words), \(node.percentage)" : words)
				}
			}
		}
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("sankey.card")
	}

	/// A node's words, and for an Other node what it holds.
	private func help(_ index: Int) -> String {
		let words = CardWords.sankey(nodes[index].label) ?? ""
		guard nodes[index].label == .other else { return words }
		let folded = links.filter { $0.target.map(Int.init) == index || Int($0.source) == index }.flatMap(\.folded)
		return ([words] + folded.map { "\($0.name) \(format.amount($0.value, on: .register))" }).joined(separator: "\n")
	}

	static func path(_ ribbon: SankeyCardLayout.Ribbon) -> Path {
		var path = Path()
		let middle = (ribbon.sourceX + ribbon.targetX) / 2
		path.move(to: CGPoint(x: ribbon.sourceX, y: ribbon.sourceSpan.lowerBound))
		path.addCurve(to: CGPoint(x: ribbon.targetX, y: ribbon.targetSpan.lowerBound), control1: CGPoint(x: middle, y: ribbon.sourceSpan.lowerBound), control2: CGPoint(x: middle, y: ribbon.targetSpan.lowerBound))
		path.addLine(to: CGPoint(x: ribbon.targetX, y: ribbon.targetSpan.upperBound))
		path.addCurve(to: CGPoint(x: ribbon.sourceX, y: ribbon.sourceSpan.upperBound), control1: CGPoint(x: middle, y: ribbon.targetSpan.upperBound), control2: CGPoint(x: middle, y: ribbon.sourceSpan.upperBound))
		path.closeSubpath()
		return path
	}
}

/// The core's chart colours 0 to 8 (Actual's `chartQual1` to `chartQual9`),
/// drawn in Veritas's own palette.
enum CardPalette {
	static let colors: [Color] = [
		.moneyIncomeFill, .chartSavings, .chartDebtPaydown, .scenario3, .moneyLowFill, .chartExpense, .moneyOverspentFill, .scenarioBaseline, .moneyAvailable
	]

	static func color(_ index: UInt8) -> Color {
		colors[Int(index) % colors.count]
	}

	static func color(_ color: SankeyCardColor) -> Color {
		switch color {
		case .palette(let index): Self.color(index)
		case .toBudgetPositive: .moneyAvailableFill
		case .toBudgetNegative: .moneyOverspentFill
		case .gray: .chartExpense
		case .blue: .moneyIncomeFill
		}
	}
}

private struct CalendarGrid: View {
	let month: CalendarCardMonth
	let format: MoneyFormat

	private var first: Month? { Month(date: month.start) }

	private var leadingBlanks: Int {
		guard let date = first?.dateValue else { return 0 }
		return Calendar.current.component(.weekday, from: date) - Calendar.current.firstWeekday
	}

	var body: some View {
		let columns = Array(repeating: GridItem(.flexible(), spacing: 4), count: 7)
		let symbols = Calendar.current.veryShortStandaloneWeekdaySymbols
		let firstWeekday = Calendar.current.firstWeekday - 1
		let ordered = Array(symbols[firstWeekday...] + symbols[..<firstWeekday])
		LazyVGrid(columns: columns, spacing: 4) {
			ForEach(cells(ordered), id: \.id) { cell in
				switch cell.kind {
				case .weekday(let symbol):
					Text(symbol).font(.text(.caption)).foregroundStyle(.textTertiary)
				case .blank:
					Color.clear.frame(height: 30)
				case .day(let day):
					dayCell(day)
				}
			}
		}
	}

	private enum CellKind {
		case weekday(String)
		case blank
		case day(CalendarDayCell)
	}

	private func cells(_ symbols: [String]) -> [(id: String, kind: CellKind)] {
		symbols.enumerated().map { ("weekday\($0.offset)", .weekday($0.element)) }
			+ (0..<((leadingBlanks + 7) % 7)).map { ("blank\($0)", .blank) }
			+ month.days.map { ("day\($0.date)", .day($0)) }
	}

	/// Each bar is the day's share of the month, as the core sizes it.
	private func dayCell(_ day: CalendarDayCell) -> some View {
		let number = Int(day.date.suffix(2)) ?? 1
		return ZStack(alignment: .bottom) {
			RoundedRectangle(cornerRadius: 4).fill(Color.track.opacity(0.5))
			HStack(alignment: .bottom, spacing: 2) {
				if day.income > 0 {
					Rectangle().fill(Color.moneyAvailableFill).frame(width: 4, height: 24 * CGFloat(day.incomeSize) / 100)
				}
				if day.expense > 0 {
					Rectangle().fill(Color.moneyOverspentFill).frame(width: 4, height: 24 * CGFloat(day.expenseSize) / 100)
				}
			}
			.frame(maxWidth: .infinity, alignment: .trailing)
			.padding(3)
			Text("\(number)").font(.figures(.caption)).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).padding(4)
		}
		.frame(height: 30)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(number): money in \(format.amount(day.income, on: .register)), money out \(format.amount(day.expense, on: .register))")
	}
}

extension Month {
	var dateValue: Date? {
		date()
	}
}

/// A markdown card: headings and paragraphs, with inline emphasis and links.
struct MarkdownText: View {
	let text: String

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			ForEach(Array(Self.blocks(text).enumerated()), id: \.offset) { _, block in
				if block.hasPrefix("#") {
					Text(Self.inline(block.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)))
						.font(.text(.heading, weight: .semibold))
				} else {
					Text(Self.inline(block))
				}
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	static func blocks(_ text: String) -> [String] {
		text.components(separatedBy: "\n\n").flatMap { paragraph in
			paragraph.hasPrefix("#") ? paragraph.components(separatedBy: "\n") : [paragraph]
		}
		.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
		.filter { !$0.isEmpty }
	}

	static func inline(_ text: String) -> AttributedString {
		(try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
	}
}

private struct InfoButton: View {
	let caption: String
	@State private var isShown = false

	var body: some View {
		Button {
			isShown.toggle()
		} label: {
			Image(systemName: "info.circle")
				.foregroundStyle(.textTertiary)
		}
		.buttonStyle(.borderless)
		.accessibilityLabel("About this card")
		.popover(isPresented: $isShown, arrowEdge: .bottom) {
			Text(caption)
				.fixedSize(horizontal: false, vertical: true)
				.frame(width: 280)
				.padding(Spacing.s3)
		}
	}
}

private extension AgeTrend {
	var words: String {
		switch self {
		case .up: "Rising"
		case .down: "Falling"
		case .stable: "Steady"
		}
	}
}
