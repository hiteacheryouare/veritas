import SwiftUI
import VeritasDesign
import VeritasKit

/// Wrapped (R-WRAP-1, R-WRAP-2, design/wrapped-editorial.md): the year as an
/// issue of facing pages, turned with the arrows, Space or the contents.
/// Hide amounts shows the core's shares in place of every amount. Export
/// saves this spread, every spread, or the issue as a PDF.
struct WrappedScreen: View {
	let workspace: WorkspaceModel
	@State private var model: WrappedModel
	@FocusState private var hasKeyFocus: Bool
	/// Which way the last turn went, so the next spread comes in from that side.
	@State private var isGoingBack = false
	/// Spreads already seen this session show complete; a first view arrives in order.
	@State private var seen: Set<String> = []
	@Environment(\.colorScheme) private var colorScheme

	init(workspace: WorkspaceModel, model: WrappedModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? WrappedModel(workspace: workspace))
	}

	private var palette: WrappedPalette { .of(colorScheme) }

	var body: some View {
		GeometryReader { proxy in
			Group {
				if let spread = model.spread {
					if proxy.size.width < WrappedCanvas.narrow {
						stacked(spread)
					} else {
						canvas(spread, in: proxy.size)
					}
				} else {
					ScreenState(kind: .loading)
				}
			}
			.frame(width: proxy.size.width, height: proxy.size.height)
		}
		.background(palette.window)
		.background {
			Button("Contents") { turn(to: model.spreads.firstIndex { $0.pages.contains { $0.kind == .contents } } ?? 0) }
				.keyboardShortcut("[", modifiers: .command)
				.opacity(0)
				.accessibilityHidden(true)
		}
		.environment(\.wrappedOpen) { index in turn(to: index) }
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.onKeyPress(keys: [.leftArrow, .rightArrow, .space, .home, .end]) { press in
			switch press.key {
			case .leftArrow: turn(to: model.index - 1)
			case .rightArrow: turn(to: model.index + 1)
			case .space: turn(to: model.index + (press.modifiers.contains(.shift) ? -1 : 1))
			case .home: turn(to: 0)
			default: turn(to: model.spreads.count - 1)
			}
			return .handled
		}
		.toolbar { toolbar }
		.navigationTitle(model.title)
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.wrapped")
	}

	/// The spread at canvas size, scaled to fit and never above full size.
	private func canvas(_ spread: WrappedSpread, in size: CGSize) -> some View {
		let inset: CGFloat = 72
		let scale = min(1, (size.width - inset * 2) / WrappedCanvas.spread.width, (size.height - inset) / WrappedCanvas.spread.height)
		return ZStack {
			WrappedSpreadView(spread: spread, palette: palette)
				.environment(\.wrappedEntrance, !seen.contains(spread.id))
				.onAppear { seen.insert(spread.id) }
				.scaleEffect(scale)
				.frame(width: WrappedCanvas.spread.width * scale, height: WrappedCanvas.spread.height * scale)
				.shadow(color: palette.shadow, radius: 24, y: 10)
				.motion(.fast, value: model.hidesAmounts)
				.id(spread.index)
				.motionTransition(isGoingBack ? .turnBack : .turnForward)
		}
		.frame(maxWidth: .infinity, maxHeight: .infinity)
		.overlay(alignment: .leading) {
			arrow("chevron.left", label: "Previous spread", id: "wrapped.previous", enabled: model.canGoBack) { turn(to: model.index - 1) }
				.padding(.leading, 16)
		}
		.overlay(alignment: .trailing) {
			arrow("chevron.right", label: "Next spread", id: "wrapped.next", enabled: model.canGoForward) { turn(to: model.index + 1) }
				.padding(.trailing, 16)
		}
	}

	/// A narrow window: the spread's pages one under the other, each a page wide.
	private func stacked(_ spread: WrappedSpread) -> some View {
		ScrollView {
			VStack(spacing: 24) {
				ForEach(spread.pages) { page in
					WrappedPageView(page: page, side: .single, palette: palette)
						.frame(width: WrappedCanvas.page.width, height: WrappedCanvas.page.height)
						.background(palette.paper)
						.shadow(color: palette.shadow, radius: 12, y: 6)
				}
				HStack {
					Button("Previous") { turn(to: model.index - 1) }.disabled(!model.canGoBack)
					Spacer()
					Button("Next") { turn(to: model.index + 1) }.disabled(!model.canGoForward)
				}
				.frame(width: WrappedCanvas.page.width)
			}
			.padding(24)
			.frame(maxWidth: .infinity)
		}
		.id(spread.index)
		.motionTransition(isGoingBack ? .turnBack : .turnForward)
	}

	private func turn(to index: Int) {
		guard index != model.index, model.spreads.indices.contains(index) else { return }
		isGoingBack = index < model.index
		model.show(index)
	}

	@ToolbarContentBuilder private var toolbar: some ToolbarContent {
		ToolbarItem(placement: .navigation) {
			Picker("Year", selection: Binding(get: { model.year }, set: { year in Task { await model.choose(year) } })) {
				ForEach(model.years, id: \.self) { Text(model.yearTitle($0)).tag($0) }
			}
			.pickerStyle(.menu)
			.labelsHidden()
			.fixedSize()
			.accessibilityIdentifier("wrapped.yearPicker")
		}
		ToolbarItem(placement: .navigation) {
			Menu {
				ForEach(model.spreads) { spread in
					Button(spread.title) { turn(to: spread.index) }
				}
			} label: {
				Label("Contents", systemImage: "list.bullet")
					.labelStyle(.titleAndIcon)
			}
			.disabled(model.spreads.count < 2)
			.accessibilityIdentifier("wrapped.contents")
		}
		ToolbarItem(placement: .primaryAction) {
			Toggle(isOn: $model.hidesAmounts) {
				Label("Hide amounts", systemImage: "eye.slash")
					.labelStyle(.titleAndIcon)
			}
			.toggleStyle(.button)
			.accessibilityIdentifier("wrapped.hideAmounts")
		}
		ToolbarItem(placement: .primaryAction) {
			Menu {
				Section {
					exportButton("This Spread as Image…", .spread, redacted: false, id: "wrapped.export.spread")
					exportButton("All Spreads as Images…", .allSpreads, redacted: false, id: "wrapped.export.all")
					exportButton("Issue as PDF…", .pdf, redacted: false, id: "wrapped.export.pdf")
				}
				Section("Without amounts") {
					exportButton("This Spread as Image…", .spread, redacted: true, id: "wrapped.export.spread.redacted")
					exportButton("All Spreads as Images…", .allSpreads, redacted: true, id: "wrapped.export.all.redacted")
					exportButton("Issue as PDF…", .pdf, redacted: true, id: "wrapped.export.pdf.redacted")
				}
			} label: {
				Label("Export", systemImage: "square.and.arrow.up")
					.labelStyle(.titleAndIcon)
			}
			.disabled(model.report == nil)
			.accessibilityIdentifier("wrapped.export")
		}
	}

	private func exportButton(_ title: String, _ kind: WrappedExport.Kind, redacted: Bool, id: String) -> some View {
		Button(title) {
			Task { await WrappedExport.run(kind, model: model, redacted: redacted, paper: colorScheme, workspace: workspace) }
		}
		.accessibilityIdentifier(id)
	}

	private func arrow(_ symbol: String, label: String, id: String, enabled: Bool, action: @escaping () -> Void) -> some View {
		Button(action: action) {
			Image(systemName: symbol)
				.font(.system(size: 15, weight: .semibold))
				.frame(width: 40, height: 40)
				.background(.surfaceControl, in: .circle)
				.overlay { Circle().strokeBorder(Color.edgeControl) }
				.contentShape(.circle)
		}
		.buttonStyle(.plain)
		.disabled(!enabled)
		.opacity(enabled ? 1 : 0.4)
		.help(label)
		.accessibilityLabel(label)
		.accessibilityIdentifier(id)
	}
}
