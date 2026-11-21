import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// What-ifs (R-FC-2): each what-if with its events, which Forecast can
/// compare against the baseline. An event dated in the past stays, flagged,
/// until it is re-dated or removed (D67).
struct WhatIfsScreen: View {
	@Bindable var workspace: WorkspaceModel
	@State private var model: WhatIfsModel
	@State private var isCreating = false
	@State private var renaming: ScenarioSummary?
	@State private var deleting: ScenarioSummary?
	@State private var editing: EventEditing?
	@FocusState private var hasKeyFocus: Bool

	init(workspace: WorkspaceModel, model: WhatIfsModel? = nil) {
		self.workspace = workspace
		_model = State(initialValue: model ?? WhatIfsModel(workspace: workspace))
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s5) {
				if model.scenarios.isEmpty {
					VStack(spacing: Spacing.s3) {
						Text("No what-ifs yet")
							.font(.text(.heading, weight: .semibold))
							.foregroundStyle(.textSecondary)
						Button("New what-if") { isCreating = true }
							.disabled(model.isReadOnly)
							.accessibilityIdentifier("whatIfs.new.empty")
					}
					.frame(maxWidth: .infinity, minHeight: 240)
					.accessibilityIdentifier("whatIfs.empty")
				}
				ForEach(model.scenarios, id: \.id) { scenario in
					ScenarioSection(
						model: model,
						scenario: scenario,
						rename: { renaming = scenario },
						delete: { deleting = scenario },
						edit: { editing = $0 }
					)
				}
			}
			.padding(.top, Spacing.s2)
			.padding(.horizontal, Spacing.s5)
			.padding(.bottom, Spacing.s6)
			.frame(maxWidth: 820, alignment: .leading)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(.surfaceWindow)
		.focusable()
		.focusEffectDisabled()
		.focused($hasKeyFocus)
		.defaultFocus($hasKeyFocus, true)
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Button { isCreating = true } label: {
					Label("New what-if", systemImage: "plus")
				}
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("whatIfs.new")
			}
		}
		.sheet(isPresented: $isCreating) { NewWhatIfSheet(model: model) }
		.sheet(item: $renaming) { RenameWhatIfSheet(model: model, scenario: $0) }
		.sheet(item: $editing) { EventSheet(model: model, editing: $0) }
		.confirmationDialog(
			deleting.map { "Delete \($0.name)?" } ?? "",
			isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
			presenting: deleting
		) { scenario in
			Button("Delete", role: .destructive) { Task { _ = await model.delete(scenario.id) } }
				.accessibilityIdentifier("whatIfs.delete.confirm")
		}
		.task(id: workspace.changeCount) { await model.load() }
		.containerIdentifier("screen.whatIfs")
	}
}

extension ScenarioSummary: @retroactive Identifiable {}

/// The event the editor sheet adds to or changes in one what-if.
struct EventEditing: Identifiable {
	let scenario: ScenarioSummary
	let event: ScenarioEventItem?

	var id: String { "\(scenario.id).\(event?.id ?? "new")" }
}

private struct ScenarioSection: View {
	let model: WhatIfsModel
	let scenario: ScenarioSummary
	let rename: () -> Void
	let delete: () -> Void
	let edit: (EventEditing) -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s2) {
			HStack(alignment: .firstTextBaseline, spacing: Spacing.s3) {
				SectionTitle(scenario.name)
				Spacer()
				Toggle("Compare", isOn: Binding(get: { model.isCompared(scenario.id) }, set: { model.setCompared(scenario.id, $0) }))
					.toggleStyle(.checkbox)
					.disabled(!model.isCompared(scenario.id) && !model.canCompareMore)
					.accessibilityIdentifier("whatIfs.compare.\(scenario.id)")
				Menu {
					Button("Rename…", action: rename)
					Button("Delete…", role: .destructive, action: delete)
				} label: {
					Label("Actions", systemImage: "ellipsis.circle").labelStyle(.iconOnly)
				}
				.menuStyle(.borderlessButton)
				.fixedSize()
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("whatIfs.actions.\(scenario.id)")
			}
			GroupedRows {
				if scenario.events.isEmpty {
					Text("No events")
						.foregroundStyle(.textSecondary)
						.frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
						.padding(.horizontal, Spacing.s4)
				}
				ForEach(Array(scenario.events.enumerated()), id: \.element.id) { index, event in
					if index > 0 { Divider().overlay(Color.separator).padding(.leading, Spacing.s4) }
					EventRow(model: model, scenario: scenario, event: event) {
						edit(EventEditing(scenario: scenario, event: event))
					}
				}
			}
			.containerIdentifier("whatIfs.events.\(scenario.id)")
			HStack {
				Spacer()
				Button {
					edit(EventEditing(scenario: scenario, event: nil))
				} label: {
					Label("Add event", systemImage: "plus")
				}
				.controlSize(.small)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("whatIfs.addEvent.\(scenario.id)")
			}
		}
	}
}

private struct EventRow: View {
	let model: WhatIfsModel
	let scenario: ScenarioSummary
	let event: ScenarioEventItem
	let edit: () -> Void

	var body: some View {
		HStack(spacing: Spacing.s3) {
			VStack(alignment: .leading, spacing: 2) {
				Text(model.words.sentence(event.change))
					.foregroundStyle(event.inThePast ? Color.textSecondary : Color.textPrimary)
				if event.inThePast {
					Label("In the past", systemImage: "clock.arrow.circlepath")
						.font(.text(.caption))
						.foregroundStyle(.moneyLow)
				}
			}
			Spacer(minLength: Spacing.s4)
			Button("Edit", action: edit)
				.controlSize(.small)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("whatIfs.event.edit.\(event.id)")
			Button("Remove") { Task { _ = await model.remove(event.id, from: scenario.id) } }
				.controlSize(.small)
				.disabled(model.isReadOnly)
				.accessibilityIdentifier("whatIfs.event.remove.\(event.id)")
		}
		.padding(.horizontal, Spacing.s4)
		.padding(.vertical, Spacing.s2)
		.frame(minHeight: 44)
		.accessibilityElement(children: .contain)
		.accessibilityIdentifier("whatIfs.event.\(event.id)")
	}
}
