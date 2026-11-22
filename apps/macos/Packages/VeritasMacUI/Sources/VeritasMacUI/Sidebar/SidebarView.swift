import SwiftUI
import VeritasDesign
import VeritasKit

/// Budget, Accounts with their balances, Bills, Goals, Reports and Insights,
/// with Settings kept at the bottom.
///
/// The rows draw their own selection: a neutral fill with primary text, so the
/// selected row reads the same whatever accent the app has (WCAG AA in both
/// appearances). ↑ and ↓ move through the rows while the sidebar has focus.
struct SidebarView: View {
	@Bindable var model: WorkspaceModel
	@FocusState private var hasFocus: Bool

	/// Every row that goes somewhere, top to bottom, for the arrow keys.
	private var order: [Destination] {
		var rows: [Destination] = [.screen(.budget), .screen(.accounts)]
		rows += (model.overview?.accountGroups ?? []).flatMap { $0.accounts.map { .account(id: $0.id) } }
		rows.append(.screen(.bills))
		if model.destination.section == .bills { rows += Screen.bills.pages.map { .screen($0) } }
		rows.append(.screen(.goals))
		rows.append(.screen(.reports))
		if model.destination.section == .reports { rows += Screen.reports.pages.map { .screen($0) } }
		rows += Screen.insights.pages.map { .screen($0) }
		return rows
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: 2) {
				SectionRow(screen: .budget, model: model)
				SectionRow(screen: .accounts, model: model)
				ForEach(model.overview?.accountGroups ?? []) { group in
					SidebarGroupRows(group: group, model: model)
				}
				SectionWithPages(section: .bills, model: model)
				SectionRow(screen: .goals, model: model)
				SectionWithPages(section: .reports, model: model)
				InsightsRows(model: model)
			}
			.padding(Spacing.s2)
		}
		.scrollIndicators(.never)
		.focusable()
		.focusEffectDisabled()
		.focused($hasFocus)
		.onKeyPress(keys: [.upArrow, .downArrow]) { press in
			guard let index = order.firstIndex(of: model.destination) else {
				model.go(to: order.first ?? .start)
				return .handled
			}
			let next = index + (press.key == .upArrow ? -1 : 1)
			if order.indices.contains(next) { model.go(to: order[next]) }
			return .handled
		}
		.environment(\.sidebarHasFocus, hasFocus)
		.safeAreaInset(edge: .bottom, spacing: 0) {
			VStack(spacing: 0) {
				Divider()
				SidebarRow(isCurrent: model.destination == .screen(.settings), identifier: "sidebar.settings") {
					model.go(to: .screen(.settings))
				} label: {
					Label(Screen.settings.title, systemImage: Screen.settings.symbol)
				}
				.padding(Spacing.s2)
			}
			.background(.bar)
		}
	}
}

extension EnvironmentValues {
	@Entry var sidebarHasFocus = false
}

/// One row that goes somewhere. The current row gets the neutral selection
/// fill, and a focus ring while the sidebar has keyboard focus and the Mac's
/// keyboard navigation is on, so a click never leaves a ring behind.
private struct SidebarRow<Content: View>: View {
	let isCurrent: Bool
	let identifier: String
	var indent: CGFloat = 0
	let action: () -> Void
	@ViewBuilder let label: Content
	@Environment(\.sidebarHasFocus) private var sidebarHasFocus

	var body: some View {
		Button(action: action) {
			label
				.frame(maxWidth: .infinity, alignment: .leading)
				.padding(.leading, Spacing.s2 + indent)
				.padding(.trailing, Spacing.s2)
				.frame(minHeight: 28)
				.background(isCurrent ? AnyShapeStyle(.surfaceSelected) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
				.overlay {
					if isCurrent, sidebarHasFocus, NSApplication.shared.isFullKeyboardAccessEnabled {
						RoundedRectangle(cornerRadius: Radius.control).strokeBorder(Color.focusRing, lineWidth: 2)
					}
				}
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.foregroundStyle(isCurrent ? Color.textPrimary : Color.textSecondary)
		.fontWeight(isCurrent ? .medium : .regular)
		.motion(.fast, value: isCurrent)
		.accessibilityAddTraits(isCurrent ? .isSelected : [])
		.accessibilityIdentifier(identifier)
	}
}

private struct SectionRow: View {
	let screen: Screen
	let model: WorkspaceModel

	var body: some View {
		SidebarRow(isCurrent: model.destination == .screen(screen), identifier: "sidebar.\(screen.rawValue)") {
			model.go(to: .screen(screen))
		} label: {
			Label(screen.title, systemImage: screen.symbol)
				.foregroundStyle(Color.textPrimary)
		}
	}
}

/// A section's pages show while you're in that section, as in the mockups.
private struct SectionWithPages: View {
	let section: Screen
	let model: WorkspaceModel

	var body: some View {
		SectionRow(screen: section, model: model)
		if model.destination.section == section {
			ForEach(section.pages, id: \.self) { page in
				PageRow(page: page, model: model)
			}
		}
	}
}

private struct PageRow: View {
	let page: Screen
	let model: WorkspaceModel

	var body: some View {
		SidebarRow(isCurrent: model.destination == .screen(page), identifier: "sidebar.\(page.rawValue)", indent: 26) {
			model.go(to: .screen(page))
		} label: {
			Text(page.title)
		}
	}
}

/// Insights has no page of its own, so it heads its pages and they always show.
private struct InsightsRows: View {
	let model: WorkspaceModel

	var body: some View {
		Label(Screen.insights.title, systemImage: Screen.insights.symbol)
			.foregroundStyle(Color.textPrimary)
			.padding(.horizontal, Spacing.s2)
			.frame(minHeight: 28)
			.accessibilityAddTraits(.isHeader)
			.accessibilityIdentifier("sidebar.insights")
		ForEach(Screen.insights.pages, id: \.self) { page in
			PageRow(page: page, model: model)
		}
	}
}

private struct SidebarGroupRows: View {
	let group: SidebarGroup
	let model: WorkspaceModel

	var body: some View {
		Text(group.name)
			.font(.text(.caption, weight: .semibold))
			.foregroundStyle(.textTertiary)
			.padding(.leading, Spacing.s2 + 26)
			.padding(.top, Spacing.s1)
			.accessibilityAddTraits(.isHeader)
		ForEach(group.accounts) { account in
			AccountRow(account: account, model: model)
		}
	}
}

private struct AccountRow: View {
	let account: SidebarAccount
	let model: WorkspaceModel

	private var isCurrent: Bool { model.destination == .account(id: account.id) }

	var body: some View {
		SidebarRow(isCurrent: isCurrent, identifier: "sidebar.account.\(account.id)", indent: 26) {
			model.go(to: .account(id: account.id))
		} label: {
			HStack {
				Text(account.name)
					.lineLimit(1)
					.foregroundStyle(isCurrent ? Color.textPrimary : (account.isInactive ? Color.textTertiary : Color.textSecondary))
				Spacer(minLength: Spacing.s2)
				Text(model.format.amount(account.balance, on: .register))
					.font(.figures(.body))
					.foregroundStyle(account.isInactive && !isCurrent ? Color.textTertiary : Color.textPrimary)
			}
			.accessibilityElement(children: .combine)
		}
	}
}
