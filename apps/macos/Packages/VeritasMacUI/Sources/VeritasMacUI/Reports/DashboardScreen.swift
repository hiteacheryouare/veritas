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
			WidgetConfigSheet(card: card, reports: reports) { config in
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
						Button(report.name) { Task { await model.add(.customReport, config: WidgetConfig(reportID: report.id)) } }
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

/// Title, its range under it, and the card's headline figure on the right.
private struct WidgetHeader: View {
	let card: DashboardCard
	let format: MoneyFormat

	var body: some View {
		HStack(alignment: .firstTextBaseline) {
			VStack(alignment: .leading, spacing: 2) {
				if let title = card.title {
					Text(title)
						.font(.text(.body, weight: .semibold))
						.lineLimit(1)
				}
				if let range = card.range {
					Text(range.title)
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
		case .netWorth(let points):
			if let last = points.last {
				VStack(alignment: .trailing, spacing: 2) {
					ReportAmount(amount: last.netWorth, format: format, weight: .semibold)
					if let first = points.first, points.count > 1 {
						Signed(amount: last.netWorth - first.netWorth, format: format)
					}
				}
			}
		case .cashFlow(_, _, let net):
			Signed(amount: net, format: format, weight: .semibold)
		case .spending(_, let spent, _, let compare):
			Signed(amount: compare - spent, format: format, weight: .semibold)
		case .calendar(_, let days):
			HStack(spacing: Spacing.s2) {
				Label(format.amount(days.reduce(0) { $0 + $1.income }), systemImage: "arrow.up")
					.foregroundStyle(Color.moneyAvailable)
				Label(format.amount(days.reduce(0) { $0 + $1.expenses }), systemImage: "arrow.down")
					.foregroundStyle(Color.moneyOverspent)
			}
			.font(.figures(.caption, weight: .semibold))
			.labelStyle(.titleAndIcon)
		case .ageOfMoney(let days, let change, _):
			VStack(alignment: .trailing, spacing: 2) {
				Text(days == 1 ? "1 day" : "\(days) days").font(.figures(.heading, weight: .semibold)).foregroundStyle(Color.moneyAvailable)
				Text(change >= 0 ? "Up \(change) days" : "Down \(-change) days")
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
			}
		case .crossover(_, _, _, let month):
			if let month { Text(ReportPeriod.monthYear(month)).font(.text(.body, weight: .semibold)) }
		case .monteCarlo(_, let rate):
			if let rate {
				Text(rate.formatted(.percent.precision(.fractionLength(0))))
					.font(.figures(.heading, weight: .semibold))
					.accessibilityLabel("Success rate \(rate.formatted(.percent.precision(.fractionLength(0))))")
			}
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
		Text((amount > 0 ? "+" : "") + format.amount(amount))
			.font(.figures(.body, weight: weight))
			.foregroundStyle(amount < 0 ? Color.moneyOverspent : Color.moneyAvailable)
	}
}

private struct WidgetBody: View {
	let card: DashboardCard
	let format: MoneyFormat

	var body: some View {
		switch card.content {
		case .summary(let figure), .formula(let figure):
			BigFigure(figure: figure, format: format)
		case .netWorth(let points):
			Chart(points, id: \.month) { point in
				AreaMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
					.foregroundStyle(Color.moneyIncomeFill.opacity(0.16))
				LineMark(x: .value("Month", ReportPeriod.shortMonth(point.month)), y: .value("Net worth", Double(point.netWorth) / 100))
					.foregroundStyle(Color.moneyIncomeFill)
			}
			.chartYAxis(.hidden)
			.chartYScale(domain: .automatic(includesZero: false))
		case .cashFlow(let income, let expenses, _):
			PairBars(first: ("Income", income, Color.moneyIncomeFill), second: ("Expenses", expenses, Color.chartExpense), format: format)
		case .spending(let month, let spent, let compareMonth, let compareSpent):
			PairBars(first: (ReportPeriod.monthName(month), spent, Color.chartExpense), second: (ReportPeriod.monthName(compareMonth), compareSpent, Color.chartExpenseSoft), format: format)
		case .sankey(let report):
			SankeyCanvas(report: report, format: format, compact: true) { _ in }
		case .customReport(let report):
			CustomReportDetail(view: report, format: format, compact: true)
		case .markdown(let text):
			ScrollView { MarkdownText(text: text) }
		case .calendar(let month, let days):
			CalendarGrid(month: month, days: days, format: format)
		case .crossover(let months, let expenses, let income, let crossover):
			Chart {
				ForEach(Array(months.enumerated()), id: \.offset) { index, month in
					LineMark(x: .value("Month", index), y: .value("Amount", Double(expenses[index]) / 100), series: .value("Line", "Expenses"))
						.foregroundStyle(Color.chartExpense)
					LineMark(x: .value("Month", index), y: .value("Amount", Double(income[index]) / 100), series: .value("Line", "Investment income"))
						.foregroundStyle(Color.chartSavings)
				}
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
			.accessibilityLabel(crossover.map { "Investment income covers expenses from \(ReportPeriod.monthYear($0))" } ?? "Investment income against expenses")
		case .budgetAnalysis(let months, let budgeted, let spent):
			Chart {
				ForEach(Array(months.enumerated()), id: \.offset) { index, month in
					BarMark(x: .value("Month", ReportPeriod.shortMonth(month)), y: .value("Amount", Double(budgeted[index]) / 100))
						.position(by: .value("Series", "Budgeted"))
						.foregroundStyle(Color.moneyIncomeFill)
					BarMark(x: .value("Month", ReportPeriod.shortMonth(month)), y: .value("Amount", Double(spent[index]) / 100))
						.position(by: .value("Series", "Spent"))
						.foregroundStyle(Color.chartExpense)
				}
			}
			.chartYAxis(.hidden)
		case .balanceForecast(let points), .monteCarlo(let points, _):
			Chart(points, id: \.month) { point in
				AreaMark(x: .value("Month", point.month.firstDay), yStart: .value("Low", Double(point.low) / 100), yEnd: .value("High", Double(point.high) / 100))
					.foregroundStyle(Color.chartBand)
				LineMark(x: .value("Month", point.month.firstDay), y: .value("Expected", Double(point.middle) / 100))
					.foregroundStyle(Color.moneyIncomeFill)
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
		case .ageOfMoney(_, _, let series):
			Chart(Array(series.enumerated()), id: \.offset) { index, days in
				AreaMark(x: .value("Month", index), y: .value("Days", days))
					.foregroundStyle(Color.moneyAvailableFill.opacity(0.18))
				LineMark(x: .value("Month", index), y: .value("Days", days))
					.foregroundStyle(Color.moneyAvailable)
			}
			.chartXAxis(.hidden)
			.chartYAxis(.hidden)
		case .pending, .unknown:
			EmptyView()
		}
	}
}

/// A summary or formula card's figure, as large as the card allows.
private struct BigFigure: View {
	let figure: CardFigure
	let format: MoneyFormat

	private var text: String {
		switch figure {
		case .money(let amount): format.amount(amount)
		case .percent(let value): (value / 100).formatted(.percent.precision(.fractionLength(0...1)))
		case .number(let value): value.formatted(.number.precision(.fractionLength(0...2)))
		case .days(let value): value == 1 ? "1 day" : "\(value) days"
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
				Text(format.amount(item.1)).font(.figures(.caption, weight: .semibold))
			}
			RoundedRectangle(cornerRadius: 3).fill(item.2).frame(width: 18, height: max(height, 2))
		}
		.accessibilityElement(children: .combine)
	}
}

private struct CalendarGrid: View {
	let month: Month
	let days: [CalendarDay]
	let format: MoneyFormat

	private var leadingBlanks: Int {
		guard let date = month.dateValue else { return 0 }
		return Calendar.current.component(.weekday, from: date) - Calendar.current.firstWeekday
	}

	var body: some View {
		let columns = Array(repeating: GridItem(.flexible(), spacing: 4), count: 7)
		let symbols = Calendar.current.veryShortStandaloneWeekdaySymbols
		let first = Calendar.current.firstWeekday - 1
		let ordered = Array(symbols[first...] + symbols[..<first])
		let biggest = max(days.map { max($0.income, $0.expenses) }.max() ?? 1, 1)
		LazyVGrid(columns: columns, spacing: 4) {
			ForEach(cells(ordered), id: \.id) { cell in
				switch cell.kind {
				case .weekday(let symbol):
					Text(symbol).font(.text(.caption)).foregroundStyle(.textTertiary)
				case .blank:
					Color.clear.frame(height: 30)
				case .day(let day):
					dayCell(day, biggest: biggest)
				}
			}
		}
	}

	private enum CellKind {
		case weekday(String)
		case blank
		case day(CalendarDay)
	}

	private func cells(_ symbols: [String]) -> [(id: String, kind: CellKind)] {
		symbols.enumerated().map { ("weekday\($0.offset)", .weekday($0.element)) }
			+ (0..<((leadingBlanks + 7) % 7)).map { ("blank\($0)", .blank) }
			+ days.map { ("day\($0.day)", .day($0)) }
	}

	private func dayCell(_ day: CalendarDay, biggest: Int64) -> some View {
		ZStack(alignment: .bottom) {
			RoundedRectangle(cornerRadius: 4).fill(Color.track.opacity(0.5))
			HStack(alignment: .bottom, spacing: 2) {
				if day.income > 0 {
					Rectangle().fill(Color.moneyAvailableFill).frame(width: 4, height: 24 * CGFloat(day.income) / CGFloat(biggest))
				}
				if day.expenses > 0 {
					Rectangle().fill(Color.moneyOverspentFill).frame(width: 4, height: 24 * CGFloat(day.expenses) / CGFloat(biggest))
				}
			}
			.frame(maxWidth: .infinity, alignment: .trailing)
			.padding(3)
			Text("\(day.day)").font(.figures(.caption)).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).padding(4)
		}
		.frame(height: 30)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(day.day): in \(format.amount(day.income)), out \(format.amount(day.expenses))")
	}
}

extension Month {
	var dateValue: Date? {
		Calendar.current.date(from: DateComponents(year: Int(year), month: Int(month), day: 1))
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
