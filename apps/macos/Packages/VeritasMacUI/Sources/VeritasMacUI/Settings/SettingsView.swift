import SwiftUI
import VeritasDesign
import VeritasKit

/// Settings, with the mockup's segmented control. AI and Tags stay hidden until they work (D69).
struct SettingsView: View {
	enum Section: String, CaseIterable {
		case backups
		case actual
		case appearance

		var title: String {
			switch self {
			case .backups: "Backups"
			case .actual: "Actual"
			case .appearance: "Appearance"
			}
		}
	}

	@Bindable var model: WorkspaceModel
	@SceneStorage("settingsSection") private var section = Section.backups

	var body: some View {
		Group {
			switch section {
			case .backups:
				if let settings = model.settings {
					BackupSettingsView(settings: settings)
				}
			case .actual:
				ActualSettingsView(model: model)
			case .appearance:
				AppearanceSettingsView()
			}
		}
		.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
		.background(.surfaceWindow)
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Picker("Settings section", selection: $section) {
					ForEach(Section.allCases, id: \.self) { section in
						Text(section.title).tag(section)
					}
				}
				.pickerStyle(.segmented)
				.labelsHidden()
				.accessibilityIdentifier("settings.section")
			}
		}
		.containerIdentifier("screen.settings")
	}
}

private struct ActualSettingsView: View {
	let model: WorkspaceModel

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s3) {
			SectionTitle("Actual")
			HStack(spacing: Spacing.s3) {
				Button("Import from Actual…") { model.isImportPanelShown = true }
				Button("Export to Actual…") {
					Task { await ExportPanel.run(for: model) }
				}
				.disabled(!model.canExport)
			}
			.controlSize(.large)
		}
		.padding(Spacing.s5)
		.frame(maxWidth: 640, alignment: .leading)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

private struct AppearanceSettingsView: View {
	@AppStorage(AppearanceSetting.storageKey) private var appearance = AppearanceSetting.system

	var body: some View {
		GroupedRows {
			LabeledRow("Appearance") {
				Picker("Appearance", selection: $appearance) {
					ForEach(AppearanceSetting.allCases, id: \.self) { setting in
						Text(setting.title).tag(setting)
					}
				}
				.pickerStyle(.segmented)
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("settings.appearance")
			}
		}
		.padding(Spacing.s5)
		.frame(maxWidth: 640, alignment: .leading)
		.frame(maxWidth: .infinity, alignment: .leading)
		.onChange(of: appearance) { _, setting in setting.apply() }
	}
}
