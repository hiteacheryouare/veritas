import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Every account with its cleared, uncleared and total balance (R-ACC-1):
/// on budget, off budget, then closed.
struct AccountsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: AccountsModel
	@State private var isCreating = false
	@State private var isManagingPayees = false
	@State private var renaming: AccountDetail?
	@State private var noting: AccountDetail?
	@State private var closing: AccountDetail?

	init(workspace: WorkspaceModel, model: AccountsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? AccountsModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s5) {
				section("On budget", model.onBudget)
				section("Off budget", model.offBudget)
				section("Closed", model.closed)
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.toolbar {
			ToolbarItemGroup(placement: .primaryAction) {
				Button {
					isManagingPayees = true
				} label: {
					Label("Payees", systemImage: "person.2")
				}
				.accessibilityIdentifier("accounts.payees")
				Button {
					isCreating = true
				} label: {
					Label("New account", systemImage: "plus")
				}
				.labelStyle(.titleAndIcon)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("accounts.new")
			}
		}
		.sheet(isPresented: $isCreating) { NewAccountSheet(model: model) }
		.sheet(isPresented: $isManagingPayees) { PayeesSheet(model: PayeesModel(workspace: workspace)) }
		.sheet(item: $closing) { CloseAccountSheet(model: model, account: $0) }
		.popover(item: $renaming) { account in
			NamePrompt(title: "Rename account", action: "Rename", name: account.name) { name in
				await model.rename(account.id, to: name)
			}
		}
		.popover(item: $noting) { account in
			NotePrompt(text: account.note ?? "") { text in await model.setNote(account.id, text) }
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.accounts")
	}

	@ViewBuilder private func section(_ title: String, _ accounts: [AccountDetail]) -> some View {
		if !accounts.isEmpty {
			VStack(alignment: .leading, spacing: Spacing.s2) {
				SectionTitle(title)
				GroupedRows {
					HStack {
						Text("")
						Spacer()
						heading("Cleared")
						heading("Uncleared")
						heading("Balance")
					}
					.padding(.horizontal, Spacing.s4)
					.frame(height: 26)
					ForEach(accounts) { account in
						Divider().padding(.leading, Spacing.s4)
						AccountRow(account: account, format: model.format) {
							workspace.go(to: .account(id: account.id))
						}
						.contextMenu { menu(account) }
					}
				}
			}
		}
	}

	private func heading(_ text: String) -> some View {
		Text(text)
			.font(.text(.caption))
			.foregroundStyle(.textTertiary)
			.frame(width: 110, alignment: .trailing)
	}

	@ViewBuilder private func menu(_ account: AccountDetail) -> some View {
		Button("Open Register") { workspace.go(to: .account(id: account.id)) }
		Divider()
		Button("Rename…") { renaming = account }
		Button(account.note == nil ? "Add Note…" : "Edit Note…") { noting = account }
		if !account.closed {
			Button("Move Up") { Task { await model.move(account.id, by: -1) } }
				.disabled(!model.canMove(account.id, by: -1))
			Button("Move Down") { Task { await model.move(account.id, by: 1) } }
				.disabled(!model.canMove(account.id, by: 1))
			Divider()
			Button(account.offBudget ? "Move On Budget" : "Move Off Budget") {
				Task { await model.setOffBudget(account.id, !account.offBudget) }
			}
			Button("Close Account…") { closing = account }
		} else {
			Button("Reopen Account") { Task { await model.reopen(account.id) } }
		}
	}
}

private struct AccountRow: View {
	let account: AccountDetail
	let format: MoneyFormat
	let open: () -> Void

	var body: some View {
		Button(action: open) {
			HStack {
				VStack(alignment: .leading, spacing: 2) {
					Text(account.name)
						.font(.text(.body, weight: .medium))
					if let note = account.note {
						Text(note)
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
							.lineLimit(1)
					}
				}
				Spacer()
				figure(account.cleared, weight: .regular)
				figure(account.uncleared, weight: .regular)
				figure(account.total, weight: .semibold)
			}
			.padding(.horizontal, Spacing.s4)
			.frame(minHeight: Size.row)
			.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.foregroundStyle(account.closed ? Color.textSecondary : Color.textPrimary)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("\(account.name), balance \(format.amount(account.total)), cleared \(format.amount(account.cleared)), uncleared \(format.amount(account.uncleared))")
		.accessibilityAddTraits(.isButton)
		.accessibilityIdentifier("account.\(account.id)")
	}

	private func figure(_ amount: Int64, weight: Font.Weight) -> some View {
		Text(format.amount(amount))
			.font(.figures(.body, weight: weight))
			.frame(width: 110, alignment: .trailing)
	}
}

private struct NotePrompt: View {
	@State var text: String
	let save: (String) async -> Bool
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			Text("Note").font(.text(.heading, weight: .semibold))
			TextField("Note", text: $text, axis: .vertical)
				.lineLimit(3...6)
				.frame(width: 280)
				.accessibilityIdentifier("accountNote.text")
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
				Button("Save") { Task { if await save(text) { dismiss() } } }
					.keyboardShortcut(.defaultAction)
			}
		}
		.padding(Spacing.s4)
	}
}

/// A new account: name, kind, and what's in it today (or owed, for debt).
private struct NewAccountSheet: View {
	let model: AccountsModel
	@State private var name = ""
	@State private var kind = NewAccountKind.checking
	@State private var balance = AmountFieldModel(amount: 0, style: .commaDot, reader: CoreAmountReader())
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("New account").font(.text(.title, weight: .semibold))
			GroupedRows {
				LabeledRow("Name") {
					TextField("Name", text: $name)
						.labelsHidden()
						.textFieldStyle(.plain)
						.multilineTextAlignment(.trailing)
						.frame(width: 220)
						.accessibilityIdentifier("newAccount.name")
				}
				Divider()
				LabeledRow("Kind") {
					Picker("Kind", selection: $kind) {
						ForEach(NewAccountKind.allCases, id: \.self) { Text($0.title).tag($0) }
					}
					.labelsHidden()
					.frame(width: 200)
				}
				Divider()
				LabeledRow(kind.isOwed ? "Owed today" : "Balance today") {
					AmountTextField(field: balance, takesFocus: false) { _ in }
						.frame(width: 120)
				}
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Add Account") {
					Task {
						guard let amount = await balance.commit() else { return }
						if await model.create(name: name.trimmingCharacters(in: .whitespaces), kind: kind, startingBalance: amount) { dismiss() }
					}
				}
				.buttonStyle(.borderedProminent)
				.keyboardShortcut(.defaultAction)
				.disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
				.accessibilityIdentifier("newAccount.add")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 460)
		.containerIdentifier("sheet.newAccount")
	}
}

/// Actual's close: a balance moves to another account as a "Closing account"
/// transfer, with an envelope when it leaves the budget.
private struct CloseAccountSheet: View {
	let model: AccountsModel
	let account: AccountDetail
	@State private var destination: String?
	@State private var category: String?
	@State private var envelopes: [EnvelopeGroup] = []
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Close \(account.name)?").font(.text(.title, weight: .semibold))
			if account.total != 0 {
				Text("\(model.format.currency(account.total)) is in it. Move it to")
				Picker("Move it to", selection: $destination) {
					Text("Choose an account").tag(String?.none)
					ForEach(model.open.filter { $0.id != account.id }) { Text($0.name).tag(String?.some($0.id)) }
				}
				.labelsHidden()
				.accessibilityIdentifier("closeAccount.destination")
				if model.closingNeedsCategory(account.id, into: destination) {
					LabeledRow("Envelope") {
						EnvelopeMenu(groups: envelopes, selection: $category)
					}
				}
			}
			HStack {
				Spacer()
				Button("Cancel", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
				Button("Close Account", role: .destructive) {
					Task { if await model.close(account.id, transferTo: destination, category: category) { dismiss() } }
				}
				.disabled(account.total != 0 && destination == nil)
				.accessibilityIdentifier("closeAccount.close")
			}
		}
		.padding(Spacing.s5)
		.frame(width: 440)
		.task {
			envelopes = await model.envelopes()
		}
		.containerIdentifier("sheet.closeAccount")
	}
}

/// Rename, merge, favorite and category learning (R-ACC-5).
struct PayeesSheet: View {
	@State var model: PayeesModel
	@State private var renaming: PayeeDetail?
	@State private var merging = false
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			HStack {
				Text("Payees").font(.text(.title, weight: .semibold))
				Spacer()
				TextField("Search", text: $model.search, prompt: Text("Search"))
					.textFieldStyle(.roundedBorder)
					.frame(width: 200)
			}
			Table(model.shown, selection: $model.selection) {
				TableColumn("Name") { payee in
					Text(payee.transferAccountId == nil ? payee.name : "\(payee.name) (account)")
						.foregroundStyle(payee.transferAccountId == nil ? Color.textPrimary : Color.textSecondary)
				}
				TableColumn("Transactions") { payee in
					Text(payee.transactions.formatted()).monospacedDigit().frame(maxWidth: .infinity, alignment: .trailing)
				}
				.width(100)
				TableColumn("Favorite") { payee in
					Toggle("Favorite", isOn: Binding(get: { payee.favorite }, set: { value in Task { await model.setFavorite(payee.id, value) } }))
						.labelsHidden()
						.disabled(payee.transferAccountId != nil || model.isReadOnly)
				}
				.width(70)
				TableColumn("Learns envelope") { payee in
					Toggle("Learns envelope", isOn: Binding(get: { payee.learnCategories }, set: { value in Task { await model.setLearnCategories(payee.id, value) } }))
						.labelsHidden()
						.disabled(payee.transferAccountId != nil || model.isReadOnly)
				}
				.width(110)
			}
			.contextMenu(forSelectionType: String.self) { ids in
				if ids.count == 1, let id = ids.first, let payee = model.payees.first(where: { $0.id == id }), payee.transferAccountId == nil {
					Button("Rename…") { renaming = payee }
				}
				if model.mergeable.count > 1 {
					Button("Merge \(model.mergeable.count) Payees…") { merging = true }
				}
			}
			.frame(minHeight: 360)
			.accessibilityIdentifier("payees.table")
			HStack {
				if model.mergeable.count > 1 {
					Button("Merge \(model.mergeable.count) Payees…") { merging = true }
						.accessibilityIdentifier("payees.merge")
				}
				Spacer()
				Button("Done") { dismiss() }
					.keyboardShortcut(.defaultAction)
			}
		}
		.padding(Spacing.s5)
		.frame(width: 640)
		.popover(item: $renaming) { payee in
			NamePrompt(title: "Rename payee", action: "Rename", name: payee.name) { name in
				await model.rename(payee.id, to: name)
			}
		}
		.confirmationDialog("Merge into which payee?", isPresented: $merging) {
			ForEach(model.mergeable) { payee in
				Button("Keep \(payee.name)") { Task { await model.merge(into: payee.id) } }
			}
		} message: {
			Text("The others' transactions move to the one you keep.")
		}
		.task { await model.load() }
		.containerIdentifier("sheet.payees")
	}
}
