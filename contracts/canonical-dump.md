# Canonical dump (contract v0)

The canonical dump is how Veritas proves parity (PLAN §6.4). There are two producers:

- `tools/actual-oracle` `dump`: the reference implementation.
- `veritas-interop::canonical_dump`: the Rust mirror. Its output must be **byte-identical** to the oracle's for the same zip.

The normative format, version 1, is specified in `docs/oracle.md` ("Canonical dump"): top-level keys, metadata normalization, the schema section, table and row ordering, cell encoding by SQLite storage class, string escaping, invalid-UTF-8 replacement, and the whitespace layout. `tools/actual-oracle/test/canonicalJson.test.ts` holds byte-level examples that the Rust tests must reuse.

## Comparisons

| Test family | Compares | Ignores |
|---|---|---|
| Round trip, R-IO-3 | `dump(export(import(zip)))` with `dump(zip)` | `kvcache*` (already excluded), metadata ids (already normalized) |
| Differential | The `computed` sections after the same op list | Nothing |
| Actual acceptance | Actual's import of a Veritas export | `messages_clock`'s node id, which Actual's `resetClock` rewrites |

A change to the format bumps `version`. It needs a `contract` PR that updates `docs/oracle.md`, the oracle, the Rust mirror and every committed golden.
