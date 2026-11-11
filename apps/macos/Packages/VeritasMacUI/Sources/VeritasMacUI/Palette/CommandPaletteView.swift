import SwiftUI
import VeritasDesign
import VeritasKit

/// ⌘K. Type to narrow, ↑ and ↓ to pick, Return to go, Esc to close.
/// It's a sheet, so it opens as its own key window with the field focused.
struct CommandPaletteView: View {
	let index: PaletteIndex
	let format: MoneyFormat
	let choose: (PaletteItem) -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var query = ""
	@State private var highlighted: PaletteItem.ID?
	@FocusState private var isFieldFocused: Bool

	private var results: [PaletteItem] {
		index.results(for: query)
	}

	var body: some View {
		VStack(spacing: 0) {
			field
			Divider()
			PaletteResults(results: results, format: format, highlighted: $highlighted, activate: choose)
		}
		.frame(width: 560)
		.defaultFocus($isFieldFocused, true)
		.onAppear { highlighted = results.first?.id }
		.onChange(of: query) { highlighted = results.first?.id }
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("palette")
	}

	private var field: some View {
		HStack(spacing: Spacing.s2) {
			Image(systemName: "magnifyingglass")
				.foregroundStyle(.textTertiary)
				.accessibilityHidden(true)
			TextField("Go to anything", text: $query)
				.textFieldStyle(.plain)
				.font(.text(.heading))
				.focused($isFieldFocused)
				.onSubmit { results.first { $0.id == highlighted }.map(choose) }
				.onKeyPress(.downArrow) { move(by: 1) }
				.onKeyPress(.upArrow) { move(by: -1) }
				.onKeyPress(.escape) {
					dismiss()
					return .handled
				}
				.accessibilityIdentifier("palette.field")
		}
		.padding(.horizontal, Spacing.s4)
		.frame(height: 48)
	}

	private func move(by step: Int) -> KeyPress.Result {
		let ids = results.map(\.id)
		guard !ids.isEmpty else { return .handled }
		let index = highlighted.flatMap { ids.firstIndex(of: $0) } ?? -step
		highlighted = ids[min(max(index + step, 0), ids.count - 1)]
		return .handled
	}
}

private struct PaletteResults: View {
	let results: [PaletteItem]
	let format: MoneyFormat
	@Binding var highlighted: PaletteItem.ID?
	let activate: (PaletteItem) -> Void

	var body: some View {
		if results.isEmpty {
			Text("No matches")
				.foregroundStyle(.textSecondary)
				.frame(maxWidth: .infinity, minHeight: 44)
		} else {
			ScrollViewReader { proxy in
				ScrollView {
					LazyVStack(spacing: 0) {
						ForEach(results) { item in
							PaletteRow(item: item, isHighlighted: item.id == highlighted)
								.id(item.id)
								.onTapGesture { activate(item) }
								.onHover { if $0 { highlighted = item.id } }
						}
					}
					.padding(Spacing.s2)
				}
				.frame(maxHeight: 340)
				.onChange(of: highlighted) { _, id in
					if let id { proxy.scrollTo(id) }
				}
			}
		}
	}
}

private struct PaletteRow: View {
	let item: PaletteItem
	let isHighlighted: Bool

	var body: some View {
		HStack(spacing: Spacing.s2) {
			if let symbol {
				Image(systemName: symbol)
					.foregroundStyle(.accent)
					.frame(width: 18)
					.accessibilityHidden(true)
			}
			Text(item.title)
				.foregroundStyle(.textPrimary)
			Spacer(minLength: Spacing.s4)
			if let detail = item.detail {
				Text(detail)
					.font(isAccount ? .figures(.body) : .text(.caption))
					.foregroundStyle(isAccount ? .textSecondary : .textTertiary)
			}
		}
		.font(.text(.body))
		.padding(.horizontal, Spacing.s2)
		.frame(height: 32)
		.background(isHighlighted ? AnyShapeStyle(.accentSoft) : AnyShapeStyle(.clear), in: .rect(cornerRadius: Radius.control))
		.contentShape(.rect)
		.accessibilityElement(children: .combine)
		.accessibilityAddTraits(isHighlighted ? [.isButton, .isSelected] : .isButton)
		.accessibilityIdentifier("palette.row.\(item.title)")
	}

	private var isAccount: Bool {
		if case .go(.account) = item.target { return true }
		return false
	}

	private var symbol: String? {
		switch item.target {
		case .go(.screen(let screen)): screen.symbol
		case .go(.account): Screen.accounts.symbol
		case .perform(.addTransaction): "plus"
		case .perform(.moveMoney): "arrow.left.arrow.right"
		case .perform(.newBill): "calendar.badge.plus"
		}
	}
}
