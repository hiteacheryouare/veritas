import SwiftUI
import VeritasDesign
import VeritasKit

/// Budget, Accounts with their balances, Bills, Goals, Reports and Insights,
/// with Settings kept at the bottom.
struct SidebarView: View {
	@Bindable var model: WorkspaceModel

	private var selection: Binding<Destination?> {
		Binding(get: { model.destination }, set: { if let destination = $0 { model.go(to: destination) } })
	}

	var body: some View {
		List(selection: selection) {
			SectionRow(screen: .budget)
			SectionRow(screen: .accounts)
			ForEach(model.overview?.accountGroups ?? []) { group in
				AccountGroupRows(group: group, format: model.format)
			}
			SectionRow(screen: .bills)
			SectionRow(screen: .goals)
			SectionWithPages(section: .reports, current: model.destination)
			SectionWithPages(section: .insights, current: model.destination)
		}
		.listStyle(.sidebar)
		.safeAreaInset(edge: .bottom, spacing: 0) {
			SettingsRow(isCurrent: model.destination == .screen(.settings)) {
				model.go(to: .screen(.settings))
			}
		}
	}
}

private struct SectionRow: View {
	let screen: Screen

	var body: some View {
		Label(screen.title, systemImage: screen.symbol)
			.tag(Destination.screen(screen))
			.accessibilityIdentifier("sidebar.\(screen.rawValue)")
	}
}

/// A section's pages show while you're in that section, as in the mockups.
private struct SectionWithPages: View {
	let section: Screen
	let current: Destination

	var body: some View {
		SectionRow(screen: section)
		if current.section == section {
			ForEach(section.pages, id: \.self) { page in
				Text(page.title)
					.padding(.leading, 26)
					.foregroundStyle(current == .screen(page) ? .textPrimary : .textSecondary)
					.tag(Destination.screen(page))
					.accessibilityIdentifier("sidebar.\(page.rawValue)")
			}
		}
	}
}

private struct AccountGroupRows: View {
	let group: AccountGroup
	let format: MoneyFormat

	var body: some View {
		Text(group.name)
			.font(.text(.caption, weight: .semibold))
			.foregroundStyle(.textTertiary)
			.padding(.leading, 26)
			.padding(.top, Spacing.s1)
			.selectionDisabled()
			.accessibilityAddTraits(.isHeader)
		ForEach(group.accounts) { account in
			AccountRow(account: account, format: format)
				.tag(Destination.account(id: account.id))
		}
	}
}

private struct AccountRow: View {
	let account: AccountSummary
	let format: MoneyFormat

	var body: some View {
		HStack {
			Text(account.name)
				.foregroundStyle(.textSecondary)
			Spacer(minLength: Spacing.s2)
			Text(format.amount(account.balance))
				.font(.figures(.body))
				.foregroundStyle(.textPrimary)
		}
		.padding(.leading, 26)
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("sidebar.account.\(account.id)")
	}
}

private struct SettingsRow: View {
	let isCurrent: Bool
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			Label(Screen.settings.title, systemImage: Screen.settings.symbol)
				.frame(maxWidth: .infinity, alignment: .leading)
				.padding(.horizontal, Spacing.s2)
				.frame(height: 30)
				.background(isCurrent ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.fontWeight(isCurrent ? .medium : .regular)
		.padding(.horizontal, Spacing.s2)
		.padding(.bottom, Spacing.s3)
		.accessibilityIdentifier("sidebar.settings")
		.accessibilityAddTraits(isCurrent ? .isSelected : [])
	}
}
