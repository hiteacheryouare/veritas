import SwiftUI
import VeritasCore
import VeritasDesign
import VeritasKit

/// The invariant audit's findings (R-REL-2). Nothing here is fixed for you.
struct IntegrityReportSheet: View {
	let report: IntegrityReport?
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: Spacing.s4) {
			Text("Integrity report")
				.font(.text(.title, weight: .semibold))
			content
			HStack {
				Spacer()
				Button("Close", role: .cancel) { dismiss() }
					.keyboardShortcut(.cancelAction)
			}
		}
		.padding(Spacing.s5)
		.frame(width: 540)
		.containerIdentifier("sheet.integrityReport")
	}

	@ViewBuilder private var content: some View {
		switch report {
		case .budget(nil), nil:
			HStack(spacing: Spacing.s2) {
				ProgressView().controlSize(.small)
				Text("The check is still running.")
			}
			.foregroundStyle(.textSecondary)
		case .budget(let findings?):
			if findings.isEmpty {
				Text("No problems found.")
					.foregroundStyle(.textSecondary)
			} else {
				FindingRows(findings: findings)
			}
		case .refusedImport:
			Text("The file is damaged, so Veritas didn't import it. Export it from Actual again, then import the new file.")
				.fixedSize(horizontal: false, vertical: true)
		case .export(let findings):
			Text("The export is written. The check found these in the budget.")
				.foregroundStyle(.textSecondary)
			FindingRows(findings: findings)
		}
	}
}

private struct FindingRows: View {
	let findings: [AuditFinding]

	var body: some View {
		ScrollView {
			GroupedRows {
				ForEach(Array(findings.enumerated()), id: \.offset) { index, finding in
					VStack(alignment: .leading, spacing: 2) {
						Text(finding.check.title)
						Text(finding.detail ?? "In \(finding.table)")
							.font(.text(.caption))
							.foregroundStyle(.textSecondary)
					}
					.frame(maxWidth: .infinity, alignment: .leading)
					.padding(.horizontal, Spacing.s4)
					.padding(.vertical, Spacing.s2)
					.accessibilityElement(children: .combine)
					if index < findings.count - 1 {
						Divider().padding(.leading, Spacing.s4)
					}
				}
			}
		}
		.frame(maxHeight: 360)
		.fixedSize(horizontal: false, vertical: true)
	}
}
