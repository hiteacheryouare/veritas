# Starter categories (D52, D54)

Status: approved by Ryan on 2026-11-11 (D54). Owner: Fred. Builds on classification.md (CLS-n), tax-buckets.md (TAX-n) and debt.md (DEBT-n).

A new Veritas budget (no Actual involved, D52) is an Actual-format file created from the pinned template (`docs/actual-format.md` §3.1, decision 1). Veritas then applies this set once, at creation, as ordinary category and group writes. After that the user owns every category and may rename, move, hide or delete any of them. Nothing in this file is re-applied to an existing budget.

## SC-1 Definitions

| Term | Meaning |
|---|---|
| Starter set | The groups and categories in SC-3, with the kind and tax bucket in the tables. |
| Seed rows | The 3 groups and 7 categories in Actual's `default-db.sqlite` (`docs/actual-format.md` §3.1). They carry fixed ids in every Actual budget. |
| Kind | `veritas_category_meta.kind` (CLS-3). Written for every starter category, including income and expense, so nothing relies on a default. |
| Tax bucket | `veritas_category_meta.tax_bucket` (TAX-2). A label for the user's own tagging, never a claim about deductibility (TAX scope). |

## SC-2 Rules

- **SC-2.1** Every starter category gets exactly one kind. `income` only where `is_income = 1`, so CLS-3.1 never raises `kind_conflict` on a fresh budget.
- **SC-2.2** Exactly one group has `is_income = 1`: "Income". It holds exactly the five income categories in SC-3. No other group holds an `is_income = 1` category.
- **SC-2.3** "Starting Balances" must exist with `is_income = 1`, in the "Income" group, with that exact name (case-insensitive match is how Actual finds it, `lc/server/accounts/payees.ts:18-36`, `docs/actual-format.md` §4.8). Keep seed id `506e8d9d-…`. It is visible, not hidden. Kind `income`, no tax bucket.
- **SC-2.4** All seed rows are reused by id (SC-4), renamed and moved by ordinary updates. None is deleted or tombstoned. `category_mapping` keeps its identity rows. A new budget has no transactions, so no history is rewritten.
- **SC-2.5** Names are plain nouns. No emoji, no jokes, no filler.
- **SC-2.6** Nothing is hidden by default (SC-5).
- **SC-2.7** Paychecks are recorded gross with withholding children (CLS-10, FIN-4), so the "Paycheck Deductions" group exists in the starter set. Those categories are `expense` or `savings`, not `income`.
- **SC-2.8** Loan payments use "Loan Principal" for the principal part, "Interest and Fees" for interest and fees, and "Mortgage Escrow" for escrow (DEBT-2, DEBT-2.4). Escrow never goes in principal or interest.
- **SC-2.9** Refunds go back into the category they came out of and reduce its spending. They are not income (CLS-6). The set therefore has no "Refunds" or "Reimbursements" category.

## SC-3 The set

35 categories in 9 groups, in this order. Groups and categories display in the order listed (`sort_order` strictly increasing, with no ties, including across the seed's two groups that share 32768).

| # | Group | Category | Kind (CLS-3) | Tax bucket (TAX-2) | Seed row reused (SC-4) |
|---|---|---|---|---|---|
| 1 | Housing | Rent | expense | — | Bills |
| 2 | Housing | Mortgage Escrow | expense | — | |
| 3 | Housing | Home Insurance | expense | — | |
| 4 | Housing | Home Maintenance | expense | — | |
| 5 | Utilities | Electricity and Gas | expense | — | Bills (Flexible) |
| 6 | Utilities | Water and Trash | expense | — | |
| 7 | Utilities | Internet and Phone | expense | — | |
| 8 | Food | Groceries | expense | — | Food |
| 9 | Food | Dining Out | expense | — | |
| 10 | Transportation | Fuel and Transit | expense | — | |
| 11 | Transportation | Car Insurance | expense | — | |
| 12 | Transportation | Car Maintenance | expense | — | |
| 13 | Health | Health Insurance | expense | — | |
| 14 | Health | Medical | expense | medical | |
| 15 | Personal and Family | Clothing and Personal Care | expense | — | |
| 16 | Personal and Family | Childcare | expense | — | |
| 17 | Personal and Family | Education | expense | education | |
| 18 | Personal and Family | Entertainment | expense | — | |
| 19 | Personal and Family | Gifts | expense | — | |
| 20 | Personal and Family | Charity | expense | charitable | |
| 21 | Personal and Family | Miscellaneous | expense | — | General |
| 22 | Paycheck Deductions | Federal Income Tax | expense | taxes_withheld | |
| 23 | Paycheck Deductions | State Income Tax | expense | taxes_withheld | |
| 24 | Paycheck Deductions | Social Security and Medicare | expense | taxes_withheld | |
| 25 | Paycheck Deductions | Retirement Contribution | savings | retirement_contribution | |
| 26 | Paycheck Deductions | HSA Contribution | savings | hsa | |
| 27 | Savings and Debt | Savings | savings | — | Savings |
| 28 | Savings and Debt | Investments | savings | — | |
| 29 | Savings and Debt | Loan Principal | debt_principal | — | |
| 30 | Savings and Debt | Interest and Fees | expense | — | |
| 31 | Income | Wages | income | w2_income | Income |
| 32 | Income | Interest | income | interest_income | |
| 33 | Income | Business Income | income | business_income | |
| 34 | Income | Other Income | income | — | |
| 35 | Income | Starting Balances | income | — | Starting Balances (required) |

Group order: Housing, Utilities, Food, Transportation, Health, Personal and Family, Paycheck Deductions, Savings and Debt, Income.

Notes on the tax column:

- Buckets are set only where the category's name already says what it is: wages, interest, business income, medical, education, charity, withholding, retirement, HSA. A category with no bucket (—, stored as NULL) can still be tagged per transaction (TAX-3). `none` is only TAX-2's tag override, never a category default.
- Health Insurance has no bucket on purpose. A premium paid through the paycheck is not the same thing as one paid out of pocket, and Veritas does not decide which is reportable.
- Tax-bucket names, never dollar limits or rates, appear here. This file contains no tax numbers.

## SC-4 Actual seed rows

Source: `docs/actual-format.md` §3.1.

| Seed row | Id prefix | Treatment |
|---|---|---|
| Group "Income" (is_income 1) | `2E1F5BDB-…` | Kept. Same id, same name, `is_income = 1`. Sorted last. |
| Group "Usual Expenses" | `fc3825fd-…` | Kept, renamed "Housing". |
| Group "Investments and Savings" | `a137772f-…` | Kept, renamed "Savings and Debt". |
| Category "Starting Balances" (is_income 1) | `506e8d9d-…` | Kept unchanged (SC-2.3). |
| Category "Income" (is_income 1) | `3c1699a5-…` | Kept, renamed "Wages". |
| Category "Bills" | `d4b0f075-…` | Kept, renamed "Rent". |
| Category "Bills (Flexible)" | `29ec2c58-…` | Kept, renamed "Electricity and Gas", moved to Utilities. |
| Category "Food" | `541836f1-…` | Kept, renamed "Groceries", moved to the new Food group. |
| Category "General" | `af375fd4-…` | Kept, renamed "Miscellaneous", moved to Personal and Family. |
| Category "Savings" | `6bbd8472-…` | Kept, name unchanged, `is_income = 0`, in Savings and Debt. |

No seed row is deleted. All 7 categories and all 3 groups are reused, so `category_mapping` and the template's seed ids stay valid and nothing is tombstoned. Renames and moves are plain updates. The other 28 categories and 6 groups are new rows with new ids and identity `category_mapping` rows, written the way Actual's own create-category does.

Seed `template_settings` (`{"source": "notes"}`) is untouched (`docs/actual-format.md` §3.1).

## SC-5 Hidden by default

Nothing. Reasons: hiding would make a day-one user think a category is missing, and "Starting Balances" must stay visible and usable as Actual expects. If the user hides or deletes any starter category later, its kind and history stay (CLS-3.2, R-REP-3).

## SC-6 Rationale

- Nine groups follow how a household thinks about monthly money: where I live, what keeps the lights on, food, getting around, health, everyone and everything else, what the paycheck takes before I see it, what I put away or owe, and what comes in.
- 30 spending categories is the practical range. Fewer hides where money goes, more makes data entry (D14, everything is entered by hand) a chore. Subscriptions and Pharmacy are not separate: they fold into Entertainment and Medical, and the user splits them out when a real need shows up.
- The debt trio matches DEBT-2: principal is `debt_principal`, interest and fees are an expense, escrow is its own expense (FIN-3).
- Paycheck Deductions exists because FIN-4 records paychecks gross. Federal, state, and Social Security and Medicare are `taxes_withheld`. Retirement and HSA are `savings` so they are not counted as spending (CLS-10 row 7). Those two need off-budget accounts to receive the transfer, or they classify as misfiled `expense` (CLS-10, row 14). New-budget setup offers to create them (D56).
- Tax buckets exist so the TAX export is useful on day one (R-REP-8) without Veritas asserting anything about deductibility.
- Savings, Investments and Loan Principal are envelopes you fund each month (CLS-9), so they appear in the budget like any other.

## SC-7 Worked examples (made-up amounts, cents)

| Case | Setup | Result |
|---|---|---|
| SC-E1 Fresh budget identities | No transactions | All 35 categories have kind and bucket per SC-3. `income_total`, `expense_total`, `debt_paydown`, `savings_total` are all 0. |
| SC-E2 Starting balance | New account CHK, balance 150000 | Actual writes the row in "Starting Balances" (`is_income = 1`). Row kind `starting_balance` (CLS-4), not income, not in Net Profit. |
| SC-E3 Gross paycheck (CLS-10 shape) | Net deposit +234400. Children: Wages +400000, Federal Income Tax -50000, State Income Tax -20000, Social Security and Medicare -30600, Retirement Contribution -40000 (to off-budget 401k), HSA Contribution -10000 (to off-budget HSA), Health Insurance -15000 | `income_total` 400000. `expense_total` 115600. `net_profit` 284400. `savings_total` 50000. `net_cash_flow` 234400. Tax buckets: `w2_income` 400000, `taxes_withheld` 100600, `retirement_contribution` 40000, `hsa` 10000. |
| SC-E4 Mortgage payment with escrow (DEBT-5 L-4) | Total 200000 = interest 100000 + escrow 40000 + principal 60000 | Interest and Fees 100000 and Mortgage Escrow 40000 are expense: 140000. Loan Principal 60000 is `debt_principal`. Parts sum to 200000. |
| SC-E5 Refund in Groceries | Groceries -12000, later refund +2500, same period | Groceries spent 9500. Not income. |
| SC-E6 Refund across a period boundary | Dining Out -8000 in March, refund +3000 in April | March spent 8000. April spent -3000 (a negative total is shown, never clamped, CLS-6). |
| SC-E7 Card payment | CHK pays the card bill 50000 | Transfer. No category from the set is touched. The purchases were spending when swiped. |
| SC-E8 Zero-history category | Education has no rows | Budget and reports show 0. Education bucket total 0 in the tax export. |
| SC-E9 Interest on savings | Bank pays +1234 into Interest | `income` 1234, bucket `interest_income` 1234. |
| SC-E10 Savings with no off-budget account | Savings and Debt "Savings" with an on-budget transfer, no off-budget account | Row 4 (`internal_transfer`, `needs_review`, because a category is present). The user should move the savings to an off-budget account, which makes it a savings contribution. |

## SC-8 Ryan's answers

1. The new-budget setup offers to create off-budget 401(k) and HSA accounts for the ones the user checks (D56).
2. No Emergency Fund category; the set ships as proposed (D54). A user can add one as a `savings` category.
3. Withholding categories are normal envelopes. Suggestions (R-FC-3) propose the usual withholding, and Veritas never budgets them on its own (D57).
