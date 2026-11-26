import SwiftUI
import VeritasDesign
import VeritasKit

/// One spread at canvas size: two facing pages, or one page across both
/// (design/wrapped-editorial.md, The format). It draws only the issue's own
/// text and figures, so the screen and every export show the same words.
struct WrappedSpreadView: View {
	let spread: WrappedSpread
	let palette: WrappedPalette

	var body: some View {
		HStack(spacing: 0) {
			if spread.isFull, let page = spread.pages.first {
				WrappedPageView(page: page, side: .full, palette: palette)
			} else {
				ForEach(Array(spread.pages.enumerated()), id: \.element.id) { index, page in
					WrappedPageView(page: page, side: index == 0 ? .left : .right, palette: palette)
						.frame(width: WrappedCanvas.page.width)
				}
			}
		}
		.frame(width: WrappedCanvas.spread.width, height: WrappedCanvas.spread.height)
		.background(palette.paper)
		.overlay {
			if !spread.isFull {
				Rectangle().fill(palette.rule).frame(width: 1)
			}
		}
		.overlay {
			if let edge = palette.edge {
				Rectangle().strokeBorder(edge, lineWidth: 1)
			}
		}
		.environment(\.colorScheme, palette.isDark ? .dark : .light)
		.accessibilityElement(children: .contain)
		.accessibilityLabel(spread.title)
		.accessibilityIdentifier("wrapped.spread.\(spread.id)")
	}
}

/// Where a page sits, which sets its margins, its folio and how its figure
/// is laid out beside or below its words.
enum WrappedSide {
	case left, right, full, single

	var leading: CGFloat {
		self == .right ? WrappedCanvas.spine : WrappedCanvas.outer
	}

	var trailing: CGFloat {
		self == .left ? WrappedCanvas.spine : WrappedCanvas.outer
	}
}

struct WrappedPageView: View {
	let page: WrappedPage
	let side: WrappedSide
	let palette: WrappedPalette

	var body: some View {
		content
			.padding(.top, WrappedCanvas.top)
			.padding(.bottom, WrappedCanvas.bottom)
			.padding(.leading, side.leading)
			.padding(.trailing, side.trailing)
			.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
			.overlay(alignment: .bottom) { folios }
			.foregroundStyle(palette.ink)
			.accessibilityElement(children: .contain)
			.accessibilityIdentifier("wrapped.page.\(page.kind.rawValue)")
	}

	@ViewBuilder private var content: some View {
		switch page.kind {
		case .cover: WrappedCover(page: page, palette: palette)
		case .backCover: WrappedBackCover(page: page, palette: palette)
		case .contents: WrappedContents(page: page, palette: palette)
		case .shape:
			VStack(alignment: .leading, spacing: 32) {
				WrappedPageWords(page: page, palette: palette)
				WrappedColumnChart(page: page, palette: palette)
			}
		default:
			if side == .full {
				HStack(alignment: .top, spacing: 80) {
					WrappedPageWords(page: page, palette: palette)
						.frame(width: 460, alignment: .leading)
					figure
				}
			} else {
				VStack(alignment: .leading, spacing: 32) {
					WrappedPageWords(page: page, palette: palette)
						.frame(maxWidth: 460, alignment: .leading)
					figure
				}
			}
		}
	}

	@ViewBuilder private var figure: some View {
		VStack(alignment: .leading, spacing: 24) {
			switch page.kind {
			case .ledger:
				WrappedTable(rows: page.rows, palette: palette, drawsShares: false)
				if !page.segments.isEmpty {
					WrappedProportionBar(segments: page.segments, palette: palette)
				}
			case .categories, .groups, .payees:
				WrappedTable(rows: page.rows, palette: palette, drawsShares: true)
			case .paidDown, .goals:
				WrappedTable(rows: page.rows, palette: palette, drawsShares: false)
			case .quietDays:
				if let calendar = page.calendar {
					WrappedCalendar(calendar: calendar, spoken: page.spoken ?? "", palette: palette)
				}
			case .netWorth:
				if !page.rows.isEmpty {
					WrappedTable(rows: page.rows, palette: palette, drawsShares: false)
				}
				if !page.points.isEmpty {
					WrappedLineChart(page: page, palette: palette)
				}
			case .notes:
				VStack(alignment: .leading, spacing: 12) {
					ForEach(page.notes, id: \.self) { note in
						Text(note)
							.font(WrappedType.notes)
							.lineSpacing(5)
							.fixedSize(horizontal: false, vertical: true)
					}
				}
				.frame(maxWidth: 460, alignment: .leading)
			default:
				EmptyView()
			}
			if let footnote = page.footnote {
				Text(footnote)
					.font(WrappedType.notes)
					.foregroundStyle(palette.inkSecondary)
					.fixedSize(horizontal: false, vertical: true)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.staged(4)
	}

	/// The page number in the outer corner, or both numbers across a full spread.
	private var folios: some View {
		HStack {
			if side != .right && side != .single {
				Text(String(page.number))
			}
			Spacer()
			if side == .right || side == .single {
				Text(String(page.number))
			} else if side == .full {
				Text(String(page.number + 1))
			}
		}
		.font(WrappedType.folio)
		.foregroundStyle(palette.inkSecondary)
		.padding(.leading, side.leading)
		.padding(.trailing, side.trailing)
		.padding(.bottom, 24)
		.opacity(page.number > 0 && page.kind != .cover && page.kind != .backCover ? 1 : 0)
		.accessibilityHidden(true)
	}
}

/// A page's words: its heading, headline, dek and hero figure.
private struct WrappedPageWords: View {
	let page: WrappedPage
	let palette: WrappedPalette

	var body: some View {
		VStack(alignment: .leading, spacing: 16) {
			if let heading = page.heading {
				Text(heading)
					.font(WrappedType.heading)
					.accessibilityAddTraits(.isHeader)
			}
			if let headline = page.headline {
				Text(headline)
					.font(WrappedType.headline)
					.lineSpacing(8)
					.fixedSize(horizontal: false, vertical: true)
					.valueChange(headline)
					.accessibilityAddTraits(.isHeader)
					.accessibilityIdentifier("wrapped.headline")
			}
			if let dek = page.dek {
				Text(dek)
					.font(WrappedType.dek)
					.lineSpacing(8)
					.foregroundStyle(palette.inkSecondary)
					.fixedSize(horizontal: false, vertical: true)
					.valueChange(dek)
					.accessibilityIdentifier("wrapped.dek")
			}
			if let hero = page.hero {
				Text(hero)
					.font(WrappedType.hero)
					.lineLimit(1)
					.minimumScaleFactor(0.5)
					.valueChange(hero)
					.staged(3)
					.accessibilityIdentifier("wrapped.hero")
			}
		}
		.staged(0)
	}
}

/// Ranked or summary rows with hairlines between them, each ranked row
/// carrying a bar as long as the core's share.
private struct WrappedTable: View {
	let rows: [WrappedPage.Row]
	let palette: WrappedPalette
	let drawsShares: Bool

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
				VStack(alignment: .leading, spacing: 6) {
					HStack(alignment: .firstTextBaseline, spacing: 16) {
						Text(row.label)
							.font(WrappedType.label)
							.lineLimit(1)
						Spacer(minLength: 16)
						Text(row.value)
							.font(WrappedType.figure)
							.lineLimit(1)
							.layoutPriority(1)
							.valueChange(row.value)
					}
					if let note = row.note {
						Text(note)
							.font(WrappedType.annotation)
							.foregroundStyle(palette.inkSecondary)
					}
					if drawsShares, let share = row.share {
						GeometryReader { proxy in
							Capsule()
								.fill(index == 0 ? palette.accent : palette.bar)
								.frame(width: max(2, proxy.size.width * CGFloat(max(share, 0)) / 1000))
						}
						.frame(height: 4)
						.accessibilityHidden(true)
					}
				}
				.padding(.vertical, 10)
				.overlay(alignment: .bottom) {
					if index < rows.count - 1 {
						Rectangle().fill(palette.rule).frame(height: 0.5)
					}
				}
				.accessibilityElement(children: .combine)
				.accessibilityIdentifier("wrapped.row.\(row.id)")
			}
		}
	}
}

/// One bar split by the core's shares: what went out, and what was left over.
private struct WrappedProportionBar: View {
	let segments: [WrappedPage.Segment]
	let palette: WrappedPalette

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			GeometryReader { proxy in
				HStack(spacing: 2) {
					ForEach(Array(segments.enumerated()), id: \.offset) { index, segment in
						Rectangle()
							.fill(index == 0 ? palette.bar : palette.barSoft)
							.frame(width: max(0, proxy.size.width * CGFloat(segment.share) / 1000 - 1))
					}
				}
			}
			.frame(height: 14)
			HStack {
				ForEach(Array(segments.enumerated()), id: \.offset) { index, segment in
					if index > 0 { Spacer() }
					Text(segment.label)
						.font(WrappedType.annotation)
						.foregroundStyle(palette.inkSecondary)
				}
			}
		}
		.accessibilityElement(children: .combine)
		.accessibilityIdentifier("wrapped.proportion")
	}
}

private struct WrappedCover: View {
	let page: WrappedPage
	let palette: WrappedPalette

	var body: some View {
		VStack(alignment: .leading, spacing: 24) {
			WrappedMasthead(palette: palette)
			Spacer()
			Text(page.headline ?? "")
				.font(WrappedType.coverYear)
				.tracking(-3)
				.accessibilityAddTraits(.isHeader)
				.accessibilityIdentifier("wrapped.headline")
			Rectangle()
				.fill(palette.accent)
				.frame(width: 48, height: 3)
				.accessibilityHidden(true)
			if let dek = page.dek {
				Text(dek)
					.font(WrappedType.dek)
					.lineSpacing(8)
					.foregroundStyle(palette.inkSecondary)
					.accessibilityIdentifier("wrapped.dek")
			}
		}
		.frame(maxHeight: .infinity, alignment: .bottomLeading)
		.staged(0)
	}
}

private struct WrappedBackCover: View {
	let page: WrappedPage
	let palette: WrappedPalette

	var body: some View {
		VStack(alignment: .trailing, spacing: 12) {
			Spacer()
			Text(page.headline ?? "")
				.font(WrappedType.heading)
			if let dek = page.dek {
				Text(dek)
					.font(WrappedType.label)
					.foregroundStyle(palette.inkSecondary)
			}
			WrappedMasthead(palette: palette)
				.padding(.top, 24)
		}
		.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
	}
}

/// The issue's contents: each entry turns straight to its spread.
private struct WrappedContents: View {
	let page: WrappedPage
	let palette: WrappedPalette
	@Environment(\.wrappedOpen) private var open

	var body: some View {
		VStack(alignment: .leading, spacing: 24) {
			Text("Contents")
				.font(WrappedType.heading)
				.accessibilityAddTraits(.isHeader)
			VStack(alignment: .leading, spacing: 0) {
				ForEach(page.contents) { entry in
					Button {
						open?(entry.spread)
					} label: {
						HStack(alignment: .firstTextBaseline) {
							Text(entry.title).font(WrappedType.contents)
							Spacer()
							Text(String(entry.page))
								.font(WrappedType.folio)
								.foregroundStyle(palette.inkSecondary)
						}
						.frame(height: 32)
						.contentShape(.rect)
					}
					.buttonStyle(.plain)
					.accessibilityLabel("\(entry.title), page \(entry.page)")
					.accessibilityIdentifier("wrapped.contents.\(entry.spread)")
				}
			}
			.frame(maxWidth: 380)
		}
		.staged(0)
	}
}

/// The Veritas wordmark on the cover and the back cover. VeritasDesign's
/// `Wordmark` takes its place when the brand (#115) lands; nothing else on
/// the page depends on its size.
struct WrappedMasthead: View {
	let palette: WrappedPalette

	var body: some View {
		Text("Veritas")
			.font(WrappedType.serif(15, .semibold))
			.tracking(1)
			.foregroundStyle(palette.ink)
			.accessibilityLabel("Veritas")
	}
}

extension EnvironmentValues {
	/// Turns to a spread by its index, from the contents page.
	@Entry var wrappedOpen: ((Int) -> Void)? = nil
	/// The spread is being seen for the first time this session, so its parts
	/// arrive in order (design/wrapped-editorial.md, first-view entrance).
	@Entry var wrappedEntrance = false
}

extension View {
	/// One part of a spread's first-view entrance, `step` staggers in.
	func staged(_ step: Int) -> some View {
		modifier(Staged(step: step))
	}
}

private struct Staged: ViewModifier {
	let step: Int
	@Environment(\.wrappedEntrance) private var entrance
	@State private var isIn: Bool?

	func body(content: Content) -> some View {
		content
			.shown(isIn ?? !entrance, .staged(step))
			.onAppear { isIn = true }
	}
}
