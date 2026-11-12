import AppKit
import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// Backups to a folder you choose (R-REL-4) and local snapshots (R-REL-3).
struct BackupSettingsView: View {
	let settings: BudgetSettingsModel
	@State private var restoring: SnapshotInfo?

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: Spacing.s3) {
				SectionTitle("Backups")
				backups
				HStack {
					Button("Back up now") { Task { await settings.backUpNow() } }
						.disabled(settings.backup.folder == nil || settings.isBackingUp)
						.accessibilityIdentifier("settings.backUpNow")
					if settings.isBackingUp {
						ProgressView().controlSize(.small)
					}
				}
				SectionTitle("Snapshots")
					.padding(.top, Spacing.s4)
				if let retention = settings.retention {
					RetentionRows(retention: retention) { retention in
						Task { await settings.setRetention(retention) }
					}
				}
				SnapshotRows(snapshots: settings.snapshots) { restoring = $0 }
			}
			.padding(Spacing.s5)
			.frame(maxWidth: 640, alignment: .leading)
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.task { await settings.load() }
		.alert(
			restoring.map { "Restore the budget to \(When.text($0.takenAt))?" } ?? "",
			isPresented: Binding(get: { restoring != nil }, set: { if !$0 { restoring = nil } }),
			presenting: restoring
		) { snapshot in
			Button("Restore") { Task { await settings.restoreSnapshot(id: snapshot.id) } }
				.keyboardShortcut(.defaultAction)
			Button("Cancel", role: .cancel) {}
		} message: { _ in
			Text("Veritas saves a snapshot of the budget as it is now first.")
		}
	}

	private var backups: some View {
		GroupedRows {
			LabeledRow("Folder") {
				HStack(spacing: Spacing.s2) {
					Text(settings.backup.folder.map(\.displayPath) ?? "None")
						.foregroundStyle(.textSecondary)
						.lineLimit(1)
						.truncationMode(.middle)
						.accessibilityIdentifier("settings.backupFolder")
					Button("Choose…", action: chooseFolder)
						.accessibilityIdentifier("settings.chooseBackupFolder")
				}
			}
			Divider().padding(.leading, Spacing.s4)
			LabeledRow("Schedule") {
				Picker("Schedule", selection: Binding(get: { settings.schedule }, set: settings.setSchedule)) {
					ForEach(BackupSchedule.allCases, id: \.self) { schedule in
						Text(schedule.title).tag(schedule)
					}
				}
				.labelsHidden()
				.fixedSize()
				.accessibilityIdentifier("settings.backupSchedule")
			}
			Divider().padding(.leading, Spacing.s4)
			LabeledRow("Last backup") {
				Text(settings.backup.lastAt.map(When.text) ?? "Never")
					.font(.figures(.body))
					.foregroundStyle(.textSecondary)
					.accessibilityIdentifier("settings.lastBackup")
			}
			if let problem = settings.backup.lastProblem {
				ProblemNote(text: problem)
					.frame(maxWidth: .infinity, alignment: .leading)
					.padding(.horizontal, Spacing.s4)
					.padding(.bottom, Spacing.s3)
			}
		}
	}

	private func chooseFolder() {
		if let folder = LaunchEnvironment.backupFolderWithoutPanel {
			Task { await settings.chooseBackupFolder(folder) }
			return
		}
		let panel = NSOpenPanel()
		panel.canChooseDirectories = true
		panel.canChooseFiles = false
		panel.canCreateDirectories = true
		panel.prompt = "Choose"
		Task {
			guard await panel.begin() == .OK, let folder = panel.url else { return }
			await settings.chooseBackupFolder(folder)
		}
	}
}

private struct RetentionRows: View {
	let retention: Retention
	let change: (Retention) -> Void

	var body: some View {
		GroupedRows {
			row("Hourly snapshots", \.hourly, range: 0...240)
			Divider().padding(.leading, Spacing.s4)
			row("Daily snapshots", \.daily, range: 0...365)
			Divider().padding(.leading, Spacing.s4)
			row("Monthly snapshots", \.monthly, range: 0...120)
			Divider().padding(.leading, Spacing.s4)
			row("Days to keep snapshots taken before changes", \.operationDays, range: 1...365)
		}
	}

	private func row(_ title: String, _ field: WritableKeyPath<Retention, UInt32>, range: ClosedRange<UInt32>) -> some View {
		LabeledRow(title) {
			Stepper(value: Binding(get: { retention[keyPath: field] }, set: { value in
				var updated = retention
				updated[keyPath: field] = value
				change(updated)
			}), in: range) {
				Text("\(retention[keyPath: field])")
					.font(.figures(.body))
					.frame(minWidth: 32, alignment: .trailing)
			}
		}
	}
}

private struct SnapshotRows: View {
	let snapshots: [SnapshotInfo]
	let restore: (SnapshotInfo) -> Void

	var body: some View {
		GroupedRows {
			if snapshots.isEmpty {
				Text("No snapshots yet")
					.foregroundStyle(.textSecondary)
					.frame(maxWidth: .infinity, minHeight: Size.row)
			}
			ForEach(snapshots, id: \.id) { snapshot in
				HStack(spacing: Spacing.s3) {
					Text(When.text(snapshot.takenAt))
						.font(.figures(.body))
					Text(snapshot.reason.title)
						.foregroundStyle(.textSecondary)
					Spacer()
					Button("Restore…") { restore(snapshot) }
						.controlSize(.small)
						.accessibilityIdentifier("settings.restore.\(snapshot.id)")
				}
				.padding(.horizontal, Spacing.s4)
				.frame(minHeight: Size.row)
				.accessibilityElement(children: .contain)
				if snapshot.id != snapshots.last?.id {
					Divider().padding(.leading, Spacing.s4)
				}
			}
		}
		.containerIdentifier("settings.snapshots")
	}
}

/// Dates and times as the Mac formats them.
enum When {
	static func text(_ date: Date) -> String {
		date.formatted(date: .abbreviated, time: .shortened)
	}

	static func text(_ coreMoment: String) -> String {
		(try? Date(coreMoment, strategy: .iso8601)).map(text) ?? coreMoment
	}
}

private extension URL {
	var displayPath: String {
		let path = path(percentEncoded: false)
		return path.count > 1 && path.hasSuffix("/") ? String(path.dropLast()) : path
	}
}
