# Motion

One motion system across the Mac app. Where a real choice existed, the options stay listed with the one Ryan chose, and the "Decisions" section at the end collects them.

Motion in Veritas has one job: keep the user oriented while money changes on screen. It shows where something came from, where it went, and that a commit landed. It never decorates, and it never shows an amount that isn't real, even for a frame.

## What ArenaSwap does well, and what changes here

ArenaSwap (`packages/ui/src/motion.ts`, `_motion.scss`) gets these things right:

- **One scale, named.** Seven durations from `instant` (100 ms) to `epic` (1000 ms), plus three loop periods. Screens pick a name, never a number.
- **One signature curve.** A quintic ease-out, `cubic-bezier(0.22, 1, 0.36, 1)`, used for nearly every entrance, with a separate ease-in for exits.
- **Exits are faster than entrances.** The guide drawer comes in over `base` (240 ms) and leaves over `quick` (180 ms) with the ease-in curve.
- **Reduced motion swaps motion for a fade.** The notice banner folds open normally and only fades under reduced motion, so state changes stay visible.
- **Content and container are staged.** The notice fold hides its content for the first half of the height change and shows it in the second half, so text never squashes.
- **Continuity through FLIP.** Multiview boxes measure before and after a layout change and animate the difference, so a reflowed grid reads as the same boxes moving.
- **Budgets on spectacle.** The card reveal staggers 80 ms per card, caps the stagger at index 6, animates at most 8 cards, and plays a full reveal only on the first open of the day, a quick one after that, and none under reduced motion.
- **Enforced by tests.** A parity test keeps the TypeScript and SCSS scales identical, and a Cypress test proves every loop stops under reduced motion.

What Veritas takes: the named scale, the faster exit, the staged fold, layout continuity, the "full the first time, quick after" budget, reduced motion as a fade rather than a cut, and test enforcement.

What Veritas leaves: ArenaSwap is a sports app and its curve launches hard. Veritas keeps its own gentler `standard` curve, drops the long `deliberate` and `epic` durations outside Wrapped, has no ambient loops, and adds two things ArenaSwap didn't need: springs that can be interrupted (keyboard users repeat commands fast) and a rule that money never animates through values that didn't exist.

## What's choppy today

From reading `apps/macos/Packages/VeritasMacUI` on `origin/dev`. The first five are the worst.

1. **Every page change is two hard cuts.** `WorkspaceView` gives the page `.transition(.asymmetric(insertion: .opacity + offset(y: 6), removal: .identity))`. The old page vanishes on the first frame and the new one fades in over an empty window. Most screens then load asynchronously, so the new page fades in as a spinner or blank and its content pops in with no transition. The `.animation(_, value: model.destination)` on the container also catches the new page's first layout pass.
2. **Dashboard card to detail is an unanimated swap.** `DashboardScreen` switches between `grid` and `CardDetail` with a plain `if`. The toolbar swaps at the same instant, and going back rebuilds the grid at the top of its scroll view, so the user loses their place.
3. **Dashboard drag snaps back, then jumps.** `WidgetCard.moveGesture` sets `drag = .zero` on release and then awaits `model.move`. The card flies back to its old slot, sits there until the core answers, then teleports to the new slot. Neighbours reflow without animation. Resize handles do the same.
4. **Budget month change runs competing animations.** `.motion(.fast, value: month)` sits on the whole content stack, so every layout difference between two months animates (rows that exist in one month slide, group heights interpolate), while each `MoneyLabel` runs its own drain-bar width animation. The bars appear to drain from one month into the next, which `interaction.md` rules out ("Cross-fade of the numbers. No sliding").
5. **Loading is inconsistent and shifts layout.** `ScreenState` waits 250 ms before showing a spinner, but Net Profit, Forecast, Tax, Statements and Custom reports show a bare `ProgressView().frame(minHeight: 200)` at once. The spinner appears with no fade, and content replaces it with no transition. In the register, `BalanceSummary` and `BalanceGraph` only appear after the account loads, pushing the table down about 150 pt. `DrilldownSheet` opens at 120 pt around a spinner and grows to 260 pt or more when rows arrive, so the sheet jumps.

Also found:

- Wrapped cards swap with no transition. `interaction.md` specifies a 220 ms cross-fade and a 6 s auto-advance; neither is built.
- New budget setup switches steps with a plain `switch`, and the sheet's height jumps between steps.
- Notice banners, the read-only banner and the reconcile bar insert at full height and push the page down in one frame.
- The tour's coach mark shows unanchored after 600 ms if its target is late, then jumps without animation when the target appears, because the placement change isn't keyed to `tour.current`.
- A new question in the Mo Nee panel pops in. (The streamed answer correctly grows without animation.)
- `ScreenState`'s delayed spinner flips opacity from 0 to 1 with no fade.

## Feel

This is the main choice. It decides how everything moves.

| | A. Crisp curves | **B. Settled (chosen)** | C. Spring-forward |
|---|---|---|---|
| Opacity and color | ease-out curve | ease-out curve | ease-out curve |
| Movement and size | ease-out curve | spring, no bounce | spring, bounce 0.15 to 0.2 |
| Interrupted mid-flight | restarts from the current value with zero velocity, a visible hitch | keeps its velocity and retargets smoothly | keeps velocity, overshoots |
| Character | Veritas today; precise, slightly mechanical | precise and calm; things arrive and stop | lively, iOS-like |
| Risk | hitches on repeated ⌘→, fast sidebar clicks | needs two kinds of token | a drain bar or card that overshoots shows a wrong amount for a moment |

Chosen: **B.** Keyboard users in a budgeting app repeat commands constantly (⌘→ three times, ↓ through envelopes), and springs absorb that where curves stutter. Bounce is ruled out because overshoot on a bar or a figure briefly displays money that isn't there.

## Tokens

Values for option B. All live in `VeritasDesign/Motion.swift` and mirror `design/tokens.css`, as `Motion.fast` and `Motion.sheet` do today.

### Durations

| Token | Enter | Exit | Use |
|---|---|---|---|
| `instant` | 90 ms | 90 ms | Hover tint, press, focus ring |
| `fast` | 150 ms | 90 ms | Selection, drain bar, value cross-fade, spinner fade (unchanged) |
| `sheet` | 220 ms | 150 ms | Banners, in-sheet step changes, Mo Nee bubbles, coach mark fade (unchanged) |
| `page` | 200 ms, starting 60 ms in | 100 ms | Screen to screen (see Navigation) |
| `story` | 480 ms | 480 ms | Wrapped spread turn |
| `stage` | 700 ms cap | none | Wrapped first-view entrance, total budget for all its parts |

Exits are explicit rather than a formula, but they follow ArenaSwap's rule of roughly 0.6 of the entrance.

### Curves

| Token | Value | Use |
|---|---|---|
| `standard` | `cubic-bezier(0.2, 0.7, 0.2, 1)` | Everything that appears or changes (unchanged) |
| `exit` | `cubic-bezier(0.4, 0, 1, 1)` | Things that leave (unchanged) |
| `linear` | linear | Determinate progress only (import, export) |

### Springs

SwiftUI's `.spring(duration:bounce:)`, where duration is perceptual.

| Token | Duration | Bounce | Use |
|---|---|---|---|
| `settle` | 0.32 s | 0 | Layout moves: reorder, drop, collapse, Dense toggle, sheet height, coach mark move |
| `morph` | 0.40 s | 0 | Card to detail and back |
| `turn` | 0.48 s | 0 | Wrapped spread movement (pairs with `story`) |

### Rapid repeat

The first press of a command animates. A repeat that arrives within 150 ms of the previous one (a held ⌘→, a held ↓) applies without animation, since the user is scrubbing and animating each step only adds lag. When the key is released, the final state is already on screen, so nothing is left to animate.

## Navigation

The transition depends on how the user got there, not on the destination. `WorkspaceModel.go(to:arrival:)` takes an arrival, `.peer` (the default) or `.drill`, so the page container can pick.

### Peer screens (sidebar, ⌘1 to ⌘6, ⌘K)

| | **A. Ready-gated fade-through (chosen)** | B. Ready-gated instant swap | C. Directional slide |
|---|---|---|---|
| What happens | The old page fades out over 100 ms (`exit`) and the new one fades in over 200 ms (`standard`), starting 60 ms in and rising 4 pt. The new page's content appears when its data has loaded, and a spinner only if that takes more than 250 ms (see The ready gate) | Same gate, then a one-frame swap | Pages slide up or down by sidebar order |
| Feel | Soft and quick, no blank frame | Finder and Mail; no motion at all | Spatial |
| Why not | | Reasonable if Ryan wants zero page motion | The sidebar isn't a sequence. Sliding implies a hierarchy that doesn't exist, and macOS apps don't slide between peers |

#### The ready gate

The gate lives in the new page, not the old one.

- **Contract.** A screen is ready when its model's loaded value is present: the non-nil sheet, report, forecast, summary or document, or `hasLoaded` where a model has it. Every screen drives two things from that one value: `ScreenState(kind: .loading)` while it's absent, and its content's fade-in (`fast`) when it arrives.
- **Timing.** The spinner waits 250 ms and then fades in, so a load that finishes inside the gate never shows one. Local loads from the core usually finish inside the page's own 260 ms fade, so the content arrives as part of the page.
- **Why the old page doesn't hold.** Holding it would need the new screen's data before its view exists. Screens load in their own `.task`, which runs only once the view is mounted. The two ways round that both cost more than they buy. One is starting each screen's load from `go(to:)`, which moves every screen's model into the workspace. The other is mounting the new page invisibly under the old one, which doubles toolbars, titles and focus for the overlap. If loads measured in the app run past about 100 ms, revisit this with `go(to:)` preloading.

This fixes most of choppy spot 1: the old page no longer vanishes in one frame, and the new page never fades in as a spinner and then pops its content in.

The 60 ms overlap is short enough that two different screens never visibly ghost over each other.

### Drilling in (a card, a row, a goal)

| | **A. Morph (chosen for cards)** | **B. Short push (chosen for rows)** | C. Fade-through |
|---|---|---|---|
| What happens | The card's frame grows into the detail page's content frame (`morph` spring). The header stays matched. The body cross-fades, because the detail draws the chart at a new size rather than scaling the old one. Back reverses it into the card's slot | The detail comes in from 24 pt to the trailing side while fading in (`settle`); the list moves 24 pt the other way while fading out. Back reverses | Same as peer screens |
| Fits | Dashboard cards, goal cards: things that look like containers | Accounts list row to register, Rules row to its editor page if it becomes one | Anything else |

Chosen: morph for dashboard cards and goal cards, short push for rows. A list row growing into a full page looks odd because a row has no visual weight to grow from.

Going back (⌘[ or the back button) restores the previous scroll position. The dashboard grid keeps its scroll state while the detail is shown instead of being rebuilt.

### Sheets, popovers, alerts

These are AppKit's. Veritas doesn't animate them itself, and nothing inside a sheet animates while the sheet is presenting. Inside a sheet:

- Step changes (new budget setup) cross-fade the content over `sheet` and animate the sheet's height with `settle`.
- A sheet that loads (Drilldown, Integrity report) opens at its final minimum size with the spinner centered inside, so its height never jumps.

### Inspector (Mo Nee)

The inspector column's slide is the system's. Inside it:

- A new question bubble enters from below: fade and 8 pt rise over `sheet`.
- An example question tapped from the empty state morphs into the question bubble (`settle`). This is the one matched-geometry moment in the panel.
- The streamed answer grows in place with no animation, and scrolling follows without animation, as it does now.
- The typing dots are the only loop in the app.
- The question field takes focus after the inspector finishes opening, not during.

### Banners and bars

Notice banners, the read-only banner, the find bar and the reconcile bar fold in, adapted from ArenaSwap's notice reveal. Height grows with `settle` while the content stays hidden; the content fades in over the second half. Leaving reverses it with `exit`, content first.

### Onboarding tour

- When a step stays on the same screen, the coach mark and its target ring move to the new anchor with `settle`, matched so they read as the same card.
- When a step changes screens, the coach mark fades out with the old page and fades in once the new page passes the ready gate. It is placed before it appears.
- If the target is late and the mark has already shown unanchored, it moves to the target with `settle` rather than jumping.

### Month change (⌘← / ⌘→)

Budget, Net Profit, Sankey and the register's month filter.

| | A. Figures cross-fade, title nudges (chosen, then adjusted) | **B. Figures cross-fade only (what ships)** |
|---|---|---|
| Figures | Cross-fade over `fast` | Same |
| Drain bars | Change without animation | Same |
| Month title | Nudges 6 pt in the direction of time while cross-fading | Cross-fades in place |
| Rows that exist in only one month | Fade in or out, no height animation | Same |

Ryan chose A, but the nudge can't be built: the month title is the window's `navigationTitle`, which AppKit draws, so SwiftUI can't move it. What ships is B. The cross-fade is applied to the figures only, not to the whole stack, which removes the competing animations of choppy spot 4.

## Continuity moments worth having

Ranked by how much orientation they buy. Each uses `matchedGeometryEffect` inside one screen's own container. The page container itself never morphs.

1. **Dashboard card into its detail page**, and back into its slot. The biggest win: today it's a hard cut and the grid forgets where you were.
2. **Goal card into the goal's detail**, if goal detail becomes a page. (Close goal stays a sheet.)
3. **Coach mark and ring between anchors** on the same screen.
4. **Mo Nee example question into the question bubble.**
5. **Wrapped contents line into its spread's headline** (see `wrapped-editorial.md`). Optional.

Things that should not morph: sheets (AppKit owns them), register rows (`NSTableView`), Sankey nodes and links (a flow that morphs looks like money moving between categories), and anything in the budget grid.

Morph needs a fallback: if the source card isn't on screen (opened from ⌘K), the detail uses the short push instead.

## Numbers and values

| | A. In place | **B. Cross-fade on commit (chosen)** | C. Digit roll |
|---|---|---|---|
| What happens | The new value replaces the old in one frame | A value changed by a commit somewhere else on screen cross-fades over `fast` | `.contentTransition(.numericText)` rolls each changed digit |
| Example | | Budgeting 40 more to Dining cross-fades To Budget, the group total and the drain bar together | |
| Why not | Easy to miss that To Budget changed after a commit | | For a few frames the figure shows a mix of old and new digits, an amount that never existed |

With B:

- The field being typed into, and its expression preview ("Return sets 452.00"), never animate.
- A value's money state (available, low, overspent) changes color in the same transaction as the value, over the same `fast`.
- Drain bars keep easing their width over `fast` on commit, as now.
- The same cross-fade applies whatever caused the change. Today a commit in this window and a change from elsewhere (sync, import, a schedule posting, another window) reach a screen the same way, through `observeChanges` and `refresh()`, and the session doesn't say which window caused it. Telling them apart would need each command path, undo and redo included, to open an animated transaction for the refresh it triggers, with every other refresh running with animations disabled. A 150 ms fade on a change from elsewhere does no harm, so the rule stays simple until there's a reason to add that signal.
- Amounts never count up, anywhere, including Wrapped.

## Lists and rows

For SwiftUI lists: envelopes, groups, bills, goals, rules, tags, what-ifs, dashboard cards.

| Change | Animation |
|---|---|
| Insert | Height opens with `settle`; the content fades in over the second half |
| Delete | Content fades out over 90 ms, then the height closes with `settle` |
| Reorder, drop | `settle` |
| Group collapse, Dense toggle | Height with `settle`, rows fade. The selected row stays put on screen: scroll is anchored to the selection, so the list doesn't drift under the user |
| Dashboard drag | The card follows the pointer one to one. On release it stays where it was dropped and settles into the snapped slot with `settle`. It holds that slot optimistically while the core answers, and moves back with `settle` only if the core rejects the move. Neighbours reflow with `settle` |

A change touching more than 20 rows, or a list longer than 500 rows, changes without animation.

The register (`NSTableView`) is separate. A single row the user adds or deletes uses `insertRows(at:withAnimation: .effectFade)` or `removeRows(at:withAnimation: .effectFade)`. Everything else (paging, sorting, filtering, search, reloads) goes through `reloadData` with no animation. The register is never touched from inside a SwiftUI animation transaction.

## Loading to content

1. **Ready gate.** The new page's content stays hidden until its data has loaded (see The ready gate).
2. **One loading view.** Every screen uses `ScreenState(kind: .loading)`. The bare `ProgressView().frame(minHeight: 200)` instances in Net Profit, Forecast, Tax, Statements and Custom reports go away.
3. **The spinner fades.** After its 250 ms delay it fades in over `fast` instead of appearing in one frame.
4. **Content cross-fades in.** The spinner fades out over 90 ms and the content fades in over `fast`. No rise: the page already rose.
5. **Reserve known space.** Anything whose size is known before its data arrives takes that size on the first frame: the register's balance summary and graph (one line plus 120 pt), a sheet's table, a dashboard card's body. Skeleton shapes are not used. Veritas screens vary too much for a fake shape to match, and a mismatched skeleton is one more jump.
6. **Reloads never show loading.** After a data change, the old content stays until the new content is ready, then figures cross-fade as in Numbers and values.

## What never animates

- An amount field's text while typing, and its expression preview.
- The figures in a row being edited, until Return or Tab commits.
- The register: scrolling, paging in, sorting, filtering, searching.
- Anything during live resize: the window, the sidebar divider, the inspector divider.
- The balance graph's hover readout and crosshair. They track the pointer one to one.
- A dragged card while the pointer is down.
- Charts on first appearance outside Wrapped. A chart that draws itself in on every visit gets old by the third visit.
- Repeated commands inside the rapid-repeat window.
- Nothing loops except the Mo Nee typing dots and system progress indicators. Nothing moves unless the user did something, except a Wrapped spread's first-view entrance, which follows a page turn the user made.

## Reduce Motion

The rule before this system was that every duration becomes 0. Apple's guidance for Reduce Motion is to remove movement, scaling and parallax and replace them with dissolves, not to remove every transition. Two options:

| | A. Everything instant (before) | **B. Movement becomes a short fade (chosen)** |
|---|---|---|
| Rule | All durations 0 | Anything that moves, scales or resizes becomes a cross-fade over `fast`. Small state changes (tints, a figure's cross-fade, a bar's width, a reorder) happen at once. Loops stop |

B keeps the cue that something changed without any motion. ArenaSwap's notice reveal does the same. It changes `TokenBehaviorTests.reduceMotionTurnsEveryDurationIntoNone`, which would become a test that every preset's reduced form has no spatial component.

Per item, under B:

| Item | Default | Reduce Motion |
|---|---|---|
| Hover, press, focus | `instant` tint | Instant |
| Selection | `fast` | Instant |
| Peer page | Ready gate, fade-through with 4 pt rise | Ready gate, cross-fade over `fast`, no rise |
| Card morph | `morph` | Cross-fade over `fast` |
| Short push | 24 pt slide and fade | Cross-fade over `fast` |
| Sheet step change | Cross-fade and `settle` height | Cross-fade, height changes in one frame |
| Banner and bar fold | Height and staged fade | Fade over `fast`, height changes in one frame |
| Coach mark between anchors | `settle` move | Fade out, fade in at the new anchor |
| Mo Nee question bubble | Fade and 8 pt rise | Fade over `fast` |
| Typing dots | Pulse | Held still (as now) |
| Month change | Figures cross-fade | Instant |
| Value change on commit | Cross-fade `fast` | Instant |
| Drain bar | Width over `fast` | Instant |
| List insert, delete | Height and fade | Fade over `fast`, height in one frame |
| Reorder, drop, collapse | `settle` | Instant |
| Dashboard drag release | `settle` into slot | Instant |
| Spinner appear | `fast` fade after 250 ms | Instant after 250 ms |
| Content after loading | `fast` fade | Instant |
| Register single add or delete | `.effectFade` | No animation |
| Wrapped spread turn | `story` and `turn` | Cross-fade over `fast` |
| Wrapped first-view entrance | Staged, up to `stage` | None; the spread appears complete |

Under option A every row of the right-hand column is "Instant".

## Performance

- Target 120 fps on ProMotion displays and never below 60 fps, measured on the slowest supported Mac with the 100k-row register fixture and a 300-envelope budget.
- No SwiftUI animation reaches the register `NSTableView`. Its row heights are fixed and its paging stays off the animation path.
- No implicit animation on a container with more than 200 children. Animate one layer (the container's opacity) rather than each row.
- Animate opacity and transforms. Don't animate frames of large hierarchies, shadow radius, blur or materials.
- Data reloads run in a transaction with animations disabled (`.transaction { $0.disablesAnimations = true }` at the reload point), so a sync can't trigger a cascade of implicit animations.
- At most one matched-geometry pair is in flight at a time.
- Swift Charts animate a data change only when the chart has 2,000 marks or fewer; above that the chart swaps.
- List animations obey the 20-row and 500-row limits in Lists and rows.
- Check hitches with the Instruments SwiftUI and Animation Hitches templates before each motion change lands, and add a UI test that drives sidebar navigation and month changes under the performance fixtures.

## Enforcement in code

Screens never write a duration, a curve or a spring. They name a preset.

### The `Motion` namespace

`VeritasDesign/Motion.swift` holds the tokens and the named motions. Each `Motion` has a curve (a timing curve with an easing, duration and delay, or a zero-bounce spring) and a reduced curve for Reduce Motion, which is `nil` when the change should happen at once.

| Motion | Curve | Reduce Motion |
|---|---|---|
| `instant` | 90 ms `standard` | at once |
| `fast` | 150 ms `standard` | at once |
| `leave` | 90 ms `exit` | at once |
| `sheet` | 220 ms `standard` | 150 ms fade |
| `pageIn` | 200 ms `standard` after 60 ms | 150 ms fade |
| `pageOut` | 100 ms `exit` | 90 ms |
| `settle` | spring 0.32 s | at once |
| `morph` | spring 0.40 s | 150 ms fade |
| `push` | spring 0.32 s | 150 ms fade |
| `turn` | spring 0.48 s | 150 ms fade |

`MotionTransition` pairs an insertion and a removal, each a motion plus an optional offset and fade: `page`, `push`, `pop`, `reveal`, `rise`, `fade`, `turnForward` and `turnBack`. With Reduce Motion on, every offset is dropped and both sides fade.

`Motion.progress(at:)` samples a curve, for tests and frame strips.

### View modifiers

| API | Does |
|---|---|
| `.motion(_:value:)` | Animates changes to `value` with a named motion, honoring Reduce Motion |
| `.motionTransition(_:)` | Inserts and removes the view with a `MotionTransition`. The transition carries its own animation, so no container animation is needed |
| `Motion.perform(_:_:)` | Replaces `withAnimation`, takes a named motion |
| `.valueChange(_:)` | A money figure's cross-fade. Never a digit roll |
| `.shown(_:)` | Fades only the view's opacity, so nothing else changing alongside it animates |
| `.still(whenChanging:)` | No animation in the subtree when a key changes, such as drain bars when the month changes |
| `.stillOnRapidRepeat(of:)` | The rapid-repeat rule for a value |
| `.withoutMotion()` | No animation in the subtree at all |
| `.morph(_:in:)` | `matchedGeometryEffect` for a card and its page. No match with Reduce Motion on, so the two cross-fade |

### Checks

- **No hand-rolled timing.** A test in `VeritasDesignTests` scans `VeritasMacUI/Sources`, `VeritasKit` and the app target, and fails on `withAnimation`, the `.animation(` modifier, `.spring(`, `.timingCurve(`, the ease curves, a linear animation, a system spring, `NSAnimationContext`, `.animator()`, `.transition(` (use `.motionTransition`) or a digit roll outside `VeritasDesign`. `TimelineView` schedules, such as `TimelineView(.animation)` in `TypingDots`, are exempt: they set a redraw rate, not a timing. `TypingDots` is the one allowed loop and has its own Reduce Motion test.
- **Parity.** `TokenParityTests` checks every duration, curve and spring against `tokens.css`, as it does for `duration-fast` and `duration-sheet` today.
- **Reduce Motion.** For every preset, the reduced form has no offset, scale or spring, and no duration above `fast`.
- **Loops.** A test lists every looping view (today only `TypingDots`) and proves it holds still under Reduce Motion, the way ArenaSwap's Cypress test does.
- **Debug time scale.** In debug builds, a launch argument `-MotionTimeScale 5` slows every motion five times, so timings can be judged by eye. macOS has no system-wide slow-animation switch like the iOS Simulator's.

### Docs that mirror this one

`tokens.md` lists the motion tokens and `interaction.md` lists the motion per interaction. Both follow the decisions below. The tokens land in `tokens.css` with the motion build.

## Decisions

Ryan's picks, November 2026:

1. **Feel:** settled springs (B).
2. **Peer page transition:** ready-gated fade-through (A). The gate lives in the new page: the old page fades out at once, and the new page's content appears when its data has loaded (see The ready gate).
3. **Drill-in:** morph for cards, short push for rows.
4. **Month change:** Ryan chose A, figures cross-fade and the month title nudges. The nudge is dropped because AppKit draws the `navigationTitle`, so only the figures cross-fade.
5. **Value changes:** cross-fade on commit (B).
6. **Reduce Motion:** movement becomes a short fade (B).
