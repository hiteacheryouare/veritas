# Third-party notices

Veritas ports algorithms from Actual Budget. Each ported module is listed below
with the Actual source it derives from. Actual is distributed under the following
license.

## Actual Budget

Source: https://github.com/actualbudget/actual (pinned to v26.11.0)

```
Copyright James Long

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Ported modules

| Veritas | Actual source |
|---|---|
| `crates/veritas-engine/src/arithmetic.rs` | `packages/loot-core/src/shared/arithmetic.ts`; `currencyToAmount` and `getNumberFormat` in `packages/loot-core/src/shared/util.ts` |
| `crates/veritas-engine/src/amount_field.rs` | `amountToInteger` in `packages/loot-core/src/shared/util.ts`; `forEdit` in `packages/desktop-client/src/hooks/useFormat.ts`; the single-operator rule in `InputValue`, `packages/desktop-client/src/components/table.tsx` |
| `crates/veritas-engine/src/envelope.rs` | `packages/loot-core/src/server/budget/envelope.ts`; `createAllBudgets`, `getBudgetRange` and the `sum-amount` query in `packages/loot-core/src/server/budget/base.ts` |
| `crates/veritas-engine/tests/arithmetic.rs`, `crates/veritas-engine/tests/fixtures/actual-arithmetic-vectors.json` | `packages/loot-core/src/shared/arithmetic.test.ts` (test vectors), and outputs of the Actual code above |
