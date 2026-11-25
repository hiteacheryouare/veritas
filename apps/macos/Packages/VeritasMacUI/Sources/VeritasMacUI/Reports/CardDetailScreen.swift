import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// A dashboard card opened full size, as clicking a card in Actual opens its
/// report page: the card's own figures drawn large, with the card's editor.
/// A custom report opens on the report itself, with the report editor.
struct CardDetail: View {
	let card: DashboardCard
	@Bindable var workspace: WorkspaceModel
	let model: DashboardModel
	let morphSpace: Namespace.ID
	let configure: (DashboardCard) -> Void
	let close: () -> Void

	private var reportID: String? {
		guard case .customReport = card.content, let id = card.config.reportId, !id.isEmpty else { return nil }
		return id
	}

	var body: some View {
		Group {
			if let reportID {
				CustomReportFocus(workspace: workspace, reportID: reportID)
					.id(reportID)
			} else {
				ScrollView {
					VStack(alignment: .leading, spacing: Spacing.s4) {
						WidgetHeader(card: card, format: model.format)
						WidgetBody(card: card, format: model.format, expanded: true)
							.frame(height: 440)
					}
					.padding(Spacing.s5)
					.background(.surfaceGrouped, in: .rect(cornerRadius: Radius.section))
					.overlay { RoundedRectangle(cornerRadius: Radius.section).strokeBorder(Color.edgeGrouped) }
					.morph(card.id, in: morphSpace)
					.padding(.top, Spacing.s2)
					.padding(.horizontal, Spacing.s5)
					.padding(.bottom, Spacing.s6)
				}
			}
		}
		.background(.surfaceWindow)
		.toolbar {
			ToolbarItem(placement: .navigation) {
				Button(action: close) {
					Label("Dashboard", systemImage: "chevron.left")
				}
				.labelStyle(.titleAndIcon)
				.keyboardShortcut("[", modifiers: .command)
				.help("Back to the dashboard")
				.accessibilityIdentifier("cardDetail.back")
			}
			if reportID == nil, model.canEdit, card.kind != nil {
				ToolbarItem(placement: .primaryAction) {
					Button {
						configure(card)
					} label: {
						Label("Edit card", systemImage: "slider.horizontal.3")
					}
					.labelStyle(.titleAndIcon)
					.accessibilityIdentifier("cardDetail.edit")
				}
			}
		}
		.containerIdentifier("screen.cardDetail")
	}
}

/// The custom report behind a card, on the report page's own model, so it
/// edits and previews through `previewCustomReport` and saves through
/// `saveCustomReport` like the Custom reports page.
private struct CustomReportFocus: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: CustomReportsModel
	@State private var drilldown: DrilldownModel?

	init(workspace: WorkspaceModel, reportID: String) {
		self.workspace = workspace
		_model = State(initialValue: CustomReportsModel(workspace: workspace, choice: .custom(id: reportID)))
	}

	var body: some View {
		CustomReportPane(model: model) { drilldown = $0 }
			.toolbar {
				ToolbarItem(placement: .primaryAction) { CustomReportEditButton(model: model) }
			}
			.sheet(item: $drilldown) { DrilldownSheet(model: $0) }
			.task(id: workspace.changeCount) { await model.load() }
	}
}
