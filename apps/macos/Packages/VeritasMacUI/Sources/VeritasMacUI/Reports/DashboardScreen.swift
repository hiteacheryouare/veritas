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
	/// The card opened full size, as clicking a card in Actual opens its report.
	@State private var openedID: String?
	@Namespace private var morphSpace
	static let rowHeight: CGFloat = 100
	static let gap: CGFloat = Spacing.s3

	init(workspace: WorkspaceModel, model: DashboardModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? DashboardModel(workspace: workspace))
	}

	private var opened: DashboardCard? {
		openedID.flatMap { id in model.sheet?.cards.first { $0.id == id } }
	}

	/// The grid stays in place under an opened card, so going back finds it
	/// where it was scrolled. The card grows into its page and shrinks back.
	var body: some View {
		ZStack {
			grid
				.opacity(opened == nil ? 1 : 0)
				.allowsHitTesting(opened == nil)
				.accessibilityHidden(opened != nil)
			if let card = opened {
				CardDetail(card: card, workspace: workspace, model: model, morphSpace: morphSpace, configure: { configuring = $0 }, close: { open(nil) })
					.motionTransition(.fade)
			}
		}
		.navigationTitle(opened.map { $0.title ?? $0.rawKind } ?? model.sheet?.name ?? "Dashboard")
		.sheet(item: $configuring) { card in
			WidgetConfigSheet(card: card, reports: reports, names: model.names, style: model.numberStyle) { config in
				Task { await model.configure(card, config) }
			}
		}
		.task(id: workspace.changeCount) {
			await model.load()
			reports = await model.customReports()
		}
		.onChange(of: model.sheet?.id) { openedID = nil }
	}

	private func open(_ card: DashboardCard?) {
		Motion.perform(.morph) { openedID = card?.id }
	}

	private var grid: some View {
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
						DashboardGrid(sheet: sheet, model: model, openedID: openedID, morphSpace: morphSpace, open: { open($0) }, configure: { configuring = $0 }, remove: { removing = $0 })
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
		.overlay { if !model.hasLoaded { ScreenState(kind: .loading) } }
		.background(.surfaceWindow)
		.toolbar {
			if opened == nil {
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

struct DashboardGrid: View {
	let sheet: DashboardSheet
	let model: DashboardModel
	var openedID: String?
	var morphSpace: Namespace.ID?
	let open: (DashboardCard) -> Void
	let configure: (DashboardCard) -> Void
	let remove: (DashboardCard) -> Void

	var body: some View {
		let rows = sheet.rows
		GeometryReader { proxy in
			let column = (proxy.size.width - DashboardScreen.gap * 11) / 12
			ZStack(alignment: .topLeading) {
				ForEach(sheet.cards) { card in
					let row = rows[card.id] ?? card.y
					Group {
						if card.id == openedID {
							Color.clear
						} else {
							WidgetCard(card: card, row: row, model: model, cell: CGSize(width: column + DashboardScreen.gap, height: DashboardScreen.rowHeight + DashboardScreen.gap), open: open, configure: configure, remove: remove)
								.morph(card.id, in: morphSpace)
						}
					}
					.frame(width: span(card.width, column), height: height(card.height))
					.padding(.leading, CGFloat(card.x) * (column + DashboardScreen.gap))
					.padding(.top, CGFloat(row) * (DashboardScreen.rowHeight + DashboardScreen.gap))
				}
			}
			.motion(.settle, value: sheet.cards.map { Slot(card: $0, row: rows[$0.id] ?? $0.y) })
		}
		.frame(height: height(sheet.cards.map { (rows[$0.id] ?? $0.y) + $0.height }.max() ?? 0))
		.containerIdentifier("dashboard.grid")
	}

	/// Where a card sits, so the others settle into place when one moves or resizes.
	private struct Slot: Equatable {
		let id: String
		let x: Int64
		let row: Int64
		let width: Int64
		let height: Int64

		init(card: DashboardCard, row: Int64) {
			id = card.id
			x = card.x
			self.row = row
			width = card.width
			height = card.height
		}
	}

	private func span(_ columns: Int64, _ width: CGFloat) -> CGFloat {
		CGFloat(columns) * width + CGFloat(max(columns - 1, 0)) * DashboardScreen.gap
	}

	private func height(_ rows: Int64) -> CGFloat {
		CGFloat(rows) * DashboardScreen.rowHeight + CGFloat(max(rows - 1, 0)) * DashboardScreen.gap
	}
}

/// A card's place on the grid: its column and its compacted row.
struct GridSlot: Equatable {
	let column: Int64
	let row: Int64
}

/// A dropped card holds its drop target until the model has moved it, so it
/// never bounces back to its old slot first. A move can return before the
/// card moves, when a newer reload overtakes the move's own load, so the hold
/// ends only when the card's slot changes, or when the move is over and no
/// load is left that could still move it.
struct DropHold: Equatable {
	let id = UUID()
	let origin: GridSlot
	var isReleased = false
	var isMoveFinished = false

	func isOver(slot: GridSlot, isLoading: Bool) -> Bool {
		guard isReleased else { return false }
		return slot != origin || (isMoveFinished && !isLoading)
	}

	/// What the card watches to end its hold.
	struct Check: Equatable {
		let slot: GridSlot
		let isLoading: Bool
		let hold: DropHold?
	}
}

private struct WidgetCard: View {
	let card: DashboardCard
	let row: Int64
	let model: DashboardModel
	/// One grid step, gap included.
	let cell: CGSize
	let open: (DashboardCard) -> Void
	let configure: (DashboardCard) -> Void
	let remove: (DashboardCard) -> Void
	/// The pointer's travel while dragging, then the snapped slot's offset until the core answers.
	@State private var drag: CGSize = .zero
	@State private var hold: DropHold?

	private var slot: GridSlot { GridSlot(column: card.x, row: row) }

	/// Once the core moves the card, the offset shrinks by the same amount, so the card never jumps.
	private var dragOffset: CGSize {
		guard let hold else { return drag }
		return CGSize(
			width: drag.width - CGFloat(slot.column - hold.origin.column) * cell.width,
			height: drag.height - CGFloat(slot.row - hold.origin.row) * cell.height
		)
	}

	private var opens: Bool { !model.isEditing && card.opensFullSize }

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
		.contentShape(.rect(cornerRadius: Radius.section))
		.onTapGesture { if opens { open(card) } }
		.offset(dragOffset)
		.gesture(model.isEditing ? moveGesture : nil)
		.onChange(of: DropHold.Check(slot: slot, isLoading: model.isLoading, hold: hold)) { endHoldIfOver() }
		.contextMenu { menu }
		.accessibilityElement(children: .contain)
		.accessibilityLabel(card.title ?? card.rawKind)
		.accessibilityAddTraits(opens ? .isButton : [])
		.accessibilityAction(named: "Open") { if opens { open(card) } }
		.accessibilityIdentifier("widget.\(card.id)")
	}

	private func steps(_ translation: CGSize) -> (columns: Int64, rows: Int64) {
		(Int64((translation.width / cell.width).rounded()), Int64((translation.height / cell.height).rounded()))
	}

	/// The card follows the pointer, settles into the snapped slot on release,
	/// and holds it until the model has the card's new slot (`DropHold`).
	private var moveGesture: some Gesture {
		DragGesture(minimumDistance: 4)
			.onChanged { value in
				if hold == nil { hold = DropHold(origin: slot) }
				drag = value.translation
			}
			.onEnded { value in
				let (columns, rows) = steps(value.translation)
				hold?.isReleased = true
				let dropped = hold?.id
				Motion.perform(.settle) {
					drag = CGSize(width: CGFloat(columns) * cell.width, height: CGFloat(rows) * cell.height)
				}
				Task {
					await model.move(card, columns: columns, rows: rows)
					if hold?.id == dropped { hold?.isMoveFinished = true }
				}
			}
	}

	private func endHoldIfOver() {
		guard let hold, hold.isOver(slot: slot, isLoading: model.isLoading) else { return }
		Motion.perform(.settle) {
			drag = .zero
			self.hold = nil
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
		if opens {
			Button("Open") { open(card) }
		}
		if model.canEdit, card.kind != nil {
			Button("Configure…") { configure(card) }
		}
		if model.canPlace {
			Menu("Move") {
				Button("Left") { Task { await model.move(card, columns: -1) } }
				Button("Right") { Task { await model.move(card, columns: 1) } }
				Button("Up") { Task { await model.move(card, down: false) } }
					.disabled(model.sheet?.placementUp(card) == nil)
				Button("Down") { Task { await model.move(card, down: true) } }
					.disabled(model.sheet?.placementDown(card) == nil)
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
struct WidgetHeader: View {
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
						.lineLimit(2)
						.fixedSize(horizontal: false, vertical: true)
				}
				if case .spending(let mode, _, let compareTo, let averageFirst, let averageLast, _, _, let difference) = card.content {
					Text(CardWords.comparison(difference, mode: mode, compareTo: compareTo, averageFirst: averageFirst, averageLast: averageLast, format: format))
						.font(.text(.caption, weight: .medium))
						.foregroundStyle(Self.spendingTone(difference))
						.accessibilityIdentifier("widget.comparison")
				}
			}
			Spacer(minLength: Spacing.s2)
			headline
		}
	}

	/// Actual's tones: more than the comparison is the overspent tone, and the same is neutral.
	static func spendingTone(_ difference: Int64) -> Color {
		difference == 0 ? Color.moneyNeutral : (difference > 0 ? Color.moneyOverspent : Color.moneyAvailable)
	}

	@ViewBuilder private var headline: some View {
		switch card.content {
		case .netWorth(_, _, _, let netWorth, let change, _, _, _, _, _):
			VStack(alignment: .trailing, spacing: 2) {
				ReportAmount(amount: netWorth, tone: netWorth < 0 ? .overspent : .neutral, format: format, weight: .semibold)
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
				.foregroundStyle(Self.spendingTone(difference))
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
			.foregroundStyle(amount == 0 ? Color.moneyNeutral : (amount < 0 ? Color.moneyOverspent : Color.moneyAvailable))
	}
}

struct WidgetBody: View {
	let card: DashboardCard
	let format: MoneyFormat
	/// Full size: axes and legends, as Actual's report page draws the card.
	var expanded = false

	var body: some View {
		switch card.content {
		case .summary:
			BigFigure(figure: card.summaryFigure ?? .undefined, format: format)
		case .formula(_, let value, _, let fontSize, let staticFontSize):
			FormulaFigure(value: value, fontSize: fontSize, staticFontSize: staticFontSize, format: format)
		case .netWorth(_, let interval, let points, _, _, _, _, _, _, let series):
			NetWorthChart(points: points, series: series, interval: interval, stacked: card.config.mode == "stacked", filtered: card.isFiltered, format: format, expanded: expanded)
		case .cashFlow(_, let income, let expense, _):
			PairBars(first: ("Money in", income, ColorToken.moneyIncomeFill), second: ("Money out", expense, ColorToken.chartExpense), format: format)
		case .spending(let mode, let compare, let compareTo, _, _, let days, let headlineDay, _):
			SpendingChart(mode: mode, compare: compare, compareTo: compareTo, days: days, headlineDay: headlineDay, filtered: card.isFiltered, format: format, expanded: expanded)
		case .sankey(_, _, _, _, let showPercentages, let nodes, let links):
			SankeyCardCanvas(nodes: nodes, links: links, showPercentages: showPercentages, format: format)
		case .balanceForecast(_, let source, _, let zeroBased, let hasForecast, let points, let low, let scheduled, let filtered, let today):
			if hasForecast {
				VStack(alignment: .leading, spacing: Spacing.s1) {
					ForecastChart(points: points, low: low, today: today, zeroBased: zeroBased, format: format, expanded: expanded)
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
			MonteCarloChart(points: chart, format: format, expanded: expanded)
		case .customReport(let report):
			CustomReportDetail(view: report, format: format, compact: !expanded)
		case .markdown(let text, _):
			ScrollView { MarkdownText(text: text) }
		case .calendar(let months):
			if expanded {
				ScrollView {
					VStack(alignment: .leading, spacing: Spacing.s4) {
						ForEach(months.reversed(), id: \.start) { month in
							Text(Month(date: month.start).map(ReportPeriod.monthYear) ?? month.start).font(.text(.body, weight: .semibold))
							CalendarGrid(month: month, format: format)
						}
					}
				}
			} else if let month = months.last {
				CalendarGrid(month: month, format: format)
			}
		case .crossover(_, _, let points, let crossover, _, _, _, _, _, _, _):
			CrossoverChart(points: points, crossover: crossover, filtered: card.isFiltered, format: format, expanded: expanded)
		case .budgetAnalysis(_, _, let months, _, _, _, _):
			BudgetAnalysisChart(months: months, format: format, showsBalance: card.config.showBalance != false, expanded: expanded)
		case .ageOfMoney(_, _, _, let points, _, _, _):
			Chart(Array(points.enumerated()), id: \.offset) { index, point in
				AreaMark(x: .value("Period", index), y: .value("Days", point.days))
					.interpolationMethod(.monotone)
					.foregroundStyle(Color.moneyAvailableFill.opacity(0.18))
				LineMark(x: .value("Period", index), y: .value("Days", point.days))
					.interpolationMethod(.monotone)
					.foregroundStyle(Color.moneyAvailable)
			}
			.chartXAxis {
				if expanded { indexAxis(points.map(\.period)) }
			}
			.chartYAxis {
				if expanded {
					AxisMarks(position: .leading) { value in
						AxisGridLine().foregroundStyle(Color.chartGrid)
						AxisValueLabel { if let days = value.as(Int.self) { Text("\(days)d").font(.figures(.caption)).foregroundStyle(.textTertiary) } }
					}
				}
			}
			.chartYScale(domain: .automatic(includesZero: true))
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

	/// Zero is neutral; otherwise the sign picks the tone, as Actual's summary card does.
	private var tone: Color {
		switch figure {
		case .money(0), .undefined: Color.moneyNeutral
		case .percent(let value) where value == 0: Color.moneyNeutral
		default: figure.isNegative ? Color.moneyOverspent : Color.moneyAvailable
		}
	}

	/// Sized from the card, not the text, so cards of one size share one figure size.
	/// A figure longer than "−00,000.00" shrinks to fit.
	var body: some View {
		GeometryReader { proxy in
			let size = min(proxy.size.height * 0.6, proxy.size.width / (10 * 0.62))
			Text(text)
				.font(.system(size: max(size, 14), weight: .semibold, design: .rounded))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.4)
				.foregroundStyle(tone)
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
	let first: (String, Int64, ColorToken)
	let second: (String, Int64, ColorToken)
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

	private func bar(_ item: (String, Int64, ColorToken), height: CGFloat) -> some View {
		VStack(spacing: 4) {
			VStack(spacing: 0) {
				Text(item.0).font(.text(.caption)).foregroundStyle(.textSecondary)
				Text(format.amount(item.1, on: .register)).font(.figures(.caption, weight: .semibold))
			}
			RoundedRectangle(cornerRadius: 3).fill(Color(item.2))
				.overlay { RoundedRectangle(cornerRadius: 3).strokeBorder(Color.chartEdge(item.2), lineWidth: 1) }
				.frame(width: 18, height: max(height, 2))
		}
		.accessibilityElement(children: .combine)
	}
}

/// X labels for a chart drawn by index: a handful of the periods' own labels.
func indexAxis(_ labels: [String]) -> some AxisContent {
	AxisMarks(values: .automatic(desiredCount: 6)) { value in
		AxisValueLabel {
			if let index = value.as(Int.self), labels.indices.contains(index) {
				Text(labels[index]).font(.figures(.caption)).foregroundStyle(.textTertiary)
			}
		}
	}
}

func moneyAxis(_ format: MoneyFormat) -> some AxisContent {
	AxisMarks(position: .leading) { value in
		AxisGridLine().foregroundStyle(Color.chartGrid)
		AxisValueLabel {
			if let amount = value.as(Double.self) {
				Text(format.axisLabel(amount)).font(.figures(.caption)).foregroundStyle(.textTertiary)
			}
		}
	}
}

/// A top-to-bottom gradient that turns from the positive tone to the
/// negative one where the value crosses zero.
private func zeroSplit(_ stop: Double, positive: Color, negative: Color) -> LinearGradient {
	LinearGradient(
		stops: [.init(color: positive, location: 0), .init(color: positive, location: stop), .init(color: negative, location: stop), .init(color: negative, location: 1)],
		startPoint: .top, endPoint: .bottom
	)
}

/// The net worth card: the trend line, or each account stacked, with the
/// hovered period's figures. Below zero the trend turns to the overspent
/// tone, as Actual's graph turns red under zero. Lines are smoothed as
/// Actual smooths them (monotone; Actual's basis for daily and weekly has
/// no Swift Charts counterpart, so those stay monotone too).
private struct NetWorthChart: View {
	let points: [NetWorthCardPoint]
	let series: [NetWorthCardSeries]
	let interval: NetWorthCardInterval
	let stacked: Bool
	let filtered: Bool
	let format: MoneyFormat
	var expanded = false
	@State private var selected: Int?

	private var values: [Double] { points.map { format.chartValue($0.netWorth, on: .register) } }

	var body: some View {
		let domain = ZeroSplit.domain(values, includesZero: false)
		let baseline = ZeroSplit.baseline(in: domain)
		Chart {
			if stacked {
				ForEach(series, id: \.accountId) { account in
					ForEach(Array(account.points.enumerated()), id: \.offset) { index, balance in
						AreaMark(x: .value("Period", index), y: .value("Balance", format.chartValue(balance, on: .register)), stacking: .standard)
							.interpolationMethod(.monotone)
							.foregroundStyle(by: .value("Account", account.accountId))
					}
				}
			} else {
				ForEach(Array(values.enumerated()), id: \.offset) { index, value in
					AreaMark(x: .value("Period", index), yStart: .value("Baseline", baseline), yEnd: .value(CardWords.netWorth(filtered: filtered), value))
						.interpolationMethod(.monotone)
						.foregroundStyle(zeroSplit(ZeroSplit.areaStop(values, baseline: baseline), positive: Color.moneyIncomeFill.opacity(0.16), negative: Color.moneyOverspentFill.opacity(0.16)))
					LineMark(x: .value("Period", index), y: .value(CardWords.netWorth(filtered: filtered), value))
						.interpolationMethod(.monotone)
						.lineStyle(StrokeStyle(lineWidth: 2))
						.foregroundStyle(values.allSatisfy { $0 == 0 } ? LinearGradient(colors: [Color.moneyNeutral], startPoint: .top, endPoint: .bottom) : zeroSplit(ZeroSplit.lineStop(values), positive: Color.chartMark(.moneyIncomeFill), negative: Color.moneyOverspent))
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
		.chartForegroundStyleScale(domain: series.map(\.accountId), range: series.map { CardPalette.mark($0.color) })
		.chartLegend(.hidden)
		.chartXAxis {
			if expanded { indexAxis(points.map { CardWords.day($0.start) }) }
		}
		.chartYAxis {
			if expanded { moneyAxis(format) }
		}
		.chartYScale(domain: stacked ? ZeroSplit.domain(stackedTotals, includesZero: true) : domain)
		.chartOverlay { proxy in
			Rectangle().fill(.clear).contentShape(.rect)
				.onContinuousHover { phase in
					switch phase {
					case .active(let location): selected = proxy.value(atX: location.x, as: Int.self)
					case .ended: selected = nil
					}
				}
				.allowsHitTesting(true)
		}
	}

	/// Each period's stacked top and bottom, so the stacked areas fit.
	private var stackedTotals: [Double] {
		let count = series.map(\.points.count).max() ?? 0
		return (0..<count).flatMap { index -> [Double] in
			let balances = series.compactMap { $0.points.indices.contains(index) ? format.chartValue($0.points[index], on: .register) : nil }
			return [balances.filter { $0 > 0 }.reduce(0, +), balances.filter { $0 < 0 }.reduce(0, +)]
		}
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
			Text(format.amount(amount, on: .register))
				.monospacedDigit()
				.foregroundStyle(amount < 0 && title == CardWords.netWorth(filtered: filtered) ? Color.moneyOverspent : Color.textPrimary)
		}
		.font(.text(.caption))
	}
}

/// The spending card as Actual's `SpendingGraph` draws it: the compared
/// month's running total rising from zero as a filled line, and the
/// comparison dashed behind it, with the month-to-date figure labeled as
/// ST-10.10 says.
private struct SpendingChart: View {
	let mode: SpendingCardMode
	let compare: Month
	let compareTo: Month
	let days: [SpendingCardDay]
	let headlineDay: UInt32
	let filtered: Bool
	let format: MoneyFormat
	var expanded = false

	private var comparison: String {
		switch mode {
		case .singleMonth: ReportPeriod.monthName(compareTo)
		case .average: "Average"
		case .budget: "Budgeted"
		}
	}

	var body: some View {
		let points = SpendingPoint.all(days, mode: mode)
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
				ForEach(points, id: \.day) { point in
					if let other = point.comparison {
						AreaMark(x: .value("Day", point.day), y: .value("Amount", format.chartValue(other, on: .register)), series: .value("Line", comparison))
							.foregroundStyle(Color.chartExpenseSoft.opacity(0.5))
						LineMark(x: .value("Day", point.day), y: .value("Amount", format.chartValue(other, on: .register)), series: .value("Line", comparison))
							.lineStyle(StrokeStyle(lineWidth: 2, dash: [6, 6]))
							.foregroundStyle(Color.chartExpense.opacity(0.6))
					}
				}
				ForEach(points, id: \.day) { point in
					if let value = point.compare {
						AreaMark(x: .value("Day", point.day), y: .value("Amount", format.chartValue(value, on: .register)), series: .value("Line", ReportPeriod.monthName(compare)))
							.foregroundStyle(Color.moneyIncomeFill.opacity(0.2))
						LineMark(x: .value("Day", point.day), y: .value("Amount", format.chartValue(value, on: .register)), series: .value("Line", ReportPeriod.monthName(compare)))
							.lineStyle(StrokeStyle(lineWidth: 2.5))
							.foregroundStyle(Color.chartMark(.moneyIncomeFill))
					}
				}
			}
			.chartXScale(domain: 1...28)
			.chartYScale(domain: .automatic(includesZero: true))
			.chartXAxis {
				if expanded {
					AxisMarks(values: [1, 7, 14, 21, 28]) { value in
						AxisValueLabel {
							if let day = value.as(Int.self) { Text(day >= 28 ? "28+" : "\(day)").font(.figures(.caption)).foregroundStyle(.textTertiary) }
						}
					}
				}
			}
			.chartYAxis {
				if expanded { moneyAxis(format) }
			}
			if expanded {
				HStack(spacing: Spacing.s4) {
					LegendSwatch(title: ReportPeriod.monthName(compare), color: .chartMark(.moneyIncomeFill))
					LegendSwatch(title: comparison, color: .chartExpense, dashed: true)
				}
				.font(.text(.caption))
			}
		}
	}
}

private struct LegendSwatch: View {
	let title: String
	let color: Color
	var dashed = false

	var body: some View {
		HStack(spacing: Spacing.s1) {
			Path { path in
				path.move(to: CGPoint(x: 0, y: 4))
				path.addLine(to: CGPoint(x: 16, y: 4))
			}
			.stroke(color, style: StrokeStyle(lineWidth: 2, dash: dashed ? [4, 3] : []))
			.frame(width: 16, height: 8)
			Text(title).foregroundStyle(.textSecondary)
		}
	}
}

/// The balance forecast card as Actual draws it: a smoothed line that turns
/// to the overspent tone below zero, a zero line when the balance goes
/// negative, this month dashed, and the lowest point marked.
private struct ForecastChart: View {
	let points: [ForecastCardPoint]
	let low: ForecastCardPoint?
	let today: UInt32?
	let zeroBased: Bool
	let format: MoneyFormat
	var expanded = false

	var body: some View {
		let values = points.map { format.chartValue($0.balance, on: .register) }
		Chart {
			ForEach(Array(values.enumerated()), id: \.offset) { index, value in
				LineMark(x: .value("Day", index), y: .value("Balance", value))
					.interpolationMethod(.monotone)
					.lineStyle(StrokeStyle(lineWidth: 2))
					.foregroundStyle(zeroSplit(ZeroSplit.lineStop(values), positive: Color.chartMark(.moneyIncomeFill), negative: Color.moneyOverspent))
			}
			if values.contains(where: { $0 < 0 }) {
				RuleMark(y: .value("Zero", 0))
					.foregroundStyle(Color.textTertiary)
			}
			if let low, let index = points.firstIndex(of: low) {
				PointMark(x: .value("Day", index), y: .value(CardWords.lowestBalance, format.chartValue(low.balance, on: .register)))
					.foregroundStyle(low.balance < 0 ? Color.moneyOverspent : Color.textSecondary)
			}
			if let today {
				RuleMark(x: .value("Day", Int(today)))
					.lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
					.foregroundStyle(Color.moneyIncome)
			}
		}
		.chartXAxis {
			if expanded { indexAxis(points.map(\.date)) }
		}
		.chartYAxis {
			if expanded { moneyAxis(format) }
		}
		.chartYScale(domain: .automatic(includesZero: zeroBased))
	}
}

/// The crossover card: investment income against the expense line, with
/// the projected expenses dashed, as Actual's `CrossoverGraph` draws them.
private struct CrossoverChart: View {
	let points: [CrossoverCardPoint]
	let crossover: Month?
	let filtered: Bool
	let format: MoneyFormat
	var expanded = false

	var body: some View {
		let expenseLine = CardWords.crossoverCategories(filtered: filtered)
		Chart {
			ForEach(Array(points.enumerated()), id: \.offset) { index, point in
				LineMark(x: .value("Month", index), y: .value("Amount", format.chartValue(point.expenses, on: .register)), series: .value("Line", expenseLine))
					.interpolationMethod(.monotone)
					.foregroundStyle(Color.chartExpense)
				if let adjusted = point.adjustedExpenses {
					LineMark(x: .value("Month", index), y: .value("Amount", format.chartValue(adjusted, on: .register)), series: .value("Line", CardWords.crossoverProjectedCategories(filtered: filtered)))
						.interpolationMethod(.monotone)
						.lineStyle(StrokeStyle(lineWidth: 2, dash: [5, 5]))
						.foregroundStyle(Color.chartExpense)
				}
				LineMark(x: .value("Month", index), y: .value("Amount", format.chartValue(point.investmentIncome, on: .register)), series: .value("Line", CardWords.projectedReturn))
					.interpolationMethod(.monotone)
					.foregroundStyle(Color.chartMark(.chartSavings))
			}
			if let crossover, let index = points.firstIndex(where: { $0.month == crossover }) {
				RuleMark(x: .value("Month", index))
					.lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 4]))
					.foregroundStyle(Color.textTertiary)
			}
		}
		.chartXAxis {
			if expanded { indexAxis(points.map { ReportPeriod.shortMonth($0.month) }) }
		}
		.chartYAxis {
			if expanded { moneyAxis(format) }
		}
		.accessibilityLabel(CardWords.crossoverChart(crossover: crossover, filtered: filtered))
	}
}

/// The budget analysis card's bars as Actual draws them: budgeted, spent
/// (below zero, as Actual's figure is negative) and the overspending
/// adjustment, with the balance line unless the card turns it off.
private struct BudgetAnalysisChart: View {
	let months: [BudgetAnalysisPoint]
	let format: MoneyFormat
	let showsBalance: Bool
	var expanded = false

	var body: some View {
		Chart {
			ForEach(months, id: \.month) { point in
				let month = ReportPeriod.shortMonth(point.month)
				BarMark(x: .value("Month", month), y: .value("Amount", format.chartValue(point.budgeted, on: .register)))
					.position(by: .value("Series", "Budgeted"))
					.foregroundStyle(Color.chartMark(.moneyIncomeFill))
				BarMark(x: .value("Month", month), y: .value("Amount", format.chartValue(point.spent, on: .register)))
					.position(by: .value("Series", "Spent"))
					.foregroundStyle(Color.chartExpense)
				BarMark(x: .value("Month", month), y: .value("Amount", format.chartValue(point.overspendingAdjustment, on: .register)))
					.position(by: .value("Series", "Overspending"))
					.foregroundStyle(Color.chartMark(.moneyOverspentFill))
				if showsBalance {
					LineMark(x: .value("Month", month), y: .value("Amount", format.chartValue(point.balance, on: .register)), series: .value("Line", "Balance"))
						.interpolationMethod(.monotone)
						.foregroundStyle(Color.textSecondary)
				}
			}
			RuleMark(y: .value("Zero", 0))
				.foregroundStyle(Color.chartGrid)
		}
		.chartXAxis(expanded ? .automatic : .hidden)
		.chartYAxis {
			if expanded { moneyAxis(format) }
		}
	}
}

/// The Monte Carlo fan: p10 to p90 and p25 to p75 shaded, p50 drawn. A
/// point whose balance is NaN or infinite is a gap.
private struct MonteCarloChart: View {
	let points: [MonteCarloCardPoint]
	let format: MoneyFormat
	var expanded = false

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
						.interpolationMethod(.monotone)
						.foregroundStyle(Color.chartBand)
					AreaMark(x: .value("Age", point.age), yStart: .value("25th percentile", value(point.p25)), yEnd: .value("75th percentile", value(point.p75)), series: .value("Band", "inner.\(run.id)"))
						.interpolationMethod(.monotone)
						.foregroundStyle(Color.chartBand)
					LineMark(x: .value("Age", point.age), y: .value("Median", value(point.p50)), series: .value("Band", "median.\(run.id)"))
						.interpolationMethod(.monotone)
						.foregroundStyle(Color.chartMark(.moneyIncomeFill))
				}
			}
		}
		.chartXAxis(expanded ? .automatic : .hidden)
		.chartYAxis {
			if expanded { moneyAxis(format) }
		}
		.chartXScale(domain: .automatic(includesZero: false))
	}

	private func value(_ minorUnits: Double) -> Double {
		format.chartValue(minorUnits, on: .register)
	}
}

/// The Sankey card's graph as the core lays it out: nodes worded by
/// ST-10.10, ribbons as wide as their links. Hovering anywhere on a node or
/// ribbon lights its path and shows its amount; nothing is lit otherwise.
private struct SankeyCardCanvas: View {
	let nodes: [SankeyCardNode]
	let links: [SankeyCardLink]
	let showPercentages: Bool
	let format: MoneyFormat
	@State private var hovered: SankeyTarget?
	@State private var pointer: CGPoint = .zero

	var body: some View {
		GeometryReader { proxy in
			let layout = SankeyCardLayout(nodes: nodes, links: links, size: proxy.size)
			let lit = hovered.map { SankeyHighlight($0, links: SankeyCardLayout.links(links)) }
			ZStack(alignment: .topLeading) {
				Canvas { context, _ in
					for ribbon in layout.ribbons {
						let tint = links[ribbon.link].color.map(CardPalette.color) ?? CardPalette.color(nodes[ribbon.source].color)
						let opacity = lit.map { $0.flows.contains(ribbon.link) ? 0.6 : 0.12 } ?? 0.3
						context.fill(Path(ribbon.outline), with: .color(tint.opacity(opacity)))
					}
					for node in layout.nodes {
						let dimmed = lit.map { !$0.nodes.contains(node.index) } ?? false
						let shape = Path(roundedRect: node.rect, cornerRadius: 2)
						context.fill(shape, with: .color(CardPalette.color(nodes[node.index].color).opacity(dimmed ? 0.4 : 1)))
						context.stroke(shape, with: .color(CardPalette.edge(nodes[node.index].color).opacity(dimmed ? 0.4 : 1)), lineWidth: 1)
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
						.frame(width: placed.rect.width, height: max(placed.rect.height, 1))
						.position(x: placed.rect.midX, y: placed.rect.midY)
						.allowsHitTesting(false)
						.accessibilityElement()
						.accessibilityLabel(showPercentages && !node.percentage.isEmpty ? "\(words), \(node.percentage)" : words)
				}
				if let hovered, let readout = SankeyCardNode.readout(hovered, nodes: nodes, links: links) {
					SankeyReadoutView(readout: readout, detail: folded(hovered), format: format)
						.position(readoutPosition(in: proxy.size))
						.allowsHitTesting(false)
				}
			}
			.contentShape(.rect)
			.onContinuousHover { phase in
				switch phase {
				case .active(let location):
					pointer = location
					hovered = layout.target(at: location)
				case .ended:
					hovered = nil
				}
			}
		}
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("sankey.card")
	}

	/// What an Other node, or a flow into one, holds.
	private func folded(_ target: SankeyTarget) -> [String] {
		let held: [SankeyFolded] = switch target {
		case .node(let index) where nodes[index].label == .other:
			links.filter { $0.target.map(Int.init) == index || Int($0.source) == index }.flatMap(\.folded)
		case .flow(let index): links[index].folded
		default: []
		}
		return held.map { "\($0.name) \(format.amount($0.value, on: .register))" }
	}

	private func readoutPosition(in size: CGSize) -> CGPoint {
		CGPoint(x: min(max(pointer.x, 110), size.width - 110), y: max(pointer.y - 34, 20))
	}
}

/// The words and amount of the hovered node or flow.
struct SankeyReadoutView: View {
	let readout: SankeyReadout
	var detail: [String] = []
	let format: MoneyFormat

	var body: some View {
		VStack(alignment: .leading, spacing: 2) {
			Text(readout.title).font(.text(.caption, weight: .semibold))
			if let amount = readout.amount {
				Text(format.amount(amount, on: .register)).font(.figures(.caption))
			}
			ForEach(detail.prefix(8), id: \.self) { line in
				Text(line).font(.text(.caption)).foregroundStyle(.textSecondary)
			}
		}
		.fixedSize()
		.padding(Spacing.s2)
		.background(.surfaceRaised, in: .rect(cornerRadius: Radius.control))
		.overlay { RoundedRectangle(cornerRadius: Radius.control).strokeBorder(Color.edgeControl) }
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("sankey.readout")
	}
}

/// The core's chart colours (Actual's `chartQual1` to `chartQual9`), drawn
/// in Veritas's category palette. Money-state colours never stand in for a category.
enum CardPalette {
	static let tokens = ColorToken.categories

	static func token(_ index: UInt8) -> ColorToken {
		tokens[Int(index) % tokens.count]
	}

	static func color(_ index: UInt8) -> Color { Color(token(index)) }
	static func color(_ color: SankeyCardColor) -> Color { Color(token(color)) }
	static func edge(_ color: SankeyCardColor) -> Color { .chartEdge(token(color)) }
	static func edge(_ index: UInt8) -> Color { .chartEdge(token(index)) }
	/// For Swift Charts series, which can't take a stroke.
	static func mark(_ index: UInt8) -> Color { .chartMark(token(index)) }

	static func token(_ color: SankeyCardColor) -> ColorToken {
		switch color {
		case .palette(let index): Self.token(index)
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
					Rectangle().fill(Color.moneyAvailableFill).overlay { Rectangle().strokeBorder(Color.chartEdge(.moneyAvailableFill), lineWidth: 1) }.frame(width: 4, height: 24 * CGFloat(day.incomeSize) / 100)
				}
				if day.expense > 0 {
					Rectangle().fill(Color.moneyOverspentFill).overlay { Rectangle().strokeBorder(Color.chartEdge(.moneyOverspentFill), lineWidth: 1) }.frame(width: 4, height: 24 * CGFloat(day.expenseSize) / 100)
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
