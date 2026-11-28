import SwiftUI
import VeritasDesign
import VeritasKit

struct WorkspaceView: View {
	@Bindable var model: WorkspaceModel
	let tour: GuidedTour
	@State private var paletteChoice: PaletteItem?
	@Environment(AssistantSettings.self) private var assistant: AssistantSettings?

	private var offersAssistant: Bool { assistant?.offersPanel ?? false }

	var body: some View {
		NavigationSplitView {
			SidebarView(model: model)
				.navigationSplitViewColumnWidth(min: 200, ideal: Size.sidebar, max: 320)
		} detail: {
			ZStack {
				page
					.id(PageID(directory: model.overview?.directory, destination: model.destination))
					.motionTransition(model.arrival == .drill ? .push : .page)
			}
				.tourOverlay(tour, isOnStepScreen: model.destination == tour.destination)
				.onChange(of: tour.current) {
					if let destination = tour.destination {
						model.go(to: destination)
					}
				}
				.onChange(of: model.destination) { _, destination in
					tour.follow(destination)
				}
				.safeAreaInset(edge: .top, spacing: 0) {
					VStack(spacing: 0) {
						if let readOnly = model.readOnly {
							ReadOnlyBanner(notice: readOnly.notice) { action in
								switch action {
								case .showIntegrityReport: model.showIntegrityReport()
								}
							}
							.motionTransition(.reveal)
						}
						if let problem = model.settings?.backup.lastProblem {
							NoticeBanner(symbol: "exclamationmark.triangle", message: "The last backup failed. \(problem)", actionTitle: "Show settings") {
								model.go(to: .screen(.settings))
							}
							.accessibilityIdentifier("backupBanner")
							.motionTransition(.reveal)
						}
						if let problem = model.overview?.scheduleProblem {
							NoticeBanner(symbol: "calendar.badge.exclamationmark", message: "Bills didn't post today. \(problem)", actionTitle: "Show bills") {
								model.go(to: .screen(.bills))
							}
							.accessibilityIdentifier("scheduleBanner")
							.motionTransition(.reveal)
						}
						if let overview = model.overview, let problem = overview.snapshotProblem {
							NoticeBanner(symbol: "clock.badge.exclamationmark", message: SnapshotWords.banner(problem: problem, fromImport: overview.snapshotProblemIsImport), actionTitle: "Show settings") {
								model.go(to: .screen(.settings))
							}
							.accessibilityIdentifier("snapshotBanner")
							.motionTransition(.reveal)
						}
						FindBar(isShown: $model.isFindShown)
					}
					.motion(.settle, value: banners)
				}
				.navigationTitle(model.title(of: model.destination))
				.toolbar { WorkspaceToolbar(model: model, offersAssistant: offersAssistant) }
				.inspector(isPresented: Binding(get: { offersAssistant && model.isAssistantShown }, set: { model.isAssistantShown = $0 })) {
					if let assistant {
						AssistantInspector(workspace: model, settings: assistant)
							.id(model.overview?.directory)
							.inspectorColumnWidth(min: 260, ideal: 320, max: 480)
					}
				}
				.onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
					assistant?.refresh()
				}
		}
		.sheet(isPresented: $model.isPaletteShown, onDismiss: activatePaletteChoice) {
			CommandPaletteView(index: model.paletteIndex, format: model.format) { item in
				paletteChoice = item
				model.isPaletteShown = false
			}
		}
	}

	/// Which banners show, so the page below settles when one comes or goes.
	private var banners: [Bool] {
		[model.readOnly != nil, model.settings?.backup.lastProblem != nil, model.overview?.scheduleProblem != nil, model.overview?.snapshotProblem != nil]
	}

	/// The screen for the current destination.
	@ViewBuilder private var page: some View {
		if model.destination == .screen(.settings) {
			SettingsView(model: model)
		} else if model.destination == .screen(.budget) {
			BudgetScreen(workspace: model)
		} else if model.destination == .screen(.bills) {
			BillsScreen(workspace: model)
		} else if model.destination == .screen(.rules) {
			RulesScreen(workspace: model)
		} else if model.destination == .screen(.statements) {
			StatementsScreen(workspace: model)
		} else if model.destination == .screen(.forecast) {
			ForecastScreen(workspace: model)
		} else if model.destination == .screen(.whatIfs) {
			WhatIfsScreen(workspace: model)
		} else if model.destination == .screen(.tax) {
			TaxScreen(workspace: model)
		} else if model.destination == .screen(.wrapped) {
			WrappedScreen(workspace: model)
		} else if model.destination == .screen(.goals) {
			GoalsScreen(workspace: model)
		} else if model.destination == .screen(.accounts) {
			AccountsScreen(workspace: model)
		} else if case .account(let id) = model.destination {
			AccountRegisterScreen(workspace: model, accountID: id)
				.id(id)
		} else if model.destination == .screen(.reports) || model.destination == .screen(.dashboard) {
			DashboardScreen(workspace: model)
		} else if model.destination == .screen(.netProfit) {
			NetProfitScreen(workspace: model)
		} else if model.destination == .screen(.sankey) {
			SankeyScreen(workspace: model)
		} else if model.destination == .screen(.customReports) {
			CustomReportsScreen(workspace: model)
		} else {
			ScreenView(destination: model.destination, title: model.title(of: model.destination))
		}
	}

	/// Runs after the palette has gone, so a command that opens a sheet
	/// doesn't race the palette's own sheet off the window.
	private func activatePaletteChoice() {
		guard let item = paletteChoice else { return }
		paletteChoice = nil
		model.activate(item)
	}
}

/// A page's identity: a new budget or a new destination is a new page, which
/// fades through from the last one, or pushes in when opened from a row.
private struct PageID: Hashable {
	let directory: URL?
	let destination: Destination
}
