# AI answers (R-AI-1)

Status: draft, revised for the PR #93 review round, with budget habits (AI-10) and wording (AI-11). Owner: Fred. Builds on classification.md (CLS-n) and statements.md (ST-n). Applies to the on-device assistant's read-only Q&A. Amounts are integer minor units. The core computes every figure, including the habits averages (AI-10.5) and the `Everything else` sum (AI-12.5), and the app only formats them. Examples use made-up names and amounts, a 2-place currency written as `$` unless an example says otherwise, and cents in the data lines.

## AI-1 Who computes what

- **AI-1.1** The app resolves the period from the user's words against today's date, the core computes every figure, and the app formats the answer. The model never computes a date, a sum, a difference, a percentage or a conversion, and never states a figure that no tool returned.
- **AI-1.1a** The model's instructions carry this sentence exactly: "Call a tool for every question about money, every time, even one asked before. Every figure you give must come from a tool you called for this question." A figure from an earlier answer in the conversation is not a figure from a tool called for this question.
- **AI-1.2** Each tool's first line is the answer. The model may add words around it but never changes a figure, a label or a date range. The model's instructions carry this sentence exactly: "Start your answer with the first line the tool returns, copied exactly. It names the figure and the dates it covers, and it is the answer: the lines after it are detail, even one with the same word, and never replace it. Then copy the tool's other lines as they are. Keep anything you add to one short sentence." A later line that shares a word with the question, such as a category line under a total, is detail and never the answer.
- **AI-1.3** The model offers no figure for a range no tool returned. A question about a range the tools cannot resolve gets the tool's own refusal, not an estimate. The model's instructions carry this sentence exactly: "Never give a figure for a time range a tool did not return. If a tool refuses the range, say so."
- **AI-1.4** No tool writes anything. Hypothetical questions never touch real data.
- **AI-1.5** Every amount is formatted by the app with the currency symbol and places (CLS-12.3). The templates below write `<amount>` for that text, and write a negative with `-`, where the app prints its minus sign (U+2212). The model never reformats an amount. AI-2 to AI-5 and AI-9 are on the register scale (R). AI-7 is on the budget scale (B), as the Budget screen formats it (CLS-12.10).
- **AI-1.6** **Grounding.** Every figure, and every category or group name, in an answer appears in the output of a tool called for that question. A figure or name the tools did not return, such as an invented category presented as if a tool had returned it, violates AI-1.1. This is a testable rule. The real-model evaluation reads the tool's own output for the same question and fails the answer when:
- **AI-1.7** **A read that cannot finish.** When a core read overflows a checked sum, such as the habits sums of AI-10.5 or the `Everything else` sum of AI-12.5, it returns an error and never a figure. The tool turns that error into one answer line, copied exactly: `These figures are too large to add up exactly, so there is no figure to give.` That line is the whole answer, and it is the "first line" of AI-1.2 with nothing after it. No estimate, partial list or rounded figure follows, and the model repeats the line and adds nothing, not even the one short sentence AI-1.2 otherwise allows. AI-1.3's refusal is about a time range the tools cannot resolve and does not cover this case. Any other read error (a budget that cannot be read, a cancelled read) is the app's own failure message, shown by the panel, and never an answer written by the model. **Code follow-up:** today the overflow surfaces as the core's error text, and the tool should return the line above.
  - its headline figure and dates are not the tool's first line (AI-1.2), or
  - its set of figures is not a subset of the tool's figures, or
  - it names a category or group the tool did not name.

  The by-category questions (AI-12) are checked this way.

## AI-2 Spending (CLS-6, ST-1)

- **AI-2.1** "Spending" is the Expenses line of Net Profit: `expense_total` (CLS-6), the same figure as ST-1 for the same period. Refunds reduce it and it can be negative (ST-1.2). Hidden and archived categories are included and labeled (ST-1.3).
- **AI-2.2** `spending(period)` answers with three parts, in this order:
  1. `Spending, <range>: <expense_total>`
  2. `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
  3. The heading `Spending by category group, <range>:`, then one line per category group, for example `Everyday: $512.00`, then the lines of AI-2.5.
- **AI-2.3** The assistant never calls transfers, card payments, net cash flow, debt paydown or savings "spending", and never calls a refund income (CLS-2, ST-1.1). When a question calls any of them spending, the answer contains this sentence exactly: "Transfers, card payments, net cash flow, debt paydown and savings are never spending."
- **AI-2.4** The ST-10.10 cards (including "Monthly spending") use raw cash movement and are not this figure. The assistant never quotes a dashboard card number as spending.
- **AI-2.5** The group lines reconcile to `expense_total` to the cent, like the ST-10.8 Total. A group line uses the ST-10.3 label of the group, for example `Everyday (hidden)`, and a group with no name reads `Unnamed group`. After the group lines come one line for each expense amount that belongs to no group:
  - rows with no category: `Uncategorized (needs review): <amount>` (CLS-4 row 15). It can be negative, because a positive uncategorized row reduces expenses.
  - a category whose group record no longer exists is in no group, and shows by its own ST-10.3 label, for example `(deleted) Old Gym: $12.00`.

## AI-3 Categories and groups

- **AI-3.1** **One figure per kind.** For a category or group and a period, each figure is the CLS-6 sum over rows of one row kind (`expense`, `income`, `debt_principal`, `savings_contribution`) whose resolved category (CLS-1) is that category, or any category currently in that group. A group's Spending counts only `expense` rows, never every row of the group's categories. The category's own kind is never a second classifier (CLS-3, CLS-4): a purchase booked to a debt or savings category with no transfer is an `expense` row (CLS-4 row 14, `misfiled`) and counts as Spending. The category's kind is read in two places only, AI-3.4 and AI-10.2.
- **AI-3.2** Totals are summed in integer cents in the core over the same classified rows as the Net Profit screen (CLS-1, CLS-6), so a figure equals its line in the drill-down (ST-1.6). Signs follow CLS-6: Expenses, Debt paydown and Savings are positive for money out, Income is positive for money in.
- **AI-3.3** **Lines.** The answer lists every kind that has at least one row, in the order Expenses, Income, Debt paydown, Savings, each with its own label:
  - `Spending on <label>, <range>: <amount>`
  - `Income from <label>, <range>: <amount>`
  - `Debt paydown for <label>, <range>: <amount>. It is not spending.`
  - `Savings for <label>, <range>: <amount>. It is not spending.`

  The first line is followed by `From <n> transactions, on the Expenses line of the Net Profit report.` (`on the Income line` for an Income first line). It reads `From 1 transaction` for one. n counts the classified rows of that line's kind: split children count, split parents and mirror legs do not (CLS-1). The Debt paydown and Savings lines carry their own sentence and no count. Naming a category or group counts as asking for it by name under AI-5.1, for that category or group only. `spending(period)` never does.
- **AI-3.4** **Nothing to report.** A category or group with no rows of any kind answers one line with `0.00` in the currency's places. It reads `Income from <label>` for an Actual income category or group, and `Spending on <label>` for anything else, with the matching second line. This is one of the two places the category's own kind is read (the other is AI-10.2).
- **AI-3.5** `<label>` is the ST-10.3 label, the same function the reports use: `Groceries (hidden)`, `Old Gym (hidden, archived)`, `Old Gym (closed goal)`, `(deleted) Old Gym`. Groups carry the same labels.
- **AI-3.6** **Matching names.** `categorySpending(name, period)` matches a category or a category group by a key. Both the asked name and each candidate name become a key the same way: fold case and accents, read `&` as `and`, treat any run of characters that is not a letter or digit as a word break, drop a leading `the`, `my` or `our`, then make each word singular (`ies` to `y` for words over 4 letters, otherwise drop a trailing `s`, but not `ss`, for words over 3 letters). On the asked name only, a trailing `category`, `categories` or `envelope` narrows the match to categories, `group` narrows it to groups, and `<name> in <group>` narrows it to that group's category.
  - **Exactly one exact key match:** it answers.
  - **More than one exact match, of any mix:** no figure, and `More than one category or group is called "Food": the category Food in Everyday, the category group Food. Ask which one.`
  - **No exact match:** no figure, and the closest names (AI-3.7).
- **AI-3.7** **Closest names.** At most 3, from the candidates ranked in tiers: (1) the candidate key starts with the asked key, (2) it contains it, (3) the asked key contains the candidate key, (4) any other candidate, kept only if its edit distance (Levenshtein, on keys) from the asked key is at most 2. Within a tier, sort by edit distance, then by name ignoring case. A name appears once. The answer reads `No category or group is called "groc". The closest name is Groceries.` for one, `The closest names are Groceries, Gifts.` for several, and just `No category or group is called "zzz".` for none. The core never guesses which one was meant.

## AI-4 Income (CLS-6)

- **AI-4.1** `income(period)` first line: `Income, <range>: <income_total>`. Second line: `This is the Income line of the Net Profit report.` When there are income categories, the heading `Income by category, <range>:` follows, then one line per income category, `From <category>: <amount>`, for example `From Salary: $5,000.00`. The category is labeled as AI-3.5 labels it. With no income categories the answer is the two lines alone.
- **AI-4.2** "Income" answers questions about earnings only. It is `income_total`, net of reversals (CLS-6). Refunds are never income. Transfers in, loan proceeds and savings withdrawals are never income.

## AI-5 Debt paydown and savings

- **AI-5.1** Debt paydown and savings are never quoted unless the user asks for them by name. Naming a category or group (AI-3.3) or asking for the Net Profit report or net cash flow (AI-9) is asking by name. `spending(period)` and `income(period)` never quote them.
- **AI-5.2** When quoted, the figure carries its own label (`Debt paydown, <range>`, `Savings, <range>`, and for one category or group `Debt paydown for <label>, <range>` or `Savings for <label>, <range>`), never "spending", and the line says `It is not spending.` Interest, fees and escrow are expenses and are not debt paydown (ST-1).
- **AI-5.3** A negative figure adds a sentence. Savings: `Negative means more was withdrawn than saved.` Debt paydown: `Negative means more was borrowed than paid down.`

## AI-6 Periods

- **AI-6.1** The label always states the exact range used. A whole calendar month or year reads `November 2026` or `2026`. A partial range reads `November 1 to 22, 2026` or `January 1 to November 22, 2026`, with the year stated once at the end when both ends share it. A partial range across years reads `December 15, 2025 to January 3, 2026`. A whole-period label is never used for a partial range. An explicit range that equals a whole month or year gets the whole label. A span of whole months reads `<first month name> to <last month name> <year>` when both ends share a year (the year once, at the end), and `<first month name> <year> to <last month name> <year>` when they do not, as AI-10.1 words its window labels. A span of whole years across different years is that same form: 2024-01-01 to 2025-12-31 reads `January 2024 to December 2025`. One whole year reads `2026`.
- **AI-6.2** Resolution, with today = 2026-11-22 unless the row says otherwise:

| User's words | Range | Label |
|---|---|---|
| nothing named, "this month" | 2026-11-01 to 2026-11-30 | November 2026 |
| "so far this month", "this month so far", "month to date" | 2026-11-01 to 2026-11-22 | November 1 to 22, 2026 |
| "last month" | 2026-10-01 to 2026-10-31 | October 2026 |
| "last month", today = 2026-01-15 | 2025-12-01 to 2025-12-31 | December 2025 |
| "this year" | 2026-01-01 to 2026-12-31 | 2026 |
| "so far this year", "year to date" | 2026-01-01 to 2026-11-22 | January 1 to November 22, 2026 |
| "last year" | 2025-01-01 to 2025-12-31 | 2025 |
| "this month last year", "same month last year" | 2025-11-01 to 2025-11-30 | November 2025 |
| "October" alone | the latest October not after today's month: 2026-10 | October 2026 |
| "October 2026", "2026-10" | as written, whole month | October 2026 |
| "February 2024" | 2024-02-01 to 2024-02-29 | February 2024 |
| "2026" | as written, whole year | 2026 |
| `2026-10-01 to 2026-10-31` | as written, equals a whole month | October 2026 |
| `2025-12-15 to 2026-01-03` | as written | December 15, 2025 to January 3, 2026 |
| `2026-11-10 to 2026-11-05` | start after end | refused with the period hint, no figure |

- **AI-6.3** "This month" and "this year" are the whole calendar period, matching the Net Profit screen, including days after today. "So far" and "to date" end today. Example: on 2026-11-22, "October" alone is October 2026. On 2026-09-05 it is October 2025. On 2026-10-05 it is October 2026, whole month.
- **AI-6.4** A row belongs to the period containing its own date (conventions, top of classification.md). A refund dated in a later period than its purchase reduces the later period only, as in CLS-8 C-1.
- **AI-6.5** **Words are read by the core.** The core's `resolvePeriod(text, today)` resolves the user's words against today and returns the range and its AI-6.1 label, or a refusal. The app and the model do no date arithmetic. The words are folded for case and accents, curly apostrophes are made straight, and runs of whitespace count as one space. **English month names only**: the full names and their 3-letter abbreviations, and `sept`. The Mac's own language does not add month names. **Decision (Architect):** month names are English only, because the app has no localizations. If the app is localized later, month names are added by a new spec section and not by reading the system's language. A month name in another language is an unread word and the whole question is refused (AI-6.11).
- **AI-6.6** **Ranges written in words.** A range names two months. The year of each end is the year written beside it, and a year written before the first month name belongs to the first month name, as in "2025 November". Written years always win. A year beside a month name that already has one is a refusal (AI-6.8).
  - **Both ends have a year:** as written. "November 2025 to January 2026" is 2025-11-01 to 2026-01-31, and "November 2024 to January 2026" is the 15 months between.
  - **Only the end has a year:** the start is the latest month of its name that is not after the end. "November to January 2026" is 2025-11-01 to 2026-01-31, and "January to January 2026" is January 2026.
  - **Only the start has a year:** the end is the first month of its name on or after the start. "November 2025 to January" is 2025-11-01 to 2026-01-31, and "October 2025 to October" is October 2025 alone, because the start already matches. A range may run into the future: "November 2026 to January", on 2026-11-22, is 2026-11-01 to 2027-01-31, whole months, and is neither refused nor clamped to today. Clamping would change the range the label states, and a whole current month already includes days after today (AI-6.3). "So far" or "to date" does cut a range that holds today at today (AI-6.3). A period wholly after today is read the same way, and the tools report the rows dated in it, which is usually none, as the Net Profit screen does for the same range.
  - **No year at all:** the end is the latest month of its name that is not after today's month, and the start is the latest month of its name that is not after that end. The current month counts whole. "December to February" on 2026-11-22 is 2025-12-01 to 2026-02-28, on 2026-02-10 it is the same range with February still counting whole, and on 2026-01-10 it is 2024-12-01 to 2025-02-28, because this February has not come yet. "this year" or "last year" beside month names gives the end its year: "March to May this year" is 2026-03-01 to 2026-05-31.
  - **One month name** follows the same rules as a range of one month (AI-6.2, "October" alone).
  - **A connector is required.** Two months, two years, two dates or two `YYYY-MM` are a range only with a connector between them: `to`, `through`, `thru`, `until`, `till`, or a hyphen or en dash between the two. "from X to Y" is X to Y. Without one, "October and November", "October versus November", "October November" and "2024 2025" are refused. `2025-11 to 2026-01` is a range of whole months, November 2025 to January 2026, and each end may be a `YYYY-MM` or a month name with the year rules above.
  - **"Beside" means adjacent.** A year or a day number is beside a month name only when it comes directly before or after it, with no word or connector between. A year that is not beside a month name has none to belong to, so it is refused when month names are present: "November to 2026" and "2025 to November" are refused, while "2025 November to January" and "November 2025 to January" are read.
  - **Phrases are not combined unless listed.** These pairs are read: a month name, a quarter or a half with `this year` or `last year`, and `this month` or `same month` with `this year` or `last year`. Any other pair of period phrases is refused: "October this month", "last month in October", "this month last month", "this year last year", "Q4 last month", or a year written beside `this year` or `last year`.
- **AI-6.7** **Years written alone.** Two years, "2024 to 2025", mean whole years, 2024-01-01 to 2025-12-31, in order, and the label is the whole-month form `January 2024 to December 2025`. Years in the wrong order are a refusal. A single year is the whole year (AI-6.2). "So far" or "to date" stops a span that holds today at today. A year is in 1900 to 2200, and so is a year reached by "last year", "last month" or any phrase: a period outside it is refused.
- **AI-6.8** **Refusals.** The core refuses, with the hint and no figure, when:
  - a month has two years ("November 2025 2026"), or a year has no month name to belong to while month names are present;
  - a year is outside 1900 to 2200, or the range's start is after its end;
  - more than two dates are written, or a date is written beside any month name, year or `YYYY-MM`. Dates stand alone: one date, or two in order (`2026-10-01 to 2026-10-15`);
  - more than two month names are written ("January, March and June"), because that is a list of months and not a range, and reading its first and last as a range would answer a different question;
  - two items have no connector between them, or a year is not beside a month name (AI-6.6), or two period phrases are combined that AI-6.6 does not list;
  - "so far" or "to date" is written on a period that lies wholly after today ("March 2027 so far"), because nothing has happened in it yet;
  - a day number is written beside a month name ("October 15", "October 1 to 15"). A period inside a month is written as dates, `2026-10-01 to 2026-10-15`, and the hint says so. The core never widens a day to its month;
  - a word is not understood (AI-6.11);
  - the words name no period it reads.

  The hint is exactly: `Ask again with a period such as this month, so far this month, last month, this year, October 2026, 2026, or 2026-10-01 to 2026-10-15.`
- **AI-6.9** **An invalid today is an error, not a refusal.** A `today` that is not a real date, or is outside 1900 to 2200, is the app's bug, not the user's wording, so `resolvePeriod` returns an error and no hint.
- **AI-6.10** **Examples (tests).** Today is 2026-11-22 unless a row says otherwise.

| User's words | Range | Label |
|---|---|---|
| "November 2025 to January 2026" | 2025-11-01 to 2026-01-31 | November 2025 to January 2026 |
| "November 2025 to January", "November to January 2026", "November to January" | 2025-11-01 to 2026-01-31 | November 2025 to January 2026 |
| "November 2024 to January 2026" | 2024-11-01 to 2026-01-31 | November 2024 to January 2026 |
| "November 2026 to January" | 2026-11-01 to 2027-01-31 | November 2026 to January 2027 |
| "November 2026 to January so far" | 2026-11-01 to 2026-11-22 | November 1 to 22, 2026 |
| "October 2025 to October" | 2025-10-01 to 2025-10-31 | October 2025 |
| "September to November" | 2026-09-01 to 2026-11-30 | September to November 2026 |
| "December to February" | 2025-12-01 to 2026-02-28 | December 2025 to February 2026 |
| "December to February", today 2026-01-10 | 2024-12-01 to 2025-02-28 | December 2024 to February 2025 |
| "March to May this year" | 2026-03-01 to 2026-05-31 | March to May 2026 |
| "2024 to 2025" | 2024-01-01 to 2025-12-31 | January 2024 to December 2025 |
| "2025 November" | 2025-11-01 to 2025-11-30 | November 2025 |
| "2025 November to January" | 2025-11-01 to 2026-01-31 | November 2025 to January 2026 |
| "2025 to 2024", "November 2025 2026" | refused | the hint |
| "October 1899", "2201-01" | refused | the hint |
| "2026-10-01 to 2026-10-15 to 2026-10-20" | refused (three dates) | the hint |
| "2026-10 to 2026-10-15", "October 2026-10-15" | refused (a date beside a month) | the hint |
| "January, March and June" | refused (three month names) | the hint |
| "October 15", "October 1 to 15" | refused (a day beside a month name) | the hint |
| "mars" (French), "Mai" (German) alone | refused (English names only) | the hint |
| today "2026-02-30" | an error, no hint | |


- **AI-6.11** **Every word must be understood.** The core reads only these words, each with the meaning given. A word outside this list refuses the whole question with the hint, and nothing is guessed from the words that were understood. So "Novembre 2025" is refused, not read as 2025, "may I see this year" is refused, not read as May, and "January and March" is refused. A lone "may" is the month May, because it is on its own. This stops the period words from being a place the model's other words can hide a different question.
  - **Names and numbers:** month names and abbreviations (AI-6.5), numbers, years and dates, `q1` to `q4` and `h1`, `h2` (AI-6.13).
  - **Period phrases:** `this`, `last`, `month`, `year`, `same`, `today`, `so`, `far`, `date`, `ytd`, `mtd`, as in AI-6.2. `previous` means `last` in "previous month" and "previous year". `past` is not read: "the past year" usually means the trailing 12 months and not the last calendar year, so "past month", "past year" and "past 12 months" are refused with the hint and not guessed. `current` means `this` in "current month" and "current year". A hyphen inside a known phrase reads as a space: "year-to-date", "month-to-date" and "so-far" are `year to date`, `month to date` and `so far`, and the hyphen is a range connector only between two periods (AI-6.6).
  - **Connectors:** `to`, `through`, `thru`, `until`, `till`, and `up to` (which is `to`, so "up to now" is the open end of AI-6.12). `from` is part of "from X to Y" and means nothing alone, so "from October" is refused. `since` (AI-6.12). `now` and `present` are the open end of AI-6.12, and `the` may precede `present`.
  - **Introducers:** `in`, `for` and `during` may precede a period and add nothing ("in October" is October, "for 2025" is 2025, "during Q4 2025" is Q4 2025). `of` may stand only between a month name, a quarter or a half and a year or a year phrase ("October of last year", "first half of 2026"). `the` may stand only before `present` or before a quarter or half ("the first half of 2026").
  - **Quarter words:** `quarter`, `half`, `first`, `second`, `third` and `fourth` only as a quarter or half (AI-6.13).
- **AI-6.12** **Open ends: "since" and "to now".** `since <start>` and `<start> to now` (also `until now`, `through today`, `to the present`, with `now`, `today` or `present` after the connecting word) read from the start to today. The start is a month (the latest such month not after today's month if no year is written), a year, a quarter or a half (the first day of the latest such quarter or half that has started, if no year is written), or a date: its first day. "since Q3" and "since 2025 November" are read. The label is the partial form, `October 1 to November 22, 2026`. A start after today is refused. These words do not stop at the named period: "October to now" on 2026-11-22 is 2026-10-01 to 2026-11-22, whereas "October so far" and "October to date" only cut a period that holds today at today (AI-6.3), so on 2026-11-22 they are all of October. "since November" on 2026-11-22 is 2026-11-01 to 2026-11-22. With no start, `until now` is "so far this month" (AI-6.2).
- **AI-6.13** **Quarters and halves.** `Q1` to `Q4`, "first" to "fourth quarter", `H1` and `H2`, "first half" and "second half", each with an optional year and the words `of` and `the`. A quarter is three whole calendar months (Q1 January to March, Q4 October to December), a half is six (the first half January to June). With a year it is that year's. Without one it is the latest such quarter or half that has started, so the current one counts whole, including days after today: on 2026-11-22 "Q4" is 2026-10-01 to 2026-12-31 and "second half" is 2026-07-01 to 2026-12-31, and on 2026-03-10 "Q4" is Q4 2025. "this year" and "last year" give the year. The label is the whole-month form (`October to December 2025`, `January to June 2026`). One quarter or half only: two, or one beside a month name, is refused. "So far" cuts it at today (AI-6.3).
- **AI-6.14** **A single date and the balance sheet.** One written date, or "today", is a one-day period: `first == last`, and the label is `November 22, 2026`. The balance sheet tool (ST-2, as of a date) reads its words through the core and takes the period's last day as its date:
  - a single date or "today" is that date. A date after today is **refused**, with the hint `Ask again with today or an earlier date, such as today or 2026-10-31.`, because a balance on a day that has not come is not a recorded fact, and answering for another date would change the question. The old clamp of a later date to today is withdrawn;
  - a period (a month, a year, "this month", "last month") is the balance at its last day, and a period whose last day is after today is the balance as of today. "this month" on 2026-11-22 is as of 2026-11-22, and "last month" is as of 2026-10-31;
  - the first line states the date used: `Balance sheet as of November 22, 2026`.
- **AI-6.15** **The tax year.** The tax summary tool (TAX-5) reads one whole calendar year through the core: the words must resolve to one whole year, 1 January to 31 December of the same year ("2025", "last year", "this year"). Anything else is refused with the existing hint `Ask again with a calendar year such as 2026.`: two years ("2024 to 2025"), a month or quarter, a partial year ("so far this year", "year to date"), or a single date, and a year that has not started ("2027" on 2026-11-22), for which no tagged rows exist yet. A summary has no partial form, and reading "year to date" as the whole year would change what was asked.

Added examples, today 2026-11-22 unless a row says otherwise:

| User's words | Range | Label |
|---|---|---|
| "since October" | 2026-10-01 to 2026-11-22 | October 1 to November 22, 2026 |
| "October to now", "October through today" | 2026-10-01 to 2026-11-22 | October 1 to November 22, 2026 |
| "since October 2025" | 2025-10-01 to 2026-11-22 | October 1, 2025 to November 22, 2026 |
| "since 2026-10-15" | 2026-10-15 to 2026-11-22 | October 15 to November 22, 2026 |
| "since December" | 2025-12-01 to 2026-11-22 (the latest December not after today's month, then to today) | December 1, 2025 to November 22, 2026 |
| "since 2027" | refused (a start after today) | the hint |
| "October so far", "October to date" | 2026-10-01 to 2026-10-31 | October 2026 |
| "Q4 2025" | 2025-10-01 to 2025-12-31 | October to December 2025 |
| "Q4" | 2026-10-01 to 2026-12-31 | October to December 2026 |
| "Q4", today 2026-03-10 | 2025-10-01 to 2025-12-31 | October to December 2025 |
| "first half of 2026" | 2026-01-01 to 2026-06-30 | January to June 2026 |
| "second half" | 2026-07-01 to 2026-12-31 | July to December 2026 |
| "Q1 to Q3 2026", "Q4 and March" | refused (more than one quarter, or a quarter beside a month) | the hint |
| "Novembre 2025", "may I see this year", "January and March" | refused (an unread word) | the hint |
| "may" | 2026-05-01 to 2026-05-31 | May 2026 |
| "2026-10-01 to October 15" | refused (a date beside a month name) | the hint |
| balance sheet, "today" | as of 2026-11-22 | Balance sheet as of November 22, 2026 |
| balance sheet, "2026-12-31" | refused (a later date) | the balance-sheet hint |
| balance sheet, "this month" | as of 2026-11-22 | Balance sheet as of November 22, 2026 |
| balance sheet, "last month" | as of 2026-10-31 | Balance sheet as of October 31, 2026 |
| tax summary, "last year" | 2025 | the tax year 2025 |
| tax summary, "2024 to 2025", "so far this year", "October 2026", "2027" | refused | `Ask again with a calendar year such as 2026.` |
| the question "How much did I spend over the holidays last year?", whatever period the model passed | the tool does not run (AI-6.17) | the hint |
| "October and November", "October versus November", "October November", "2024 2025" | refused (no connector) | the hint |
| "October - December", "October through December" | 2025-10-01 to 2025-12-31 (the end is the latest December not after today's month, AI-6.6) | October to December 2025 |
| "October to December this year", "Q4" | 2026-10-01 to 2026-12-31 (this year, or the current quarter, is asked for in words) | October to December 2026 |
| "year-to-date", "month-to-date" | as "year to date" and "month to date" (AI-6.2) | January 1 to November 22, 2026, and November 1 to 22, 2026 |
| "since Q3" | 2026-07-01 to 2026-11-22 (a quarter is a start, AI-6.12) | July 1 to November 22, 2026 |
| "since 2025 November" | 2025-11-01 to 2026-11-22 | November 1, 2025 to November 22, 2026 |
| "in October", "during October" | 2026-10-01 to 2026-10-31 | October 2026 |
| "for 2025" | 2025-01-01 to 2025-12-31 | 2025 |
| "previous month" | as "last month" | October 2026 |
| "past month", "past year", "the past year" | refused (`past` is not read) | the hint |
| "current year", "current month" | as "this year", "this month" | 2026 and November 2026 |
| "October of last year", "the first half of 2026" | as AI-6.2 and AI-6.13 | October 2025 and January to June 2026 |
| "from October" | refused (`from` with no `to`) | the hint |
| "up to now" with no start | as "so far this month" | November 1 to 22, 2026 |
| "2025-11 to 2026-01" | 2025-11-01 to 2026-01-31 | November 2025 to January 2026 |
| "November to 2026", "2025 to November", "November 2025 to 2026" | refused (a year not beside a month name) | the hint |
| "March 2027 so far", "Q1 2027 so far" | refused (a period that has not started, so far) | the hint |
| "October this month", "last month in October", "this month last month", "this year last year" | refused (phrases not combined) | the hint |
| "March 2027" | 2027-03-01 to 2027-03-31 | March 2027, and the tools add `This period has not started.` after their first line |
| "this month", "this year" | as AI-6.2 | no `This period has not started.` line |

- **AI-6.16** **A period that has not started.** When a resolved period starts after today (`first > today`), a tool that reads recorded rows (`spending`, `spendingByCategory`, `categorySpending`, `income`, `netProfit` and `cashFlow`) adds the fixed line `This period has not started.` directly after its first line, so that a `$0.00` is not read as a finding. It does not change the first line. The line is not added to a period that has started and runs past today ("this month", "this year"), nor to the Budget screen's `budget` and `budgetMonth` tools, whose figures for a later month are plans and not rows, nor to the balance sheet, which refuses a later date (AI-6.14). Every pinned line after the first (AI-2.2, AI-3.3, AI-4.1, AI-9 for `netProfit`, AI-12.6 for `spendingByCategory`, and the cash flow statement's lines) moves down one line, and only when this line is present.
- **AI-6.17** **Time words the model dropped.** AI-6.11 refuses unread words in the period the tool receives, but the model chooses what to pass: "over the holidays last year" can arrive as "last year", and the answer would then be a different period than the one asked. The app therefore checks the user's own question before a tool with a `period` argument runs.
  - **Where it lives (PLAN section 6.1).** The list, the scan and the name spans are in the core, behind an FFI read such as `periodGuard(question:)`, which returns either "clear" or the AI-6.8 hint. The shell only passes the question and shows the hint. Nothing in Swift reads the list.
  - **The rule.** When the question holds a word from the **unread time words** below, outside every name span, the tool does not run and the answer is the period hint of AI-6.8, with no figure. This holds whatever period the model passed, including an empty one.
  - **Tokenization.** The question is folded for case and accents and split on every non-letter, so `past-due` gives `past` and `due`. Words are compared whole. A word that is followed directly by an apostrophe, with or without a following `s` (`day's`, `weeks'`), is a possessive and is never counted.
  - **Name spans.** Before the scan, each span of words that equals a whole name by its AI-3.6 key is removed: a category or group name, and a payee, account, goal or bill name. "on Holiday Gifts last year" has the span "Holiday Gifts" removed, leaves "on last year", and passes. "on Holiday Gifts over the holidays last year" leaves "holidays" after the span is removed, so it is refused. "Spring Street Cafe" and "Easter Seals", as payees, are spans and do not refuse. A name that is only part of a span, such as "Holiday" when the category is "Holiday Gifts", does not remove the word, because the span rule needs the whole name.
  - **The unread time words** are two closed lists, and a word is added only by adding it here with a test row.
    - **Single words that always count:** `holiday`, `holidays`, `christmas`, `thanksgiving`, `easter`, `summer`, `winter`, `spring`, `autumn`, `season`, `seasons`, `weekend`, `weekends`, `weekly`, `daily`, `yesterday`, `tomorrow`, `tonight`, `lately`, `recently`, `recent`, `ago`, `trailing`, `rolling`, `quarterly`, `fiscal`. Greetings are not on it: "morning", "afternoon" and "evening" are left off, because "Good morning, what did I spend last month?" is an ordinary question.
    - **Words that count only in a period use.** `past` counts only when the next word is a number, `few`, `couple`, `several` or a period noun (`day`, `days`, `week`, `weeks`, `month`, `months`, `quarter`, `quarters`, `year`, `years`), so "the past 3 months", "the past few weeks" and "the past year" count, "past due" does not, and "in the past" alone passes. `day`, `days`, `week` and `weeks` count only when the word before is a number, `this`, `last`, `next`, `past`, `each`, `every` or `per`, or the word after is `ago`, `before` or `after`, so "last week", "every day", "3 days" and "the day before" count, and "a good day", "the week's spending" and "day-to-day" do not.
  - **Limits that stay.** The guarantee of AI-6.11 is for the words the tools receive, and AI-6.17 closes it only for the listed words in the uses above. A time word on neither list, a listed word in a use the rules above do not count (a lone `day` or `week`), and any other language can still be dropped by the model, because the app cannot tell a time word from any other without a list. The real safeguard is the answer's first line, which states the exact range used (AI-6.1), so a wrong period is visible to the user. The list grows from the evaluation's failures.
  - **Tests.** Each always-counting word refuses a period question ("How much did I spend over the holidays last year?", "What did I spend this summer?", "Spending yesterday"). Context rows: refused are "the past 3 months", "the past few weeks", "the past year", "last week", "every day", "3 days ago" and "the day before". Passing are "my past-due bills last month", "in the past I spent a lot, what did I spend last month?", "the day's spending last month", "the week's spending last month" and "a good day, what did I spend last month?". "Good morning, what did I spend last month?" is not refused. The name spans: "on Holiday Gifts last year" passes, "on Holiday Gifts over the holidays last year" is refused, and "at Spring Street Cafe last month" passes when that payee exists. A question with none of the words is unchanged. The evaluation's "over the holidays last year" case expects the hint, and its test row is in AI-6.10.
## AI-7 Budget figures

- **AI-7.1** `budget(name, month)` first line: `Budgeted for <name>, <month>: <amount>`. Second line: `Budget screen figures for <month>: activity <a>, available <v>.`
- **AI-7.2** Activity is the Spent figure of Veritas's own Budget screen, as printed there. That screen prints money out as positive (`-spent`), unlike Actual's budget cell, which binds the raw `catSumAmount` and prints spending as negative (`reference/actual/packages/desktop-client/src/components/budget/envelope/EnvelopeBudgetComponents.tsx:473-477`). The core keeps Actual's sign. Only the Mac display flips it (D78, Gate 1 mockup D43). Sources: `apps/macos/Packages/VeritasMacUI/Sources/VeritasMacUI/Budget/EnvelopeRowView.swift:47` (`screen.format.amount(-envelope.spent, on: .budget)`), `apps/macos/Packages/VeritasMacUI/Sources/VeritasMacUI/Budget/EnvelopeGroupView.swift:23` (`figure(-group.spent)`), `design/mockups/budget.html` (Dining out shows spent 214.75) and `design/interaction.md:93` (VoiceOver reads "spent 214 dollars 75"). The assistant prints exactly what the Veritas screen prints and flips nothing itself. An envelope where refunds exceed purchases prints a negative activity, as the screen does. Available is the screen's own figure, not derived by the assistant or the model. These are budget figures, not Net Profit figures, and may differ from AI-2 (CLS-9). The word stays "activity" (see AI-7.3).
- **AI-7.3** The words "spent" and "spending" are never used for budget figures. Reassigning budgeted money is a budget change, not a transaction.
- **AI-7.4** In a budget whose currency does not have 2 places, the answer adds the CLS-12.10 fixed line after the two answer lines: "Spending is recorded at 2 places and budgets at {places}, so these numbers mix both, as in Actual." For yen it reads "budgets at 0". The word "over" never appears.
- **AI-7.5** **Routing.** A question about how much was budgeted goes to `budget`, even when it names a category. The model's instructions carry this sentence exactly: "For how much was budgeted, use budget, even when the question names a category." `categorySpending` is for what was spent, never for what was budgeted.

## AI-8 Worked examples (tests)

Today is 2026-11-22. Scenario A (examples 1, 5, 8, 10, 12): one on-budget account. November rows: category Groceries -20000 and -21200, category Gas -10000 (both in group Everyday), category Dining out -8000 and a refund +3000 (group Fun), Car Loan Principal -30000 (a debt principal transfer, row 6), a savings transfer -20000 (row 7), Salary +500000, a card payment -50000 between own accounts. Other examples state their own rows.

1. **Spending, November so far.** Expenses = 41200 + (8000 - 3000) + 10000 = 56200. Debt paydown 30000, savings 20000 and the card payment contribute nothing. Answer:
   - `Spending, November 1 to 22, 2026: $562.00`
   - `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
   - `Spending by category group, November 1 to 22, 2026:`
   - `Everyday: $512.00` and `Fun: $50.00`

   Net Profit for the same range: 500000 - 56200 = 443800.
2. **Refund crosses the boundary.** A -8000 Dining out purchase on October 30 and its +8000 refund on November 2. October Dining out $80.00, November `Spending on Dining out, November 2026: -$80.00`. Never shown as income.
3. **Category with no rows.** `Spending on Gifts, November 2026: $0.00`, then `From 0 transactions, on the Expenses line of the Net Profit report.` An income category with no rows: `Income from Side work, November 2026: $0.00`, then `From 0 transactions, on the Income line of the Net Profit report.`
4. **Labels.** Groceries hidden: `Spending on Groceries (hidden), November 2026: $412.00`. A hidden, archived category Old Gym with one -1200 row: `Spending on Old Gym (hidden, archived), November 2026: $12.00`, then `From 1 transaction, on the Expenses line of the Net Profit report.`
5. **Group.** Everyday = Groceries 41200 + Gas 10000 = 51200: `Spending on Everyday, November 2026: $512.00`.
6. **Income.** Salary +500000 and a -5000 reversal in Salary: income_total = 495000. A user who said "so far this year" gets `Income, January 1 to November 22, 2026: $4,950.00`, never the whole-year label.
7. **No match.** "groc" has no exact key. Keys: `groc` against `grocery` (starts with it, tier 1), `gas` (edit distance 3) and `gift` (edit distance 3). Answer: `No category or group is called "groc". The closest name is Groceries.` and no figure.
8. **Budget.** Budgeted 45000, Actual's `spent` -41200: `Budgeted for Groceries, November 2026: $450.00` then `Budget screen figures for November 2026: activity $412.00, available $38.00.` (no carryover). The word "spent" does not appear. Refunds win: budgeted 10000, `spent` +3000: activity `-$30.00`, available `$130.00`.
9. **Ambiguous name.** A group Food and a category Food in group Everyday: "food" answers `More than one category or group is called "Food": the category Food in Everyday, the category group Food. Ask which one.` and no figure. "food group" narrows to the group and answers. "Gift" and "Gifts" both exist as categories: the key is `gift` for both, so asking for either is ambiguous.
10. **Card payment.** "How much did I spend on my card bill?" No tool knows a card bill. The model calls `spending` for the range and answers with its first line, `Spending, November 1 to 22, 2026: $562.00`, the AI-2.2 second line, and the sentence "Transfers, card payments, net cash flow, debt paydown and savings are never spending." The -50000 payment is not in any figure (CLS-2).
11. **Group with mixed kinds.** Separate scenario: group Savings and Debt holds a -30000 principal transfer (row 6) in Car Loan Principal and a -1000 bank fee (row 13) in Fees. Answer:
    - `Spending on Savings and Debt, November 2026: $10.00`
    - `From 1 transaction, on the Expenses line of the Net Profit report.`
    - `Debt paydown for Savings and Debt, November 2026: $300.00. It is not spending.`

    Never $310.00 as spending. A -2000 purchase booked to the debt-kind category Car Loan Principal with no transfer is an `expense` row (CLS-4 row 14): `Spending on Car Loan Principal, November 2026: $20.00` and no Debt paydown line.
12. **Reconciliation.** Scenario A plus an uncategorized +4000 deposit (CLS-4 row 15, contributes -4000) and a -1200 row in a deleted category Old Gym whose group is gone. Expenses = 56200 - 4000 + 1200 = 53400. Lines: `Everyday: $512.00`, `Fun: $50.00`, `Uncategorized (needs review): -$40.00`, `(deleted) Old Gym: $12.00`. 51200 + 5000 - 4000 + 1200 = 53400.
13. **Debt paydown, asked by name.** `Debt paydown, November 1 to 22, 2026: $300.00. It is not spending.` A withdrawal-driven negative Savings line of -$50.00 adds `Negative means more was withdrawn than saved.`
14. **0-place currency.** Yen, made-up. A 500 yen purchase is stored -50000 (R), `expense_total` 50000: `Spending, November 2026: ¥500`. An envelope budgeted 600 (B): `Budgeted for Rice cooker, November 2026: ¥600`, then `Budget screen figures for November 2026: activity <a>, available <v>.` with a and v printed exactly as the Budget screen prints them (CLS-12.10), then the CLS-12.10 fixed line. The tests assert the line, the budgeted text and that "over" is absent. The activity and available digits come from the Budget screen's own formatter, not from this file.
15. **Periods.** The AI-6.2 table and the AI-6.10 table are the test tables, one row per case. Expected labels are as written there.

## AI-9 Net Profit summary

`netProfit(period)` answers when the user asks for Net Profit, net cash flow, or the whole report. Asking for it is asking for debt paydown and savings by name (AI-5.1). Each line carries the range, in this order:

- `Net Profit, <range>: <amount>`
- `Income, <range>: <amount>`
- `Expenses, <range>: <amount>`
- `Debt paydown, <range>: <amount>. It is not spending.`
- `Savings, <range>: <amount>. It is not spending.`
- `Net cash flow, <range>: <amount>`
- `Unreviewed transfers, <range>: <amount>. Not in Net cash flow.` Shown only when not zero (CLS-6 `unreviewed_total`).

The negative sentences of AI-5.3 follow a negative Debt paydown or Savings line. The figures are the ST-1 lines for the range. Example, Scenario A: Net Profit `$4,438.00`, Income `$5,000.00`, Expenses `$562.00`, Debt paydown `$300.00`, Savings `$200.00`, Net cash flow `$3,938.00` (443800 - 30000 - 20000 = 393800). No Unreviewed transfers line.

## AI-10 Budget habits (`budgetHabits`)

`budgetHabits()` answers "how can I improve my money habits", "where am I overspending", "which categories ran over budget lately" and questions like them. It looks at the user's own Budget screen figures, names the categories that ran over their monthly budget in most recent months, and gives one concrete figure for the largest. It takes no argument, because the window is fixed by AI-10.1. It writes nothing (AI-1.4). Every figure is an integer in minor units, computed by the core read `budgetHabits`: the month figures, the averages, the overage, the listing and the ranking (AI-10.3 to AI-10.6). The tool only formats them. The model never averages, subtracts, ranks or rounds (AI-1.1).

- **AI-10.1** **Window.** The window is the last 3 complete calendar months before today's month, never today's month and never part of one. With today = 2026-11-22 the window is August, September and October 2026, and its label is `August to October 2026`. Today = 2026-11-01 gives the same window. Today = 2026-01-15 gives October, November and December 2025, labelled `October to December 2025`. A window month counts only if it is not before the budget's first month: `BudgetMonth.firstMonth`, the first month Actual created, as returned by `budgetMonth(month:today:)` (contracts/ffi.md). That read is clamped to the months Actual has created, so asking it for an earlier month returns the first month's sheet. The tool compares each window month with `firstMonth` itself and never uses a clamped sheet for an earlier month. Let N be the number of window months that count, 0 to 3.
  - **Label.** The label is written directly: when the window's months share a year, `<first month name> to <last month name> <year>`, for example `August to October 2026` or `September to October 2026`. When they do not, `<first month name> <first year> to <last month name> <last year>`, for example `November 2025 to January 2026` (N = 3, today in February 2026) or `December 2025 to January 2026` (N = 2, today in February 2026). One month reads `<month name> <year>`.
  - **N = 0:** the answer is the one line `Budget habits: no complete month of history yet.` and nothing else.
  - **N = 1:** the one line `Budget habits: only 1 complete month of history (October 2026), so there is no pattern to report yet.`
  - **N = 2:** the window is those 2 months, labelled for example `September to October 2026`, and the rules below run with B at most 2.
  - **N = 3:** the full window.
- **AI-10.2** **Which categories.** A category is considered when all of these hold:
  1. It is an expense envelope on the Budget screen: not in an income group (CLS-3.1).
  2. It is not hidden, and neither is its group, as of today. Hiding keeps a category's history in every report and in `categorySpending` (CLS-3.2). It only takes the category out of this list, because a hidden category is one the user has put away. A hidden category is never counted or named here. Habits is a forward-looking list of the envelopes the user still manages, and its disclosure line (AI-10.9) says hidden categories are left out. AI-12 includes hidden categories because spending is history and must tie to the total.
  3. Its effective kind (CLS-3) is `expense`. A category of kind `debt_principal` or `savings` is never listed, because paying down debt and saving are not spending (AI-5). This is the second and last place a category's own kind is read (AI-3.4). The tool takes the kind from the core and never guesses it from a name or a group.
  4. It has no goal, open or closed (CLS-3.3, DEBT-8). A goal's budgeted amount is saved up for a later purchase, so a month with a purchase would look like a month over budget when it is the goal doing its job. Goal categories keep their lines in every other tool.
  5. The budget's currency has 2 places. When its decimal places are not 2 (0 places, 3 places or any other count), budgeted is on the budget scale and activity is on the register scale, which is always 2 places (D74, CLS-12.10, D77), so the two cannot be compared and the tool compares nothing. The decision is the currency's decimal places, not the presence of a note. The answer is `Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales.` followed by the core's CLS-12.10 note, copied as returned, only when the core returns one. `contracts/ffi.md` (`budgetMonth`) sets `mixedScaleNote` only in a 0-place budget and leaves it nil otherwise, so today a 3-place budget's answer is the first line alone. When the core sets the note for every currency without 2 places, as CLS-12.10 now requires, the note follows the first line in a 3-place budget too. No figure follows in either case.
  - If the existing core reads do not expose a category's effective kind or goal link, that is a contract addition for the implementer to raise. The rule above does not change.
- **AI-10.3** **The two figures per month.** For each considered category and each window month that counts:
  - `budgeted` is the Budget screen's `budgeted` for that month: the amount assigned in that month alone. Carryover, rollover and the envelope's `available` are never added to it. A category with savings carried in from earlier months is still measured against the month's own assignment (open question 1 in AI-10.9).
  - `activity` is the Budget screen's Spent figure as it prints (D78, AI-7.2): `-spent`, money out positive, a month where refunds win negative. The core keeps Actual's sign, and the tool flips it once, the same way the Mac screen does. Activity is a Budget screen figure, not a Net Profit figure, and may differ from `categorySpending` (CLS-9).
  - A month is a budgeted month for the category when `budgeted > 0`. A category with `budgeted = 0` that month has activity the user never budgeted for. That month is not counted for the category, and its activity is ignored by this tool. A category created partway through the window is counted only from the month it was first budgeted. Let B be the category's number of budgeted months.
- **AI-10.4** **Over budget most months.** A budgeted month is over when `activity > budgeted` (equal is not over). A category is listed when all of these hold:
  - B is at least 2.
  - The over months, O, satisfy `2 * O > B` (strictly more than half: 2 of 3, 3 of 3, 2 of 2).
  - Its `overage` on the rounded averages (AI-10.5) is greater than 0. A category that was over in 2 months by a little and far under in the third, or whose rounded averages are equal, is never called over budget, and a `$0.00` overage never prints.
- **AI-10.5** **Averages and rounding.** For a listed category:
  - `averageActivity = sum of activity over its B budgeted months / B`, and `averageBudgeted = sum of budgeted over the same B months / B`.
  - Each average is rounded once, to the currency's minor unit, half away from zero, in integers: `(2 * |sum| + B) / (2 * B)` with integer division, then the sign of the sum is put back.
  - `overage = averageActivity - averageBudgeted`, using the two rounded averages, so the three printed figures always agree to the cent. No total of categories is ever printed, so no rounding remainder has to be placed anywhere.
- **AI-10.6** **Ranking and limits.** Listed categories are ordered by `overage`, largest first. Ties go to the larger `averageActivity`, then to the name with case and accents folded (AI-3.6 key), then by category id. At most 3 are shown. When more than 3 qualify, the line `Showing the 3 largest of <k>.` follows the list, and the first line's count is `<k>`. Two shown categories with the same name in different groups read `<name> in <group>` (AI-3.6).
- **AI-10.7** **The suggestion.** The one computed suggestion is for the first (largest) listed category: `Bringing <name> back to its monthly budget would free about <overage> a month (average of <B> budgeted months in <range>).` `<B>` is that category's own count of budgeted months (AI-10.3) and `<range>` is the AI-10.1 label, so the basis is always stated. `<overage>` is the AI-10.5 figure, printed to the minor unit with no further rounding, never a whole-dollar figure. "About" is there because the figure is an average over the window, not a promise about next month. The tool offers no suggestion for the other categories, no total for them and no projection. This line is the only suggestion in an answer (AI-11.1).
- **AI-10.8** **Nothing to report.** When no category is listed by AI-10.4, the answer never invents a problem. It states that nothing ran over in most months and shows the largest budgeted categories against their budgets, so the user still sees their own numbers:
  - Considered categories with B at least 2, ordered by `averageActivity` largest first, then by the name with case and accents folded (the AI-3.6 key), then by category id, at most 3.
  - No suggestion line.
  - When there are no such categories at all: `No category has a monthly budget in <range>, so there is nothing to compare.` followed by the disclosure line of AI-10.9.
- **AI-10.9** **Output lines.** The model copies the first line exactly and the lines after it as returned (AI-1.2). The word "spent" and the word "spending" never appear (AI-7.3), except inside the CLS-12.10 fixed line, which is copied as it is (AI-7.4). `<range>` is the AI-10.1 label. `<k>` categories read `1 category` for one.
  1. `Budget habits, <range>: <k> categories ran over their monthly budget in most months.` For k = 1 it reads `Budget habits, <range>: 1 category ran over its monthly budget in most months.`
  2. One line per shown category: `<name>: over in <O> of <B> months. Average activity <averageActivity> a month against <averageBudgeted> budgeted, <overage> a month over.`
  3. `Showing the 3 largest of <k>.` (only when k is more than 3)
  4. `Bringing <name> back to its monthly budget would free about <overage> a month (average of <B> budgeted months in <range>).`
  5. `Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted.`
  6. `Hidden categories, goal categories and months with no budget aren't included.` The disclosure line is fixed and factual, and it ends every answer that lists or compares categories: this form, the nothing-over form below, and the `No category has a monthly budget` answer. The short refusals of AI-10.1 and AI-10.2 item 5 carry no figure to qualify and do not have it.

  When nothing is listed (AI-10.8), lines 1 to 6 are replaced by:
  1. `Budget habits, <range>: no category ran over its monthly budget in most months.`
  2. `Largest categories by average activity:`
  3. `<name>: average activity <averageActivity> a month against <averageBudgeted> budgeted.` (up to 3 lines)
  4. The line 5 sentence above.
  5. The line 6 disclosure sentence above.

  Amounts are on the budget scale, formatted as the Budget screen formats them (AI-1.5).
- **AI-10.10** **Worked examples (tests).** Today is 2026-11-22 and the budget began in 2026-01, so N = 3 and the window is August, September and October 2026. A 2-place currency, written in cents in the data and in dollars in the answers. Activity is `-spent`.

  **Scenario H.** Considered categories, each month in the order August, September, October:

  | Category | Budgeted | Activity |
  |---|---|---|
  | Dining | 30000, 30000, 30000 | 45000, 38250, 40500 |
  | Shopping | 15000, 15000, 15000 | 26010, 14020, 29000 |
  | Rideshare | 6000, 6000, 6000 | 12000, 6600, 10200 |
  | Groceries | 45000, 45000, 45000 | 44000, 47000, 41000 |
  | Gas | 10000, 10000, 10000 | 12000, 9000, 9000 |

  1. **Ryan's preview.** Dining: over (45000 > 30000, 38250 > 30000, 40500 > 30000), O = 3 of B = 3, `2 * 3 = 6 > 3`. Sum of activity 123750, average `(2 * 123750 + 3) / 6 = 247503 / 6 = 41250` (integer division, remainder 3 dropped), budgeted average 30000, overage 41250 - 30000 = 11250. Shopping: over in August (26010 > 15000) and October (29000 > 15000), not in September (14020 < 15000). O = 2, `2 * 2 = 4 > 3`. Sum 69030, average `(2 * 69030 + 3) / 6 = 138063 / 6 = 23010`, budgeted 15000, overage 8010. Rideshare: over all three months (12000, 6600, 10200 against 6000). Sum 28800, average `(57600 + 3) / 6 = 9600`, overage 9600 - 6000 = 3600. Groceries: over only in September (47000 > 45000), O = 1, `2 * 1 = 2` is not more than 3, so it is not listed (and its average 44000 is under 45000). Gas: over only in August, not listed. Order by overage: Dining 11250, Shopping 8010, Rideshare 3600. Answer:
     - `Budget habits, August to October 2026: 3 categories ran over their monthly budget in most months.`
     - `Dining: over in 3 of 3 months. Average activity $412.50 a month against $300.00 budgeted, $112.50 a month over.`
     - `Shopping: over in 2 of 3 months. Average activity $230.10 a month against $150.00 budgeted, $80.10 a month over.`
     - `Rideshare: over in 3 of 3 months. Average activity $96.00 a month against $60.00 budgeted, $36.00 a month over.`
     - `Bringing Dining back to its monthly budget would free about $112.50 a month (average of 3 budgeted months in August to October 2026).`
     - `Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted.`
     - `Hidden categories, goal categories and months with no budget aren't included.`
  2. **Rounding and the limit.** Add Fun, budgeted 20000, 20000, 20000, activity 21000, 20001, 20000. Over in August and September, not October (20000 is not more than 20000). O = 2, `4 > 3`. Sum 61001, average `(2 * 61001 + 3) / 6 = 122005 / 6 = 20334` (61001 / 3 = 20333.67 rounds to 20334, half away from zero), budgeted 20000, overage 334. It ranks fourth, so the first line reads `4 categories ran over...`, the three lines above follow unchanged, then `Showing the 3 largest of 4.`, then the Dining suggestion with its basis, the closing sentence and the disclosure line.
  3. **Nothing over.** Only Groceries and Gas are considered. Groceries: O = 1 of 3, average activity `(2 * 132000 + 3) / 6 = 44000`, budgeted 45000. Gas: O = 1 of 3, activity sum 30000, average `(60000 + 3) / 6 = 10000`, budgeted 10000. Neither is listed. Answer:
     - `Budget habits, August to October 2026: no category ran over its monthly budget in most months.`
     - `Largest categories by average activity:`
     - `Groceries: average activity $440.00 a month against $450.00 budgeted.`
     - `Gas: average activity $100.00 a month against $100.00 budgeted.`
     - `Activity and budgeted are the Budget screen's figures for each month. Carryover is not counted.`
     - `Hidden categories, goal categories and months with no budget aren't included.`

     No suggestion line.
  4. **Excluded rows, same window, none of them listed.** Hobbies is hidden, budgeted 10000, activity 30000 each month. Vacation has an open goal, budgeted 20000, activity 60000 in October. Car Loan Principal is a `debt_principal` category with activity 30000 each month and budgeted 30000. Salary is in an income group. Coffee has budgeted 0 and activity 5000 each month, so B = 0 and it is unbudgeted, not over. None appears in any line, and none changes the figures of examples 1 to 3.
  5. **A category budgeted partway.** Transit is first budgeted in September: budgeted 0, 8000, 8000, activity 3000, 9500, 9000. B = 2 (August is not a budgeted month, and its 3000 is ignored). Over in both (9500 > 8000, 9000 > 8000), O = 2, `4 > 2`. Sum 18500, average `(2 * 18500 + 2) / 4 = 37002 / 4 = 9250`, budgeted average 8000, overage 1250. `Transit: over in 2 of 2 months. Average activity $92.50 a month against $80.00 budgeted, $12.50 a month over.` Not `over in 2 of 3`. If Transit were the top category, its suggestion basis would read `(average of 2 budgeted months in August to October 2026)`.
  6. **Over in most months but under on average.** A category budgeted 10000 each month with activity 10100, 10100, 1000. O = 2 of 3, but the average activity is `(2 * 21200 + 3) / 6 = 7067` against 10000, so it is not listed.
  7. **Carryover does not hide it.** Dining has 50000 carried in on 2026-08-01, so its `available` is positive all three months and the Budget screen shows no overspending. Its budgeted amounts and activity are as in example 1, so it is listed exactly as there, and the closing line says carryover is not counted.
  8. **Fewer than 3 months.** The budget began 2026-10. N = 1, and the answer is `Budget habits: only 1 complete month of history (October 2026), so there is no pattern to report yet.` The budget began 2026-09. N = 2, the label is `September to October 2026`, and a category is listed only if it is over in both months, for example Dining over in September only (45000 > 30000) and under in October (28000 < 30000) is not listed. The budget began in November 2026 (today's month). N = 0, and the answer is `Budget habits: no complete month of history yet.` Today = 2026-11-01 gives the same window as 2026-11-22, and today = 2026-01-15 gives October to December 2025.
  9. **Negative activity.** A month where refunds exceed purchases has negative activity, for example -3000. It is never over. It lowers that category's average like any other figure.
  10. **Currency without 2 places.** A yen budget (0 places) with the core's CLS-12.10 note: `Budget habits can't be compared in this budget, because budgets and activity are recorded at different scales.` then the note as the core returns it, here `Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual.`, and no figure. A made-up 3-place dinar budget gets the same first line, then the core's note when it returns one (today none, `contracts/ffi.md`; CLS-12.10 requires one with `budgets at 3`), and no figure.
  11. **Two names.** Misc in Everyday and Misc in Fun both qualify and both are shown: `Misc in Everyday: ...` and `Misc in Fun: ...`.
  12. **No zero overage.** A category budgeted 10000 each month with activity 10001, 10001 and 9999. O = 2 of 3, `4 > 3`. Sum 30001, average `(2 * 30001 + 3) / 6 = 60005 / 6 = 10000` (30001 / 3 = 10000.33 rounds to 10000), budgeted average 10000, overage 0. It is not listed, so neither `$0.00 a month over` nor `free about $0.00` can print.
  13. **Window across a year.** Today = 2026-02-10 and the budget began in 2025-01. N = 3, the window is November 2025, December 2025 and January 2026, labelled `November 2025 to January 2026`. Dining, budgeted 30000 each month, activity 31000, 32000, 30000: over in 2 of 3 (30000 is not more than 30000), sum 93000, average `(186000 + 3) / 6 = 31000`, overage 1000. Lines: `Budget habits, November 2025 to January 2026: 1 category ran over its monthly budget in most months.`, `Dining: over in 2 of 3 months. Average activity $310.00 a month against $300.00 budgeted, $10.00 a month over.`, `Bringing Dining back to its monthly budget would free about $10.00 a month (average of 3 budgeted months in November 2025 to January 2026).`, the closing sentence and the disclosure line.
  14. **Two months across a year.** Today = 2026-02-10 and the budget began in 2025-12. N = 2, labelled `December 2025 to January 2026`. Dining, budgeted 30000 both months, activity 31000 and 32000: over in 2 of 2, sum 63000, average `(126000 + 2) / 4 = 31500`, overage 1500. `Dining: over in 2 of 2 months. Average activity $315.00 a month against $300.00 budgeted, $15.00 a month over.` and `... would free about $15.00 a month (average of 2 budgeted months in December 2025 to January 2026).`
  15. **Activity before the budget's first month.** The budget's `firstMonth` is 2026-09, and Dining has older transactions dated in August 2026 (activity 99000 in a read clamped to September's sheet). August does not count, so N = 2, the label is `September to October 2026`, and the clamped August read is never used. Dining, budgeted 30000 in September and October, activity 45000 and 40500: over in 2 of 2, sum 85500, average `(171000 + 2) / 4 = 42750`, overage 12750. `Dining: over in 2 of 2 months. Average activity $427.50 a month against $300.00 budgeted, $127.50 a month over.`
  16. **One category over.** Only Dining qualifies. With today = 2026-11-22 the first line reads `Budget habits, August to October 2026: 1 category ran over its monthly budget in most months.`, and the one category line follows, then the suggestion for it, the closing sentence and the disclosure line.
- **AI-10.11** **Decisions (Ryan, 2026-11-23).** Each was an open question, and Ryan chose the default in every case.
  1. **Measured against the month's own budgeted amount, not `available`.** It states a fact that is always true (activity ran above what was assigned that month) and does not repeat the overspending logic Actual already applies. Carryover does not change the comparison (AI-10.3).
  2. **The suggestion is exact to the minor unit** (`$112.50`), so the printed figures agree to the cent (AI-10.5, AI-10.7).
  3. **Hidden categories are left out of this list** (AI-10.2), and the answer says so in the disclosure line (AI-10.9). They keep their history in every report and in `categorySpending`.
  4. **Unbudgeted spending is ignored here** (AI-10.3), and the disclosure line says months with no budget are not included.

## AI-11 Wording and questions outside the data

- **AI-11.1** **Allowed.** Suggestions that follow from a figure the tool returned: the tool's own words, copied: "ran over" and the one suggestion line of AI-10.7, "Bringing <name> back to its monthly budget would free about <amount> a month (average of <B> budgeted months in <range>)". The model writes no suggestion of its own, even one that cites a figure. Every claim about the user's habits carries the figure it comes from, in the lines the model copies.
- **AI-11.2** **Banned.** The model never writes "you should", "you must", "you need to" or "I recommend". It never gives investment advice or debt payoff advice, and it never gives tax advice (tax features organize numbers and never advise, R-REP-8). It never offers a generic tip or rule of thumb (50/30/20, "build an emergency fund", "cut subscriptions", "pay yourself first"). It never states a figure, a trend, a cause or a motive that no tool returned. It never says a category "wastes" money or that a purchase was "unnecessary".
- **AI-11.3** **Instructions.** The model's instructions carry these sentences exactly:
  - "For questions about improving money habits, call budgetHabits and give only what it returns. Give only the suggestion the tool returns, never orders."
  - "Say a category ran over its budget only when a budgetHabits line says it did. When budgetHabits says no category ran over, say that."
  - "Never give general financial advice, generic tips, or investment, debt or tax advice."
- **AI-11.4** **Questions the tools cannot answer** (general advice, forecasts outside the tools, the market, the user's feelings about money). The redirect is returned by a read-only tool, `whatMoNeeAnswers()`, which takes no argument, reads nothing and writes nothing (AI-1.4). It returns exactly the text below. The model's instructions carry this sentence exactly: "Only when none of your other tools can answer the question, such as general advice, a forecast, the market or feelings about money, call whatMoNeeAnswers and reply with exactly what it returns and nothing else." The text is not in the model's instructions. Kept there, its example questions made the model open real answers with the redirect (found by the real-model evaluation). The answer has no figure and no apology that starts "I can't", and the model does not answer the question from general knowledge. The text, unchanged:
  - `Mo Nee answers from your own numbers: spending, income, budgets and which categories ran over their monthly budget. For example: "How much did I spend last month?", "Which categories ran over budget lately?" or "How much did I budget for Groceries this month?"`
- **AI-11.5** **Tests.** The tool's tests pin AI-10.9's strings and AI-10.10's examples. The instruction tests pin AI-11.3's sentences exactly, the AI-11.4 tool sentence, the AI-11.6 sentences including the precedence sentence, and the AI-1.2, AI-7.5 and AI-12.7 sentences. The AI-11.3 habits bullet ("For questions about improving money habits, call budgetHabits...") and the first sentence of AI-12.7 are pinned as two separate sentences, each exactly. The redirect is a constant that `whatMoNeeAnswers()` returns and the panel's tests compare to AI-11.4.
- **AI-11.6** **Other instruction sentences.** The model's instructions also carry these sentences exactly:
  - "Answer only from what your tools return. If the tools don't have what the question needs, say which figure is missing."
  - "Use each tool's own words for its figures. Call a figure Income, Expenses, Spending or Net Profit only when the tool names it that. Never call a balance, a saved report's total or a budget figure by those words."
  - "If a result includes a note about scales, repeat it with the figures."
  - "Tax buckets only total what the user tagged. Never say an amount is deductible, taxable or owed, and don't give tax advice."
  - Precedence: "For general advice, a forecast, the market or feelings about money, call whatMoNeeAnswers and do not say a figure is missing." The AI-11.4 tool sentence wins over "say which figure is missing" for these questions, which are not missing figures.

## AI-12 Spending by category (`spendingByCategory`)

`spendingByCategory(period)` answers "what did I spend last month by category", "break down my spending", "what did I spend the most on" and questions like them. It is the same Expenses figure as `spending` (AI-2), broken into its category lines. Its data source is the core read `spendingByCategory`, which returns the register-scale Net Profit Expenses lines. It writes nothing (AI-1.4), and the model never ranks, sums or rounds (AI-1.1).

- **AI-12.1** **Period.** It takes `period` exactly as `spending` does: the user's own words, resolved by the app against today (AI-6), with the same label rules and the same refusal (the period hint) for words it cannot read. "So far this month" is a partial range and "last month" a whole one.
- **AI-12.2** **Which categories.** Every line of the Expenses section of the Net Profit report for the range (CLS-6, ST-1) is a candidate, so the lines add up to `expense_total` and the first line is the `spending` figure for the same period, to the cent. Nothing is filtered by the category's own flags or kind:
  - Hidden and archived categories are included and labeled (AI-3.5, ST-1.3), because the `spending` total includes them. History is never dropped.
  - Goal categories are included. Spending on a goal is spending (CLS-3.3), and closing a goal hides its category, so a closed goal's category reads `(hidden, closed goal)`, and `(closed goal)` only if the user has since unhidden it (ST-10.3). D77 concerns a goal's suggestion, ETA and `available`, none of which this tool reads, so it does not apply.
  - A category of kind `debt_principal` or `savings` appears only for rows that are `expense` rows, such as a purchase booked to it with no transfer (CLS-4 row 14, AI-3.1). Its principal and savings transfers are never in the Expenses section and never appear here.
  - Income, internal transfers, card payments, off-budget accounts' own rows, debt paydown and savings are not in the Expenses section, so they are not here (CLS-2, AI-2.3).
  - Rows with no category are one line, `Uncategorized (needs review)`, which can be negative (AI-2.5, CLS-4 row 15). It is always shown when it is not 0, outside the limit of AI-12.4. A category whose group record is gone shows by its own label, for example `(deleted) Old Gym`.
  - A category line whose total is 0 (a purchase and an equal refund in the same range) is left out. It changes nothing in the total, and it is not counted in `Everything else`.
- **AI-12.3** **By group.** There is no group variant and no flag. A question "by group" is answered by `spending`, whose AI-2.2 part 3 already lists the Expenses line by category group, reconciled to the cent (AI-2.5). The routing sentence in AI-12.7 sends it there.
- **AI-12.4** **Order and limit.** The ranked lines are the categories, not the uncategorized line. They are ordered by amount, largest first, with a negative last. Ties go to the name with case and accents folded (the AI-3.6 key), then to the category id. When there are at most 6 categories, all are shown. When there are more than 6, the first 5 are shown, then one line `Everything else (<n> categories): <amount>` where n is the number of categories left out (always at least 2) and the amount is their exact sum. The uncategorized line, when its total is not 0, is then always printed on its own last line, `Uncategorized (needs review): <amount>`, outside the limit. It never counts toward the 6, and `Everything else` never absorbs it. Two shown lines with the same name key read `<name> in <group>` (AI-3.6). A category left out is never named.
- **AI-12.5** **Reconciliation and rounding.** Every figure is an integer in minor units, computed by the core read `spendingByCategory`. The category lines and the `Everything else` amount are summed in the core with checked integer arithmetic, so an overflow is an error and never a wrong figure. Nothing is divided, so there is no rounding rule and no remainder to place. The shown lines, `Everything else` and the uncategorized line equal `expense_total` to the cent, including negative lines. No percentages are printed. Signs follow CLS-6: money out is positive, a refund-heavy category is negative, the same direction as the Budget screen's activity (D78).
- **AI-12.6** **Output lines.** The model copies the first line exactly and the rest as returned (AI-1.2). Amounts are on the register scale, formatted with the currency symbol and places as `spending` formats them (AI-1.5).
  1. `Spending, <range>: <expense_total>`
  2. `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
  3. `Spending by category, <range>, largest first:`
  4. One line per shown category: `<label>: <amount>`
  5. `Everything else (<n> categories): <amount>` (only when more than 6 categories)
  6. `Uncategorized (needs review): <amount>` (only when not 0; always the last line)

  When no line is left after zero lines are omitted (no expense rows at all, or rows that net to zero), lines 1 and 2 are followed by `No spending lines in <range>.` and no heading. The wording holds in both cases. A currency without 2 places needs no refusal: spending and its category lines are all register-scale figures, as in AI-8 example 14, and no budget figure is read. So the CLS-12.10 note does not apply here, and the figures print as `spending` prints them.
- **AI-12.7** **Routing.** The model's instructions carry these three sentences exactly, in place of the two that choose between `spending` and `categorySpending`. Budget words win: a question with them goes to `budgetHabits` first, and only otherwise does a breakdown go to `spendingByCategory`.
  - "For questions about being over budget, overspending, improving money habits or what to change in my budget, use budgetHabits. For any other question about spending by category, a breakdown, or what was spent the most on, use spendingByCategory."
  - "For a total, or spending by group, use spending."
  - "Use categorySpending only when the question names a category or group. If it names more than one, call categorySpending once for each name and never add the figures together."

  Routing examples: "What should I change in my budget?" goes to `budgetHabits`. "What should I change about my investments?" is outside the data, so it gets the AI-11.4 redirect, copied exactly, and no figure. "What did I spend the most on?" goes to `spendingByCategory`. "Break down my spending by group" goes to `spending`.

  The instructions also carry AI-7.5's budget sentence. Each of these sentences must be present exactly. Their order within the routing paragraph is not part of the rule, and the precedence above is stated by what the sentences say, not by their position.

  A question that names two categories therefore gets two answers and no combined figure (AI-1.1). `categorySpending` with no name keeps asking for one (AI-3.6) and never guesses.
- **AI-12.8** **Worked examples (tests).** Today is 2026-11-22, a 2-place currency, amounts in cents in the data.
  1. **More than 6 lines.** November rows, one expense line each: Rent 150000, Groceries 41200 (hidden), Utilities 12000, Gas 10000, Dining out 5000 (8000 less a 3000 refund), Fun Money 4500, Gifts 2500. Total 150000 + 41200 + 12000 + 10000 + 5000 + 4500 + 2500 = 225200. Seven lines, so 5 shown: 150000 + 41200 + 12000 + 10000 + 5000 = 218200, and Everything else (2 categories) = 4500 + 2500 = 7000. 218200 + 7000 = 225200. "This month" gives:
     - `Spending, November 2026: $2,252.00`
     - `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
     - `Spending by category, November 2026, largest first:`
     - `Rent: $1,500.00`, `Groceries (hidden): $412.00`, `Utilities: $120.00`, `Gas: $100.00`, `Dining out: $50.00`
     - `Everything else (2 categories): $70.00`

     The first line equals `spending` for the same range, and the hidden Groceries is in both.
  2. **Exactly 6 categories.** Example 1 without Gifts: total 222700, all six shown (the sixth is `Fun Money: $45.00`), no Everything else line.
  3. **A negative line.** Lines A 100000, B 90000, C 80000, D 70000, E 60000, F 50000 and Dining out -8000 (October purchase, November refund, AI-8 example 2). Total 450000 - 8000 = 442000. Shown: A to E = 400000. Everything else (2 categories) = 50000 + (-8000) = 42000, printed `$420.00`. 400000 + 42000 = 442000. The negative line is last, so it falls in Everything else.
  4. **Uncategorized and deleted.** The AI-8 example 12 rows: Groceries 41200, Gas 10000, Dining out 5000, (deleted) Old Gym 1200, Uncategorized -4000. Four categories, all shown, then the uncategorized line last: `Groceries: $412.00`, `Gas: $100.00`, `Dining out: $50.00`, `(deleted) Old Gym: $12.00`, `Uncategorized (needs review): -$40.00`. Total 41200 + 10000 + 5000 + 1200 - 4000 = 53400, `Spending, November 2026: $534.00`.
  5. **Ties.** Coffee 5000 and Dining out 5000 tie at the fifth and sixth places of a seven-line list. The fold key `coffee` is before `dining out`, so Coffee is shown and Dining out falls into Everything else. Two categories both named Misc, in Everyday (3000) and Fun (2000), shown together, read `Misc in Everyday: $30.00` and `Misc in Fun: $20.00`.
  6. **Zero line.** A -8000 Dining out purchase and its +8000 refund in the same month net to 0. Among other categories the line is left out and is not counted in Everything else (AI-12.2). When it is the only expense line, the total is 0 and the answer is `Spending, November 2026: $0.00`, the AI-12.6 second line, and `No spending lines in November 2026.`
  7. **Goal and misfiled lines.** A -20000 card purchase in the open goal's category Vacation: `Vacation: $200.00`, included. In a closed goal, `Vacation (hidden, closed goal): $200.00`, because closing a goal hides its category (ST-10.3). If the user has since unhidden it, `Vacation (closed goal): $200.00`. A -2000 purchase booked to the debt-kind category Car Loan Principal with no transfer (CLS-4 row 14): `Car Loan Principal: $20.00`. The -30000 principal transfer is not in the Expenses section and does not appear.
  8. **No rows.** October with no expense rows: `Spending, October 2026: $0.00`, the AI-12.6 second line, `No spending lines in October 2026.`
  9. **Partial range.** "So far this month": `Spending, November 1 to 22, 2026: ...` and `Spending by category, November 1 to 22, 2026, largest first:`. The lines are those of rows dated in the range only.
  10. **0-place currency.** A made-up yen budget, Groceries stored -50000 and Transit stored -20000 (register scale, AI-8 example 14): `Spending, November 2026: ¥700`, then `Groceries: ¥500` and `Transit: ¥200`. 50000 + 20000 = 70000. No refusal and no CLS-12.10 note.
  11. **By group.** "What did I spend by group last month?" is answered by `spending`, whose AI-2.2 group lines reconcile to the total. `spendingByCategory` is not called.
  12. **Uncategorized outside the limit.** Example 1's seven categories plus uncategorized rows of 3000 money out. Total 225200 + 3000 = 228200, `Spending, November 2026: $2,282.00`. Shown: the same five (218200), `Everything else (2 categories): $70.00` (4500 + 2500 = 7000, not 10000), then `Uncategorized (needs review): $30.00`. 218200 + 7000 + 3000 = 228200. With a +4000 uncategorized deposit instead (-4000): total 221200, the same five and the same `$70.00`, then `Uncategorized (needs review): -$40.00`. 218200 + 7000 - 4000 = 221200. Uncategorized is never inside Everything else, and it is not counted among the 6.
- **AI-12.9** **Decisions (Ryan, 2026-11-23).** Uncategorized is always shown on its own line when it is not 0, outside the top-category limit, and Everything else never absorbs it. It is the one line the user has to act on (CLS-4 row 15, `needs_review`), so it is never folded away.
