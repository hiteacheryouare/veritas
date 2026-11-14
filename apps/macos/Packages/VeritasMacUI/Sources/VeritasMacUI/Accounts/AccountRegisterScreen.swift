import Charts
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// One account's register (R-ACC-2, design/mockups/register.html): its
/// balance over time, filters, and every transaction.
struct AccountRegisterScreen: View {
	@Bindable var workspace: WorkspaceModel
	let accountID: String
	@State private var register: RegisterModel
	@State private var accounts: AccountsModel
	@State private var editing: TransactionEditorModel?
	@State private var deleting: [RegisterRow] = []
	@State private var reconcile: ReconcileModel?
	@State private var search = ""

	init(workspace: WorkspaceModel, accountID: String, register: RegisterModel? = nil) {
		self.workspace = workspace
		self.accountID = accountID
		_register = State(initialValue: register ?? RegisterModel(workspace: workspace, scope: .account(accountId: accountID)))
		_accounts = State(initialValue: AccountsModel(workspace: workspace))
	}

	private var account: AccountDetail? { accounts.account(accountID) }

	@ToolbarContentBuilder private var toolbar: some ToolbarContent {
			ToolbarItemGroup(placement: .primaryAction) {
				Button {
					reconcile = ReconcileModel(workspace: workspace, accountID: accountID)
					register.isReconciling = true
				} label: {
					Label("Reconcile", systemImage: "checkmark.seal")
				}
				.disabled(reconcile != nil || register.isReadOnly)
				.accessibilityIdentifier("register.reconcile")
				Button {
					workspace.perform(.moveMoney)
				} label: {
					Label("Move money", systemImage: "arrow.left.arrow.right")
				}
				.disabled(register.isReadOnly)
				Button {
					editing = TransactionEditorModel(workspace: workspace, accountID: accountID)
				} label: {
					Label("Add transaction", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(register.isReadOnly)
				.help("Add transaction (⌘N)")
				.accessibilityIdentifier("register.add")
			}
			}

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			if let reconcile {
				ReconcileBar(model: reconcile, format: register.format) {
					self.reconcile = nil
					register.isReconciling = false
				}
			}
			VStack(alignment: .leading, spacing: Spacing.s3) {
				BalanceGraph(rows: register.loadedRows, format: register.format, shows: register.showsBalance)
					.frame(height: 140)
				RegisterFilters(register: register, search: $search)
			}
			.padding(.horizontal, Spacing.s5)
			.padding(.top, Spacing.s2)
			.padding(.bottom, Spacing.s3)
			RegisterTable(model: register) { row in
				editing = TransactionEditorModel(workspace: workspace, accountID: accountID, editing: row)
			} delete: { rows in
				deleting = rows
			}
		}
		.background(.surfaceWindow)
		.navigationTitle(account?.name ?? "Account")
		.navigationSubtitle(account.map { register.format.currency($0.total) } ?? "")
		.toolbar { toolbar }
		.sheet(item: $editing) { TransactionSheet(model: $0) }
		.confirmationDialog(deleting.count == 1 ? "Delete this transaction?" : "Delete \(deleting.count) transactions?", isPresented: Binding(get: { !deleting.isEmpty }, set: { if !$0 { deleting = [] } })) {
			Button("Delete", role: .destructive) {
				let ids = deleting.map(\.id)
				Task { await register.delete(ids) }
			}
		} message: {
			if deleting.contains(where: \.isTransfer) {
				Text("A transfer's other side goes too.")
			}
		}
		.task(id: workspace.changeCount) {
			await accounts.load()
			await register.load()
			await reconcile?.refresh()
		}
		.containerIdentifier("screen.register")
	}
}

/// Date range, status and search, as the register mockup lays them out.
private struct RegisterFilters: View {
	let register: RegisterModel
	@Binding var search: String

	enum Range: String, CaseIterable {
		case all, last30, thisMonth, lastMonth, thisYear

		var title: String {
			switch self {
			case .all: "All dates"
			case .last30: "Last 30 days"
			case .thisMonth: "This month"
			case .lastMonth: "Last month"
			case .thisYear: "This year"
			}
		}

		func bounds(today: Date = .now) -> (String?, String?) {
			let calendar = Calendar.current
			let civil = TransactionEditorModel.civilDate
			switch self {
			case .all: return (nil, nil)
			case .last30: return (civil(calendar.date(byAdding: .day, value: -30, to: today) ?? today), civil(today))
			case .thisMonth:
				let month = Month(date: civil(today)) ?? Month(year: 2026, month: 1)
				return (month.firstDay, month.lastDay)
			case .lastMonth:
				let month = (Month(date: civil(today)) ?? Month(year: 2026, month: 1)).adding(months: -1)
				return (month.firstDay, month.lastDay)
			case .thisYear:
				let year = calendar.component(.year, from: today)
				return (String(format: "%04d-01-01", year), civil(today))
			}
		}
	}

	private var range: Range {
		Range.allCases.first { $0.bounds() == (register.filter.from, register.filter.to) } ?? .all
	}

	var body: some View {
		HStack(spacing: Spacing.s2) {
			Picker("Dates", selection: Binding(get: { range }, set: { choice in
				let (from, to) = choice.bounds()
				Task { await register.setFilter { $0.from = from; $0.to = to } }
			})) {
				ForEach(Range.allCases, id: \.self) { Text($0.title).tag($0) }
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("register.range")
			Picker("Status", selection: Binding(get: { register.filter.status }, set: { status in
				Task { await register.setFilter { $0.status = status } }
			})) {
				Text("All transactions").tag(ClearedStatus?.none)
				Text("Uncleared").tag(ClearedStatus?.some(.uncleared))
				Text("Cleared").tag(ClearedStatus?.some(.cleared))
				Text("Reconciled").tag(ClearedStatus?.some(.reconciled))
			}
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("register.status")
			TextField("Search", text: $search, prompt: Text("Payee, envelope or notes"))
				.textFieldStyle(.roundedBorder)
				.frame(maxWidth: 260)
				.onSubmit { Task { await register.setFilter { $0.search = search.isEmpty ? nil : search } } }
				.onChange(of: search) {
					if search.isEmpty { Task { await register.setFilter { $0.search = nil } } }
				}
				.accessibilityIdentifier("register.search")
			Spacer()
			if !register.isUnfiltered {
				Text(register.total == 1 ? "1 transaction" : "\(register.total.formatted()) transactions")
					.foregroundStyle(.textSecondary)
					.accessibilityIdentifier("register.count")
			}
		}
	}
}

/// The running balance of the newest rows, as in the register mockup.
private struct BalanceGraph: View {
	let rows: [RegisterRow]
	let format: MoneyFormat
	let shows: Bool

	private var points: [(date: String, balance: Int64)] {
		guard shows else { return [] }
		let recent = rows.prefix(60).compactMap { row in row.runningBalance.map { (row.date, $0) } }
		return recent.reversed()
	}

	var body: some View {
		let points = points
		if points.count > 1, let first = points.first, let last = points.last {
			ZStack(alignment: .bottomTrailing) {
				Chart(Array(points.enumerated()), id: \.offset) { index, point in
					AreaMark(x: .value("Day", index), y: .value("Balance", Double(point.balance) / 100))
						.foregroundStyle(Color.textPrimary.opacity(0.06))
					LineMark(x: .value("Day", index), y: .value("Balance", Double(point.balance) / 100))
						.foregroundStyle(Color.textPrimary)
				}
				.chartXAxis(.hidden)
				.chartYAxis(.hidden)
				.chartYScale(domain: .automatic(includesZero: false))
				Text("\(RegisterRow.short(first.date)) – \(RegisterRow.short(last.date))")
					.font(.text(.caption))
					.foregroundStyle(.textSecondary)
			}
			.accessibilityElement(children: .ignore)
			.accessibilityLabel("Balance from \(RegisterRow.short(first.date)), \(format.amount(first.balance)), to \(RegisterRow.short(last.date)), \(format.amount(last.balance))")
		} else {
			Color.clear
		}
	}
}

extension RegisterRow {
	static func short(_ date: String) -> String {
		let parts = date.split(separator: "-").compactMap { Int($0) }
		guard parts.count == 3, let value = Calendar.current.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2])) else { return date }
		return value.formatted(.dateTime.month(.abbreviated).day())
	}
}

/// Statement balance against what's cleared (R-ACC-3). Rows are checked off
/// in the register; Finish locks them once the difference is zero.
struct ReconcileBar: View {
	let model: ReconcileModel
	let format: MoneyFormat
	let done: () -> Void
	@State private var statement: AmountFieldModel

	init(model: ReconcileModel, format: MoneyFormat, done: @escaping () -> Void) {
		self.model = model
		self.format = format
		self.done = done
		_statement = State(initialValue: AmountFieldModel(amount: 0, style: .commaDot, reader: CoreAmountReader()))
	}

	var body: some View {
		HStack(spacing: Spacing.s4) {
			Text("Statement balance")
				.font(.text(.body, weight: .medium))
			AmountTextField(field: statement, takesFocus: true) { exit in
				if exit == .done || exit == .next { check() }
				if exit == .cancel { done() }
			}
			.frame(width: 130)
			.accessibilityIdentifier("reconcile.statement")
			Button("Check") { check() }
				.accessibilityIdentifier("reconcile.check")
			if let result = model.result {
				LabeledFigure(title: "Cleared", value: format.amount(result.cleared))
				LabeledFigure(title: "Difference", value: format.amount(result.difference))
					.foregroundStyle(result.difference == 0 ? Color.moneyAvailable : Color.moneyOverspent)
					.accessibilityIdentifier("reconcile.difference")
			}
			Spacer()
			if let result = model.result, result.difference != 0 {
				Button("Add Adjustment") { Task { await model.addAdjustment() } }
					.help("Adds a cleared transaction for the difference")
					.accessibilityIdentifier("reconcile.adjust")
			}
			Button("Finish") {
				Task { if await model.finish() { done() } }
			}
			.buttonStyle(.borderedProminent)
			.disabled(!model.isBalanced)
			.accessibilityIdentifier("reconcile.finish")
			Button("Cancel", role: .cancel) { done() }
		}
		.padding(.horizontal, Spacing.s5)
		.padding(.vertical, Spacing.s3)
		.background(Color.accentSoft)
		.containerIdentifier("reconcile.bar")
	}

	private func check() {
		Task {
			if let amount = await statement.commit() { await model.check(amount) }
		}
	}
}

private struct LabeledFigure: View {
	let title: String
	let value: String

	var body: some View {
		HStack(spacing: Spacing.s1) {
			Text(title).foregroundStyle(.textSecondary)
			Text(value).font(.figures(.body, weight: .semibold))
		}
		.accessibilityElement(children: .combine)
	}
}

