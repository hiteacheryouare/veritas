/// The workspace banner for a failed snapshot. An open or hourly snapshot's
/// problem clears with the next good snapshot, so "the last" stays true; the
/// import snapshot's lasts the session, so it names that copy instead.
public enum SnapshotWords {
	public static func banner(problem: String, fromImport: Bool) -> String {
		fromImport
			? "A snapshot failed: the copy taken when you imported this budget wasn't saved. \(problem)"
			: "The last snapshot failed. \(problem)"
	}
}
