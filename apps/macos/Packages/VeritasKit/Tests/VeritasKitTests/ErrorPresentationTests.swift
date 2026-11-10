import Testing
import VeritasCore
import VeritasKit

/// One case per row of contracts/errors.md (R-REL-7).
struct ErrorPresentationTests {
	@Test func aMissingRowRefreshesTheScreen() {
		#expect(ErrorPresentation(FfiError.NotFound(entity: "category", id: "c1")) == .refresh)
	}

	@Test func invalidInputShowsTheCoreMessageInline() {
		#expect(ErrorPresentation(FfiError.Invalid(message: "Pick a date in 2027.")) == .inline("Pick a date in 2027."))
	}

	@Test func aReadOnlyWriteShowsTheBannerForItsReason() {
		#expect(ErrorPresentation(FfiError.ReadOnly(reason: .openElsewhere)) == .readOnly(.openElsewhere))
	}

	@Test func anOlderActualFileSaysWhichVersionToOpenItIn() throws {
		guard case .alert(let alert) = ErrorPresentation(FfiError.UpgradeInActualFirst(required: "26.11.0")) else {
			Issue.record("expected an alert")
			return
		}
		#expect(alert.message == "Open this budget in Actual 26.11.0 or newer, then export it again.")
		#expect(!alert.offersRetry)
	}

	@Test func aFailedAuditOpensTheReport() {
		#expect(ErrorPresentation(FfiError.AuditFailed(violations: 3)) == .integrityReport(violations: 3))
	}

	@Test func aZipThatIsntABudgetExplainsWhatWasFound() {
		let presentation = ErrorPresentation(FfiError.NotAnActualBudget(message: "The zip has no db.sqlite."))
		#expect(presentation == .alert(ErrorAlert(title: "That file isn't an Actual budget", message: "The zip has no db.sqlite.")))
	}

	@Test func aStorageFailureSaysNothingWasLostAndOffersRetry() {
		let presentation = ErrorPresentation(FfiError.Storage(message: "disk I/O error"))
		#expect(presentation == .alert(ErrorAlert(title: "Couldn't save that change. Nothing was lost.", offersRetry: true)))
	}

	@Test func aFileProblemNamesThePath() {
		let message = "Can't write to /Volumes/OneDrive/Backups. Check that the folder exists."
		guard case .alert(let alert) = ErrorPresentation(FfiError.Io(message: message)) else {
			Issue.record("expected an alert")
			return
		}
		#expect(alert.message == message)
	}

	@Test func anOverflowSaysTheAmountIsTooLarge() {
		#expect(ErrorPresentation(FfiError.Overflow) == .inline("That amount is too large."))
	}

	@Test func aCancelledReportShowsNothing() {
		#expect(ErrorPresentation(FfiError.Cancelled) == .nothing)
	}

	@Test func aBugOffersItsDetailsToCopy() {
		let presentation = ErrorPresentation(FfiError.Internal(message: "the core hit a bug: index out of bounds"))
		#expect(presentation == .alert(ErrorAlert(
			title: "Veritas hit a bug.",
			message: "Your budget wasn't changed.",
			details: "the core hit a bug: index out of bounds"
		)))
	}

	@Test func anInlineMessageBecomesAnAlertAwayFromAField() {
		#expect(ErrorPresentation(FfiError.Overflow).withoutField == .alert(ErrorAlert(title: "That amount is too large.")))
		#expect(ErrorPresentation(FfiError.Cancelled).withoutField == .nothing)
	}

	@Test func everyReadOnlyReasonExplainsItselfInPlainWords() {
		let kinds: [ReadOnlyKind] = [.newerActualVersion, .trackingBudget, .openElsewhere, .auditFailed, .newerVeritasSchema]
		for kind in kinds {
			let message = kind.notice.message
			#expect(message.contains("read-only"), "\(kind)")
			#expect(!message.contains("schema") && !message.contains("migration"), "\(kind)")
		}
		#expect(ReadOnlyKind.auditFailed.notice.action == .showIntegrityReport)
		#expect(Set(kinds.map(\.notice.message)).count == kinds.count)
	}
}
