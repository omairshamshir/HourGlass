# The visual language

Hourglass is set like an almanac: warm paper, ink, hairline rules, a serif for
words and a monospace for figures. Structure comes from rules and whitespace,
never from cards or shadows.

This document exists because the design is easy to erode by accident. The
default instinct for a dark-mode developer tool — cool graphite, rounded cards,
pill tabs, a saturated accent used for emphasis — is precisely what this app is
not, and every one of those choices was made deliberately against.

Everything here lives in `crates/hourglass/src/theme.rs`. Use the named helpers.
A raw `rgb(0x…)` in a view is a bug.

## The one rule

**Burnt sienna means the clock is running. It means nothing else.**

Not hover. Not focus. Not "this is important". Not the selected tab's text. A
user should be able to tell from across the room whether they are on the clock,
and that only works if the colour is never spent on anything else.

`theme.rs` carries a unit test asserting every project swatch, in both
palettes, stays a minimum distance from it, so a stopped project can never be
misread as a running one.

The two places it is allowed to appear: the hero clock while running, and the
`Start`/`Stop` button while running. The underline on the selected range tab and
the caret in a text field are the only borrowings, and both are hairlines rather
than fills.

## Palette

There are two, and they are the same design: paper under daylight, and the same
page under a lamp. Dark mode is not a separate visual language and must not
drift into one.

| Token | Light | Dark | Used for |
| --- | --- | --- | --- |
| `paper()` | `#FAF7F0` | `#16130F` | The page: report pane, open expanses |
| `margin()` | `#F1ECE1` | `#100E0A` | The project rail, like the deeper edge of a page |
| `raised()` | `#FFFFFF` | `#201C16` | Inputs and lifted rows |
| `rule()` | `#DCD4C4` | `#383127` | Rules that carry structure |
| `rule_soft()` | `#EBE5D9` | `#272219` | Rules between list rows, and empty chart track |
| `ink()` | `#1A1815` | `#F0EAE0` | Primary text — never pure black, never pure white |
| `muted()` | `#6E6659` | `#A2988A` | Labels, timestamps, units, the stopped clock |
| `faint()` | `#A9A091` | `#6B6355` | Text that should nearly disappear |
| `ember()` | `#C2410C` | `#E8763A` | Running. Only running. |
| `alert()` | `#9F1239` | `#E5647E` | Destructive actions |

Nothing in either column is a neutral grey. Every tone carries warmth, which is
what keeps the light palette from reading as "light mode dashboard" and the
dark one from collapsing into the graphite every other developer tool uses.

Two things genuinely differ rather than merely inverting:

- **Ember warms up.** Burnt sienna is a dark colour, and dark colours vanish on
  a dark ground. `#E8763A` is the same signal at a brightness that survives.
- **Hovers reverse.** `wash(alpha)` mixes from ink on paper and from white
  under the lamp, because a white wash on paper does nothing and an ink wash on
  near-black does nothing. This is why views must call `wash()` rather than
  writing their own overlay. `ember_wash(alpha)` is the only tinted surface in
  the app either way, used for the running project's row and the resume bar.

Views never name a hex value. They call the accessor for the *role*, and
`theme::set_appearance` swaps what the roles resolve to. A literal `rgb(0x…)`
in a view is a bug that will only show up in the appearance you did not test.

### Project swatches

Eight hues, chosen to stay clear of burnt sienna and assigned round-robin by
project count. The dark set is lifted well above the light one, because the
daylight hues read as mud on a dark ground — but held back from full
saturation, because bright pastels read as a children's chart rather than as
the same almanac.

```
light   #2F4B7C indigo   #1F6F6B teal    #6B3FA0 plum    #A83A5B rose
        #55702A olive    #2A6F97 sky     #5A6472 slate   #8A6516 ochre

dark    #8098C9 indigo   #5AB3A6 teal    #A88FD0 plum    #CE8595 rose
        #A2B56A olive    #6FAAC9 sky     #98A1AC slate   #C9B25F ochre
```

`swatch_soft(index, alpha)` thins one for a ring or a border.

### Choosing an appearance

Light, dark, or auto, offered as three small capitals divided by rules at the
right of the masthead — the way a printed page offers alternatives, rather than
as a toggle or a dropdown that would need chrome of its own. Auto is the
default and follows macOS as it changes.

## Type

Three faces, resolved once at startup against what is actually installed, with
fallbacks that all ship with macOS so a missing font can never blank the timer.

| Role | Face | Falls back to |
| --- | --- | --- |
| `display` — hero clock, prose, project names | New York | Iowan Old Style, Charter, Palatino, Georgia |
| `mono` — figures in columns | SF Mono | Menlo, Monaco |
| `ui` — controls, short labels | `.SystemUIFont` | — |

### Figures must not jitter

Every number goes through `numeral()` with a font from `Fonts::numeric` or
`Fonts::display_numeric`, both of which enable `tnum`. A clock whose seconds
column shifts once a second is the fastest way to make software feel cheap.

`display_numeric` additionally forces `lnum`. Georgia and Iowan default to
old-style figures, which sit at varying heights — without it the hero clock
hops above and below its own baseline every second.

gpui exposes arbitrary OpenType tags through
`FontFeatures(Arc<Vec<(String, u32)>>)`, which is how both are set.

### Scale

The hero clock is 76px, light weight. Everything else is 9.5–15px. That gap is
the point: one figure dominates the page and the rest is quiet.

A **stopped** clock is `muted()`, not `ink()`. Rendering it in full black made
the stopped state louder than the running state, which inverts the emphasis the
whole design is built on.

### Spaced capitals

Section markers, the wordmark, the masthead date, and the range tabs are set in
spaced capitals, the way a printed table heads its columns.

gpui 0.2.2 has **no letter-spacing**, so `theme::letterspaced()` interleaves thin
spaces (U+2009) between characters. Short words only — the spacing makes long
ones unreadable.

## Layout

```
┌──────────────────────┬──────────────────────────────────────┐
│ ●●●        HOURGLASS │ MONDAY 7 SEPTEMBER 2026              │ 52px strip
├──────────────────────┼──────────────────────────────────────┤
│ 1 ○ Atlas      3h 30m│  ● Atlas Redesign          [ Stop ]  │
│ 2 ○ Beacon        47m│                                      │
│ 3 ○ Cirrus     1h 05m│  0:00:00              TODAY  5h 56m  │
│                      │                                      │
│                      │  [ the day band ]                    │
│                      ├──────────────────────────────────────┤
│                      │  TODAY  THIS WEEK  THIS MONTH  5h 56m│
│                      │  ● Atlas   ─────────────────   3h 30m│
│                      │  ENTRIES                             │
│                      │  ● 09:12 – 10:44  Atlas        1h 32m│
├──────────────────────┼──────────────────────────────────────┤
│ + New project  Export│                                      │
└──────────────────────┴──────────────────────────────────────┘
```

**The 52px top strip is structural, not decoration.** macOS draws the traffic
lights into its left corner, so nothing may be placed there. The rail keeps its
wordmark at the far end of the strip and the page hangs the date on the other
side of the rule. This turns space the system forces you to leave empty into
something useful — and it is the fix for the wordmark previously sitting
underneath the close button.

If you change `TITLE_STRIP` in `ui/root.rs`, change `traffic_light_position` in
`main.rs` to match; the lights should stay vertically centred in the strip.

Page margins are 40px (`MARGIN` in `ui/root.rs`). Generous, because the whole
look rests on figures having room.

### The day band

The one picture the app is built around. A bar chart tells you *how much*; the
band tells you *when*, and where the gaps are. Honest hours are as much about
the empty stretches as the full ones.

It draws a dynamic window rather than midnight-to-midnight, since a fixed day
spends most of its width on hours nobody works. The window covers the working
day (8am–7pm) and stretches only as far as the day's real entries and the
current hour require.

Segments are square, not rounded — it should read as a chart printed on the page
rather than a widget dropped onto it. The empty track is `rule_soft()`, the
colour of a ruled line. A one-minute entry gets a minimum width so short
sessions still register.

## Motion

Almost none, and that is a decision rather than an omission.

The interactions here happen dozens of times a day. At that frequency an
animation is not delight, it is latency the user pays over and over. Keyboard
actions are never animated.

The two exceptions are both immediate: buttons respond on press rather than
release, via `.active(|style| style.opacity(0.7))`, so the control never feels a
frame behind the finger; and controls that would otherwise clutter a list —
archive, delete — appear on row hover through `group_hover`, so the rail reads
as a list of projects rather than a list of buttons.

### The clock's settle

One piece of ambient motion is allowed, and it is the only one: when the hero
clock ticks, the figures that changed arrive at 10% opacity and firm up to full
over 180 ms, eased in and out.

This is not decoration. A 76px figure replaced instantly once a second reads as
a flicker in peripheral vision; the same figure fading up reads as ink meeting
paper, which is the metaphor the rest of the page is already committed to. It
does not delay anything, because nothing is waiting on it. 180 ms is long
enough to register and still well inside the second, so the window is idle for
about four fifths of each tick.

Three constraints keep it honest, and a change that breaks any of them is
a regression even if it looks fine:

- **Only the figures that changed move.** A counting clock rewrites a suffix of
  its own text and nothing else, so `settled_prefix` compares the reading
  against the previous second and animates the tail alone. The steady part
  stays a single static run, which is what guarantees the left edge and the
  digit spacing cannot shift. Animating the whole reading would reintroduce
  exactly the jitter `tnum` was chosen to prevent.
- **It never runs while stopped.** A stopped clock has nothing to settle, and
  asks for no frames at all.
- **The animation key is the second, not the frame.** gpui requests a frame
  only while an animation is in flight, so a key that changed every frame would
  pin the window at the display's refresh rate for as long as it was open. Keyed
  on the second, the window paints for about a fifth of each second and is idle
  for the rest.
- **The easing must use the whole duration.** Duration is literally frame
  count here, so a sharp curve wastes it: `ease_out_quint` is 97% complete at
  half time, meaning half the frames it pays for render a change too small to
  see. Measured on this window a frame costs roughly half a percent of a core,
  so the curve is a budget decision as much as an aesthetic one.

Measured cost with the window open and the clock running, at the previous
120 ms settle: roughly 4% of one core without the settle and 7–8% with it.
180 ms is half again as many animation frames, so expect something closer to
9–10% of one core in that same state — still only while the window is open
and the clock is running. With the window closed or the clock stopped it is
nothing, which is most of the time for a menu-bar app. `SETTLE` is the single
dial if that trade ever stops being worth it.

Worth knowing before optimising the wrong thing: most of that 4% baseline is
not the window. It is `sync_tray` writing the menu-bar title four times a
second, which is AppKit work the animation has nothing to do with.

Do not extend this to a slide, a roll, or an odometer. Position changes on a
figure this large are legible from across the room, which is precisely why they
become irritating over an eight-hour day — and why the fade was chosen instead.

## Things not to do

- Do not introduce a card, a drop shadow, or a border radius above 3px.
- Do not add a second accent colour. If something needs emphasis, use weight,
  size, or a rule.
- Do not use the accent for hover, focus, or selection.
- Do not set a figure in a proportional face, or without `tnum`.
- Do not use pure white as a page background or pure black as text, in either
  palette.
- Do not place anything in the top-left 80×52 of the window.
- Do not add a spinner or a progress animation. Nothing here takes long enough.
- Do not animate anything on a key repeat or a per-frame value. The clock's
  settle is keyed to the second precisely so it ends; an animation keyed to
  something that changes every frame never stops asking for frames.
- Do not write a hex value into a view. Call the role accessor in `theme.rs`,
  or add one if the role is genuinely new.
- Do not make dark mode its own design. It is the same page under a lamp; if a
  change only makes sense in one palette, it is the wrong change.
- Do not turn the add-hours form into a modal dialog. It opens as a band with
  the day's figures still visible above it, because the numbers on screen are
  usually what the user is reading the missing hours off.
