import AppKit
import SwiftUI
import VeritasDesign
import VeritasKit

/// Budget, Accounts with their balances, Bills, Goals, Reports and Insights,
/// with Settings kept at the bottom.
///
/// A native sidebar list, so arrows, type-select, Home and End, Page keys,
/// scrolling to the selection, one Tab stop and VoiceOver's rows are the
/// system's. Its highlight is not: the system's follows the accent, and with a
/// light accent it put white text on light grey. The list's table draws no
/// highlight, and the current row draws the neutral `surface-selected` fill
/// with primary text instead, whatever accent the app has.
struct SidebarView: View {
	@Bindable var model: WorkspaceModel

	private var selection: Binding<Destination?> {
		Binding(get: { model.destination }, set: { if let destination = $0 { model.go(to: destination) } })
	}

	var body: some View {
		List(selection: selection) {
			SectionRow(screen: .budget, current: model.destination)
			SectionRow(screen: .accounts, current: model.destination)
			ForEach(model.overview?.accountGroups ?? []) { group in
				SidebarGroupRows(group: group, format: model.format, current: model.destination)
			}
			SectionWithPages(section: .bills, current: model.destination)
			SectionRow(screen: .goals, current: model.destination)
			SectionWithPages(section: .reports, current: model.destination)
			InsightsRows(current: model.destination)
		}
		.listStyle(.sidebar)
		.background(NoSystemHighlight())
		.safeAreaInset(edge: .bottom, spacing: 0) {
			SettingsRow(isCurrent: model.destination == .screen(.settings)) {
				model.go(to: .screen(.settings))
			}
			.padding(.top, Spacing.s2)
			.background(.bar)
		}
	}
}

/// The current row's fill: neutral, inset like the system's highlight.
private struct CurrentRowFill: ViewModifier {
	let isCurrent: Bool

	func body(content: Content) -> some View {
		content.listRowBackground(
			isCurrent ? RoundedRectangle(cornerRadius: Radius.control).fill(.surfaceSelected).padding(.horizontal, Spacing.s2) : nil
		)
	}
}

/// Finds the list's own table and turns its highlight off, so only
/// `CurrentRowFill` marks the current row. Selection itself, and every key and
/// accessibility behaviour that comes with it, stays the table's.
struct NoSystemHighlight: NSViewRepresentable {
	func makeNSView(context: Context) -> NSView { Finder() }
	func updateNSView(_ view: NSView, context: Context) {
		(view as? Finder)?.apply()
	}

	/// Sits behind the List, so it has the List's frame. It accepts only a table
	/// whose scroll view spans exactly the List's columns and covers its frame
	/// (the scroll view also runs under the pinned Settings row), so a screen's
	/// table, such as the register, is never touched. Until the List's table
	/// exists it does nothing, and tries again on the next layout pass or update.
	final class Finder: NSView {
		override func viewDidMoveToWindow() {
			super.viewDidMoveToWindow()
			apply()
			DispatchQueue.main.async { [weak self] in self?.apply() }
		}

		override func layout() {
			super.layout()
			apply()
		}

		func apply() {
			guard let table = listTable(), table.selectionHighlightStyle != .none else { return }
			table.selectionHighlightStyle = .none
		}

		func listTable() -> NSTableView? {
			guard !bounds.isEmpty else { return nil }
			let root = window?.contentView ?? Self.root(of: self)
			let mine = convert(bounds, to: nil)
			return Self.tables(in: root).first { table in
				guard let scroll = table.enclosingScrollView else { return false }
				let theirs = scroll.convert(scroll.bounds, to: nil)
				return abs(theirs.minX - mine.minX) <= 1 && abs(theirs.width - mine.width) <= 1
					&& theirs.insetBy(dx: -1, dy: -1).contains(mine)
			}
		}

		private static func root(of view: NSView) -> NSView {
			var top = view
			while let parent = top.superview { top = parent }
			return top
		}

		private static func tables(in view: NSView) -> [NSTableView] {
			(view as? NSTableView).map { [$0] } ?? view.subviews.flatMap(tables)
		}
	}
}

private struct SectionRow: View {
	let screen: Screen
	let current: Destination

	var body: some View {
		Label(screen.title, systemImage: screen.symbol)
			.foregroundStyle(Color.textPrimary)
			.fontWeight(current == .screen(screen) ? .medium : .regular)
			.modifier(CurrentRowFill(isCurrent: current == .screen(screen)))
			.tag(Destination.screen(screen))
			.accessibilityIdentifier("sidebar.\(screen.rawValue)")
	}
}

/// A section's pages show while you're in that section, as in the mockups.
private struct SectionWithPages: View {
	let section: Screen
	let current: Destination

	var body: some View {
		SectionRow(screen: section, current: current)
		if current.section == section {
			ForEach(section.pages, id: \.self) { page in
				PageRow(page: page, current: current)
			}
		}
	}
}

private struct PageRow: View {
	let page: Screen
	let current: Destination

	var body: some View {
		let isCurrent = current == .screen(page)
		Text(page.title)
			.padding(.leading, 26)
			.foregroundStyle(isCurrent ? Color.textPrimary : Color.textSecondary)
			.fontWeight(isCurrent ? .medium : .regular)
			.modifier(CurrentRowFill(isCurrent: isCurrent))
			.tag(Destination.screen(page))
			.accessibilityIdentifier("sidebar.\(page.rawValue)")
	}
}

/// Insights has no page of its own, so it heads its pages, which always show.
private struct InsightsRows: View {
	let current: Destination

	var body: some View {
		Label(Screen.insights.title, systemImage: Screen.insights.symbol)
			.foregroundStyle(Color.textPrimary)
			.selectionDisabled()
			.accessibilityAddTraits(.isHeader)
			.accessibilityIdentifier("sidebar.insights")
		ForEach(Screen.insights.pages, id: \.self) { page in
			PageRow(page: page, current: current)
		}
	}
}

private struct SidebarGroupRows: View {
	let group: SidebarGroup
	let format: MoneyFormat
	let current: Destination

	var body: some View {
		Text(group.name)
			.font(.text(.caption, weight: .semibold))
			.foregroundStyle(.textTertiary)
			.padding(.leading, 26)
			.padding(.top, Spacing.s1)
			.selectionDisabled()
			.accessibilityAddTraits(.isHeader)
		ForEach(group.accounts) { account in
			AccountRow(account: account, format: format, isCurrent: current == .account(id: account.id))
				.tag(Destination.account(id: account.id))
		}
	}
}

private struct AccountRow: View {
	let account: SidebarAccount
	let format: MoneyFormat
	let isCurrent: Bool

	var body: some View {
		HStack {
			Text(account.name)
				.foregroundStyle(isCurrent ? Color.textPrimary : (account.isInactive ? Color.textTertiary : Color.textSecondary))
			Spacer(minLength: Spacing.s2)
			Text(format.amount(account.balance, on: .register))
				.font(.figures(.body))
				.foregroundStyle(account.isInactive && !isCurrent ? Color.textTertiary : Color.textPrimary)
		}
		.fontWeight(isCurrent ? .medium : .regular)
		.padding(.leading, 26)
		.modifier(CurrentRowFill(isCurrent: isCurrent))
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
				.background(isCurrent ? AnyShapeStyle(.surfaceSelected) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
				.contentShape(.rect)
		}
		.buttonStyle(.plain)
		.foregroundStyle(Color.textPrimary)
		.fontWeight(isCurrent ? .medium : .regular)
		.padding(.horizontal, Spacing.s2)
		.padding(.bottom, Spacing.s3)
		.accessibilityIdentifier("sidebar.settings")
		.accessibilityAddTraits(isCurrent ? .isSelected : [])
	}
}
