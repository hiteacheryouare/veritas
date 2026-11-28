import Testing
import VeritasKit

struct SnapshotWordsTests {
	@Test func anImportFailureNamesTheCopyThatWasntSaved() {
		#expect(
			SnapshotWords.banner(problem: "The disk is full.", fromImport: true)
				== "A snapshot failed: the copy taken when you imported this budget wasn't saved. The disk is full."
		)
	}

	@Test func anOpenOrHourlyFailureIsTheLastSnapshot() {
		#expect(SnapshotWords.banner(problem: "The disk is full.", fromImport: false) == "The last snapshot failed. The disk is full.")
	}
}
