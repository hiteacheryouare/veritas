# Moving Veritas to a new Actual version

Veritas reads and writes one exact Actual schema (PLAN §6.3). When Ryan upgrades Actual, Veritas follows with this playbook. Any agent can run it. It lands as a single PR labeled `file-format` and `contract`.

Below, `OLD` and `NEW` are release tags such as `v26.11.0` and `v26.12.0`. `NEWVER` is the same version without the `v`.

## 1. Read what changed

```sh
git -C reference/actual fetch --depth 1 origin tag NEW
git -C reference/actual diff OLD NEW --stat -- packages/loot-core/migrations packages/loot-core/src/server/sql packages/loot-core/src/server/aql/schema
git -C reference/actual diff OLD NEW -- packages/loot-core/migrations
```

Sort each new migration into one of three kinds:

| Kind | Examples | What Veritas must do |
|---|---|---|
| Additive | New table, new nullable column | Usually nothing. Passthrough keeps it (R-IO-5). Add a mapping only if Veritas should model it. |
| Default or semantic | New column Actual reads with a non-null default, changed budget math | Inserts must write the new default. Port any math change, citing file:line. |
| Destructive | Dropped, renamed or retyped column, rewritten rows | Update the `veritas-store` mappings and the canonical dump. Stop and ask Ryan if user data is affected. |

Also diff `server/budget/`, `server/transactions/`, `server/rules/`, `server/schedules/` and `shared/arithmetic.ts`. A behavior change there is a parity change, even with no migration.

## 2. Move the pins

1. **Manifest.** Rename `crates/veritas-interop/manifest/actual-OLDVER.json` to `actual-NEWVER.json`. Update `actualTag`, `actualCommit` (`git -C reference/actual rev-parse NEW^{commit}`) and `apiVersion`, then regenerate `migrations` from the numeric prefixes of `packages/loot-core/migrations/*` (skip `README.md`). Check `additiveOnlyCutoff` against `ADDITIVE_ONLY_CUTOFF` in `packages/loot-core/src/server/migrate/migrations.ts`.
2. **Reference.** Delete the checkout, then recreate it: `rm -rf reference/actual && cargo xtask reference`.
3. **Oracle.** In `tools/actual-oracle/`, run `npm install --save-exact @actual-app/api@NEWVER`. Commit the lockfile. If Actual raised its Node engine floor, update `.nvmrc`.
4. **Notices.** Bump the pinned version in THIRD_PARTY_NOTICES.md.

## 3. Update Veritas

1. `veritas-store` mappings and insert defaults, for every default, semantic or destructive change from step 1.
2. `docs/actual-format.md`: re-verify every claim whose cited file changed, and update its citations.
3. Ported logic (engine, rules, arithmetic): port the change, and add the new upstream test vectors.
4. Regenerate the fixtures with `cargo xtask fixtures`, then commit them.

## 4. Prove it

- Run `cargo xtask ci` until it's green. That covers the round trip on every fixture, the differential behavior suite and Actual's acceptance of every export.
- Open an older fixture and confirm Veritas asks you to upgrade it in Actual first. Open one with an extra unknown migration id and confirm it opens read-only (R-IO-7).
- Run the private-fixture shadow comparison locally (PLAN §8, Phase 3). Report only pass or fail.

## 5. Ship

Open the PR with the migration classification table from step 1 in the description. It needs the standard merge bar plus the Architect (`contract`). Until that PR merges, a budget exported from the new Actual opens read-only in Veritas, by design.
