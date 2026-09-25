<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Changelog

All notable changes to Teksilo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) —
though pre-1.0, so breaking changes can land in a minor bump. `release.toml`
keeps every workspace crate on one shared version; entries below are grouped
by crate for clarity, not because crates version independently.

## [Unreleased]

### Added

#### Accessibility

- **`tools/reader/`: what a screen reader gets from an example, recorded.**
  `tools/reader/reader.py` runs an example in a private, invisible desktop
  session (its own D-Bus, AT-SPI bus, KWin and runtime directory, no
  `DISPLAY`, no sound) and records the AT-SPI events it emits, the tree a
  reader walks, and what Orca 46.1 says, speaking to nothing. Acts are real
  key presses through the private compositor, AT-SPI actions, or the
  automation bridge. Scenarios state what a reader should get; `tabwalk` and
  `tree` need no scenario. Linux only. See
  `docs/a11y/reader-harness.md`.
- **`Widget::accessibility_proxy`: a composite can publish itself through the
  field that holds focus.** Where a composite keeps its focus, text and value
  on one inner widget, that widget is the node a screen reader lands on and has
  to carry the composite's name, while an application can only reach the
  composite's id. Returning the inner widget from the hook makes the tree apply
  everything attached to the composite (its overrides, a `FormLayout`'s
  `labelled_by`, `access_described_by`, the tooltip it owns) to that widget's
  node, after the widget's own, and point relations naming the composite at
  it. The overrides guide has a section on it.
- **`accessibility::audit::focusable_nodes_hidden`: a control that takes
  focus inside a hidden subtree.** A hidden node hides everything under it
  from every platform adapter, and only the focused node is let back through,
  alone, so a reader lands on the control and finds nothing around it. The
  audit lists every node that offers `Action::Focus` while hidden, itself or
  through an ancestor, and is not disabled. The widget previewer's catalog
  census now runs it over every widget and documentation snippet. Beside it,
  `audit::nodes_in_filtered_tree` lists the nodes `accesskit_consumer`'s
  filter keeps, which are the ones a platform adapter walks and can
  announce.

#### Internationalization

- **A formatted date can name its weekday, or stop at the month.**
  `TeksiloDateTimeFormatter::date_fields(DateFields)` picks which fields the
  date part names: `YearMonthDay`, the default and what every date style
  rendered before; `YearMonthDayWeekday`, CLDR's full date at `DateStyle::Long`
  ("lundi 31 août 2026", "Monday, August 31, 2026"); or `YearMonth`, a month
  with its year ("août 2026"). ICU picks the pattern for the whole combination,
  so the order and the grammar are the locale's: a Russian month is genitive
  after a day and nominative beside its year alone. `format_in_locale(value,
  &lang)` renders one value once, as a plain `String`, in a locale the caller
  names, for a string computed on demand, such as an accessibility name.
  `calendar_system(CalendarSystem::Gregorian)` keeps the date on the Gregorian
  calendar where CLDR gives the locale another one: by default `fa-IR` writes
  24 September 2026 as the 2nd of Mehr 1405, and `th-TH` counts its year 2569.

### Changed

#### Data views

- **A single-selection `TreeView` over an index `SelectionModel` keeps the
  selected row selected through a structural change.** An insert, a removal or
  a collapse above it used to leave the selection at its old position, on
  whichever row moved there. The selection now moves with its row, as a keyed
  selection does. Multiple selections are unchanged. **Behaviour change.**

### Fixed

#### Core

- **Every window was an unnamed "frame" to Orca.** The accessibility tree's
  root, which AT-SPI presents as the window, carried no name, so Orca 46.1
  announced each window as it came up with the bare word "frame". The root now
  takes the window's title, and follows it when the title changes. Windows and
  macOS read the title from the native window and are unchanged.
- **Orca heard only the first message the framework's announcer said.** Every
  `announce` in every Teksilo application, after the first of a session, was
  dropped by Orca 46.1, and even a first one was lost whenever Orca handled the
  event after its node had gone. Each message is now spoken from a node the
  tree has never had, which stays in the tree for five seconds
  (`teksilo_core::announcer::LINGER`) and then leaves for good. A burst keeps
  at most eight in the tree at once. The tree no longer carries two hidden
  announcer nodes while nothing is being said, so a `TreeUpdate` of an idle
  tree has two nodes fewer.
- **The announcement ring recorded text no platform announces.**
  `WidgetTree::announcements_since`, and the automation bridge's
  `pull_announcements` that reads it, recorded a live node's `value` before
  its label, from every live node the walk emitted, whenever that text changed.
  Every AccessKit adapter announces a node's *name* instead, which is its value
  only for a `Role::Label`, and only for a node in the filtered tree. So a
  hidden live region, one inside a hidden subtree, and a `Status` or `Alert`
  carrying its text as a value were all recorded while every platform stayed
  silent, and a probe reading the ring passed over the silence. A tree that
  records announcements now replays every update `sync_accessibility`
  returns, including the re-placed copy a scroll hands out, through
  `accesskit_consumer`, and the ring records what all three adapters
  announce: a live node's name as it enters the filtered tree, and again
  when it changes while the node stays there, at the
  politeness the node has or inherits from a live ancestor. A node inside a
  live region is recorded, as the adapters announce it. A change of politeness
  alone, which Windows and macOS announce and AT-SPI does not, is not, and
  neither is a blank name. Several in one update are kept in reading order,
  so a message the framework's announcer says comes after a change of the
  application's own live region in the same frame. The replay is a second
  pass over every node of every update, making a full sync of 3,000 nodes
  1.4 to 2 times as long, so only a tree that has a reader records: one
  built with `WidgetTree::new()`, as every headless tree and every test is,
  and the tree of a window `teksilo-app` opens only when
  `install_automation_bridge_in_debug()` installed the bridge, which it does
  in a debug build with the `automation` feature. The new
  `WidgetTree::set_records_announcements` switches it. **Behaviour change**
  for anything that read the ring, and for anything that read it from a
  window without the automation bridge, where it is now empty.
- **`announce_unless_widget_speaks` kept quiet beside a live region nobody
  could hear.** It took a widget to be speaking when any node in its subtree
  carried a politeness and a value or a label, so a hidden live region, or a
  `Status` holding its text as a value, silenced the framework's own message,
  and the user heard nothing at all. It now reads the last update through
  `accesskit_consumer`, whether or not the tree records announcements, and
  counts only a live node the platform adapters walk, with a name to speak,
  whose politeness is set inside the widget. A button that is live only
  because it sits in a toast or another live region is not the one speaking,
  and does not keep the framework quiet about its own menu. The long-press
  context-menu announcement goes through it.
- **`accessibility::announced_text` and the label audits took a value for a
  name.** For any role but `Role::Label`, `announced_text` fell back to the
  node's `value` when it had no label, and `audit::duplicate_label_leaks`
  and `labels_without_text_ranges` read names the same way, while no
  platform adapter takes a value as the name of anything but a label. A
  `Status` holding its text as a value was reported as announcing it, and a
  label repeating such a value was reported as repeating a name nobody
  hears. Both now read a `Role::Label`'s value and any other node's label,
  and nothing else. **Behaviour change** for a test that found a node by a
  value through `announced_text`.
- **A merged name took the text from under a hidden descendant.**
  `access_merge_subtree` skipped a hidden descendant's own name but went on
  merging its children, so text an application hid with `access_hidden(true)`
  on a wrapper was read aloud on the merged node. A hidden descendant and its
  whole subtree now contribute nothing, as the adapters treat them. The
  rustdoc of `AccessNodeBuilder::set_hidden`, `clear_hidden` and
  `access_hidden` said the flag was local to its node; it now says that it
  hides the subtree and that a wrapper which is only chrome is a
  `Role::GenericContainer` instead.
- **A field's validation message was never read on arriving at the field.**
  `access_described_by`, and the four stock inputs that wire their
  `ValidationStrip` with it (`TextInput`, `PasswordField`, `DateTimeEdit`,
  `DateRangeEdit`), promised WCAG 3.3.1: the message read as the field's
  description when it gains focus. AccessKit 0.25 passes the relation to no
  screen reader, on any platform: the consumer never derives a description from
  it, and no adapter exports it. So coming back to a refused field said its name
  and value and not why. The tree now writes each `described_by` target's text
  into the node's `description`, after its own, which Orca, NVDA and VoiceOver
  read. Around focus it holds new text back, because Orca speaks a change to
  the description of the node it holds as focus: a message appearing while the
  user is in the field is said once, by its live region, is not begun again as
  the user leaves, and is read with the field on the next visit. A shown tooltip
  no longer takes away its anchor's description either. **Behaviour change**: a
  node carrying `described_by` now carries a description. An application that
  announces a message itself as it sends focus to the field showing it is heard
  once on each platform, by a different voice: on Windows the arrival leaves
  the announced text out, since NVDA says the announcement; on Linux and macOS
  the arrival reads it, since Orca cuts an announcement to read the new focus
  (VoiceOver is unverified and treated the same way).
- **A throttled frame-tick subscriber held back every frame requested while it
  was on screen.** A widget on `subscribe_frame_tick_throttled` set the pace not
  only of its own tick but of every `request_frame`: with a once-a-minute clock
  painted, the announcer's follow-up syncs, a caret or a drag auto-scroll each
  waited up to a minute. An announcement queue drained one step per wait, so
  its later messages reached the screen reader at the user's next key press,
  spoken ahead of whatever that key said. The interval now delays the
  subscriber's own tick and nothing else; a requested frame is due at 60 Hz
  whatever is subscribed.

#### Widgets

- **A check box, a switch, a radio button or a slider changed without a
  screen reader being told.** On Space, an arrow key, a click or a screen
  reader's own activation, `Checkbox`, `Toggle`, `RadioButton` and `Slider`
  changed their state and no platform heard of it: the new state reached the
  accessibility tree only with the next unrelated update, most often the focus
  move that followed, where Orca's "checked" or "52" was cut by the new focus.
  Each change now reaches every platform in the frame that makes it, whether
  the user, a screen reader or the application made it, and so does a
  `Checkbox` in a `ListView` or `TreeView` row checked with Space on the row.
  A `Slider` also gives the platform its figures as they are written: an
  `f32` 0.3 was published as 0.30000001192092896, which Orca spoke digit by
  digit, and is now 0.3, as are its minimum, maximum and steps. A value the
  arrow keys reach reads as the step it reached: three steps of 0.01 up from
  0.3 are read as 0.33, not as the 0.32999998 the `f32` sums come to.
- **A dialog's content was hidden from assistive technology.** The panel
  `RecipeDialogStyle` draws around a `ModalContainer`'s content called
  `set_hidden()` to say it was only chrome, and the consumer every platform
  adapter reads through treats a hidden node as hiding everything under it.
  So on Linux, Windows and macOS a screen reader reached the focused control
  of a dialog and nothing else: not the title or the message as text, not the
  other fields and buttons, and not even the focused control when walking the
  dialog, whose children it no longer listed. Orca's flat review gathers
  what it reviews from those children (`getOnScreenObjects` in its
  `script_utilities.py`), so it had nothing to review, and a walk of the UIA
  tree met the same empty dialog, since the Windows adapter lists children
  through the same filter; what NVDA's object navigation made of it was not
  observed. The panel is now a bare `Role::GenericContainer`, which the
  adapters drop while keeping its children. Every `ModalContainer` is
  affected, on the default style and on the macOS and Fluent presets, which
  reuse it. A custom `DialogStyle` should do the same, as
  `DialogStyle::make_panel` now says. What that lets through is presented
  once: a `ModalContainer` whose content is itself a `Role::Dialog` or
  `Role::AlertDialog`, as a `MessageBox`'s and a `CommandPalette`'s is, now
  publishes a bare `Role::GenericContainer` instead of a second dialog of the
  same name, which Orca would speak as focus entered each; and the
  `CommandPalette`'s content is `Live::Off`, so the polite palette is
  announced by its name alone, and not row by row. **Behaviour
  change**: the container of a presented `MessageBox` is no longer a
  `Role::Dialog` named by the title. A test or a probe that found the box's
  buttons under that node finds them under the box's `Role::AlertDialog`,
  which holds them on this version and the last.
- **The content of a `Card`, a `Panel`, a `Toolbar`, a `StatusBar`, a
  snackbar and a `RadioTile` body was hidden from assistive technology.** The
  frames `RecipeCardStyle`, `RecipePanelStyle`, `RecipeSnackbarStyle` and
  `RecipeRadioTileStyle` draw, and a `Panel` marked `a11y_presentational`,
  called `set_hidden()` the way the dialog panel did. A screen reader found a
  card, a panel, a toolbar, a status bar or a snackbar empty except for
  whichever of its controls held focus, and never reached a tile's `body`.
  Each is now a bare `Role::GenericContainer`, and the snackbar's frame is
  also `Live::Off`, so a snackbar that announces its message says it once.
  The macOS and Fluent presets
  build their cards, panels and snackbars from these frames, and Material 3
  its cards, so they are fixed with them. The `make_body` of `CardStyle`,
  `PanelStyle`, `SnackbarStyle` and `RadioTileStyle` now says what a custom
  frame has to do.
- **An open menu listed none of its items to assistive technology.** The
  panel `RecipePopoverStyle` draws for its `Menu` variant, under every
  `MenuList`, `ComboBox` drop-down and search suggestion list, called
  `set_hidden()`. Whatever held focus could still be heard, since the filter
  lets a focused node and an active descendant through, but the menu listed
  no children, so a screen reader could not review the items around it. The
  surface is now a bare `Role::GenericContainer`, and
  `PopoverStyle::make_body` says what a custom one has to do.
- **Toasts were never announced and could not be reached.** The `ToastHost`
  every toast is mounted under called `set_hidden()`, so no toast ever
  entered the tree a platform adapter reads. The AT-SPI, UIA and macOS
  adapters all announce a live region as it enters that tree and pass over
  one that is filtered out, so a toast's `Role::Status` or `Role::Alert` said
  nothing on any platform, and its text and actions could not be reviewed or
  used from a screen reader. The host is now a bare `Role::GenericContainer`,
  and the toast's content is `Live::Off`, so a toast is announced by its
  title, once; its body is its description, read on reaching it, and its
  actions and close button stay reachable. `ToastStyle::make_body` says a
  custom style has to do the same.
- **A `TabWidget`'s panel, a `Stepper`'s step and anything inside a
  `Switcher`, `MaxSize` or `AspectRatio` was hidden from assistive
  technology.** The three layout primitives called `set_hidden()` to publish
  no node of their own, and a hidden node takes its whole subtree out of
  every platform's tree. `TabWidget` and `Stepper` show their pages through a
  `Switcher`, so the selected `Role::TabPanel` and everything in it could not
  be reached, and neither could a `Calendar`'s day grid, the title bar's
  maximize button or the debug inspector's panel, while a menu capped by
  `max_visible_items` lost its rows to the `MaxSize` around them. A screen
  reader met a control inside them only once focus reached it. All three are
  now a bare `Role::GenericContainer`, pruned from the tree, and the page a
  `Switcher` shows takes its place; its other pages stay out of the tree as
  before, because they are dormant. **Behaviour change**: none of the three
  publishes a node of its own any more, so what one wraps is a child of the
  node around it, in the update `sync_accessibility` returns and in an
  automation snapshot alike. The snapshot, which ignores the hidden flag,
  held the same pages before, under the hidden node.
- **A `Snackbar`'s button, the notification bell, a `TitleBar`'s centre
  content, an unlabelled `Splitter` pane and every `TreeTableView` cell were
  hidden from assistive technology.** Each sits inside a shell that called
  `set_hidden()` to keep itself out of the tree: the `Snackbar` around its
  trigger, `NotificationCenterButton` around its `IconButton`, the title
  bar's drag region around `TitleBar::center`, a splitter pane that has no
  `pane_label` (or a labelled one while collapsed, sliver and all), and the
  row inside each `TreeTableView` row. A screen reader could reach any of
  them only while it held focus, and a tree table's cells not at all. Each
  shell is now a bare `Role::GenericContainer`. **Behaviour change** for a
  test that read the drag region's hidden flag: it is no longer hidden, and
  still no stop.
- **A `Banner` was followed by its description, its action and its dismiss
  button.** The banner is a polite live region named by its title, and every
  node inside it that sets no politeness of its own inherits it, so the
  AT-SPI, UIA and macOS adapters announced each of them as a message of its
  own as the banner appeared, in the order of the consumer's hash set. Orca
  speaks each announcement with interrupt set, so what was left to hear was
  whichever came last. The banner's content
  is now `Live::Off`: its title is what it announces, once, and the rest
  stays reachable. A `StatusBar` with `announce_changes(true)` keeps the
  inheritance, which is what it is for.
- **Opening a `MessageBox` announced its default button, and not its
  question.** The box is an assertive live region so that its question is
  announced as it appears, and every node inside inherited the setting. The
  hidden dialog panel let only the focused default button through, and it
  was announced, assertively, as a message of its own, while the box's name
  never was: a confirmation opening on its default button announced "No",
  and its question reached a reader only as the name of the dialog
  presenting the box, around the focus. With the dialog's content reachable,
  everything inside the box is now `Live::Off`: the question, the box's own
  name, is what it announces, once, and its text and buttons stay reachable.
- **A focused `SpinBox` reported a nameless text field.** Focus lands on the
  spin box's editing field, while its name, value and range sat on a node
  around it, so a screen reader named the field once or not at all, and could
  not hear a step move the number. The editing field is now the spin button:
  `Role::SpinButton`, the `label` as its name, the value, range and step,
  `Increment` and `Decrement`, and its own text runs. A focus change says the
  name and value once, and a step changes the node the reader is on. What an
  application gives the spin box's id (`access_label`, a `FormLayout` label,
  `access_described_by`, a tooltip) reaches that node through the new
  `Widget::accessibility_proxy`. The accessible value is now exactly the text a
  reader can review, so a painted `suffix` is no longer in it.
  **Behaviour change**: a test that read the spin button off the `SpinBox`'s
  own id finds a `GenericContainer` there; the spin button is the field,
  `first_focusable_descendant` of that id.
- **A `SpinBox` with a custom `value_from_text` could not be typed in words.**
  The numeric input filter ran whatever the parser, so a month field that reads
  `"march"` as 3 dropped the letters as they were typed, and the commit put the
  old month back. A custom parser now receives every character and alone
  decides, at commit, what it can read; the default parser keeps its filter.
- **A `SpinBox` step threw away what had just been typed.** Up, Down, the
  wheel, the step buttons and an assistive `Increment` stepped from the value
  last committed, so 35 typed over 10 in a field stepping by 5 became 15 on
  `Up`. A step now reads the typed text first, as `Enter` would, and moves on
  from it: 40, with one `on_value_changed`. Text that cannot be read steps
  nothing and the held value is shown again, as in Qt. `Enter` and focus loss
  now commit keystrokes delivered in the same batch as the key that commits
  them, which they used to miss by a frame. **Behaviour change.**
- **The calendar spoke English and ISO dates to a user in any language.**
  In French the grid was "Calendar, septembre 2026", a day was "lundi août 31,
  2026", and the value read "2026-09-24 (selected: 2027-03-12)". Every string
  `Calendar` gives assistive technology is now in the user's language: the
  words from the framework bundle, and every date written by ICU for the tree's
  locale, in the locale's order and grammar, and on the Gregorian calendar the
  grid is laid out in even where the locale prefers another. French says the
  first of the month as "premier" ("1er" on screen), where ICU's bare "1" is
  read "un" by a speech engine, and Italian and Romanian say it "primo" and
  "întâi", where the digit is read "uno" and "unu" (Serianni, cited by the
  Accademia della Crusca; DOOM2): "lunedì primo marzo 2027", "luni, întâi
  martie 2027". The Italian date drawn under a range calendar writes "1º mar
  2027", and the Romanian one keeps the digit, which is how Romanian writes
  it. No other shipped language is rewritten: Spanish and Portuguese read the
  digit right as it stands, and the languages that read every day as an
  ordinal are a question for every day, not the first. The same grid now
  reads "Calendrier, septembre 2026", "lundi 31 août 2026" and "jeudi 24
  septembre 2026 (sélection : vendredi 12 mars 2027)", and a range is joined
  by words ("du … au …"). With the cursor on the one selected day, which is
  where a date field's popover opens, the day is said once, marked selected,
  where the value used to say it twice: "vendredi 12 mars 2027
  (sélectionné)". The title is ICU's month with its year, so Japanese reads
  "2027年3月"; the decade title reads "2020 to 2029" instead of joining the
  years with an em-dash; the line under a range calendar uses the locale's
  medium date and now follows a range committed from the keyboard or by the
  application, not only by a click. `DateEdit`, `DateRangeEdit` and
  `DateTimeEdit` open this calendar, so their popovers are fixed with it.
  `calendar-name-with-month` now takes the month with its year as its one
  `$month`, the unused `calendar-cell-name` is gone, and
  `calendar-value-with-selection`, `calendar-value-on-selection`,
  `calendar-date-range` and `calendar-decade` are new, in all 23 locales.
  **Behaviour change** for anything that read the grid's value as an ISO
  date.
- **`DateEdit`, `DateRangeEdit` and `DateTimeEdit` gave assistive technology an
  ISO value.** The value on the node standing for the whole field was
  "2026-05-02", "2026-05-01/2026-05-10" or "2026-05-02T14:35:07", which UIA
  and macOS hand to a screen reader as it stands. It is now the day in full in
  the tree's locale, "samedi 2 mai 2026", the range joined by words, "du
  vendredi premier mai 2026 au dimanche 10 mai 2026", and the day with its time,
  "samedi 2 mai 2026 à 14:35", with the seconds only when the field shows
  them. The editable text inside keeps its pattern. **Behaviour change** for
  anything that parsed those values.
- **A `Calendar`'s days and header buttons could not be reached, and
  arrowing through its days said nothing on Linux.** The day grid's body and
  the header row called `set_hidden()` so as not to be announced beside the
  `Role::Grid`, and hid what they hold with them: the 42 cells, each naming
  its date in full with its selected and today states, and the arrows and
  the title button. A screen reader found nothing inside the grid to review,
  and reached a header button only once it held focus. The day under the
  keyboard cursor lived only in the grid's value, which UIA and macOS report
  as a value change and AT-SPI carries on no interface, so Orca heard nothing
  while the cursor moved. Both are now a bare `Role::GenericContainer`, so
  the days are in the platform's tree, six rows of seven cells under the
  grid, and so are the header's five buttons. While it holds focus in the day
  view, the grid names the cell under the cursor as its active descendant,
  and AccessKit reports that cell as the focus on all three platforms: each
  arrow press, and each change of month from the keyboard, is a focus change
  to the new day ("samedi 13 mars 2027"). The value still says the cursor
  and the selection, for a client that reads the grid. A day offers
  `Action::Click` and, like a grouped `RadioTile`, no longer
  `Action::Focus`: the dispatcher moved keyboard focus onto a day an
  assistive technology focused, off the grid, and every arrow press after
  that moved the cursor in silence. `DateEdit`, `DateRangeEdit` and
  `DateTimeEdit` open this calendar and are fixed with it. **Behaviour
  change** for an automation client that focused a day, which is now
  reported unhandled.
- **A calendar said things twice.** The grid was a polite live region, and a
  live node speaks its name as it enters the tree and on every rename, with
  every descendant inheriting the setting. Opening a date field's calendar
  announced the grid's name, then each of the seven weekday headers, and then
  focus said the name again; PageUp or PageDown in the grid announced the new
  month while Orca spoke the rename of its focus as well; and each header
  button Tab reached was announced as it appeared and again as it took focus.
  The grid is no longer live, and the calendar's content is `Live::Off`, so
  one placed inside an application's own live region lends it the grid's
  name and no day, weekday or button. A change of month made from a header
  arrow, where focus stays on the arrow, is announced once through the
  tree's announcer, as the title now reads; the Today button announces the
  day it moved to, in full. Neither announces while the grid itself holds focus.
  **Behaviour change** for anything that listened to the grid's live region.

#### Data views

- **A single-selection data view could move its cursor off the selection.**
  `Ctrl`+arrow and `Ctrl` (⌘ on macOS) + `Home` / `End` / `Page` moved the
  cursor and left the selection behind, so a screen reader announced one row
  while actions used another. They now move the selection with the cursor in
  any of the five data views whose selection holds one entry: a
  `SelectionMode::Single` model — including one handed to a table in the
  default `MultiRow` mode — or a `TableSelectionMode::SingleRow` / `SingleCell`
  table. Multi-selection views are unchanged. **Behaviour change.**
- **A selection set by the application left the cursor behind.** In a
  single-selection data view the keyboard cursor stayed on the row the user
  last reached, so a screen reader went on announcing it and the next arrow
  key started from it. The cursor now moves with the selection when the
  application or a model change moves the selection. Multi-selection views are
  unchanged. **Behaviour change.**
- **A `GridView` never said how many tiles were selected, and announced each
  tile it realized.** The count was an English value on a grid marked as a
  live region. A grid takes its name from its label, so no platform announced
  the value, while every tile inherited the live setting and was announced as
  it scrolled into the realized window. The grid is no longer live. A click, a
  key, an assistive click or a marquee that changes how many tiles are
  selected now says the new count once, through the tree's announcer, in the
  user's language ("3 éléments sélectionnés"), Space on a tile the grid has
  not realized included. Moving a single selection says no count, since the
  tile the reader lands on says it is selected. The grid's value
  carries the same words. `grid-view-selection-count` is new, in all 23
  locales. **Behaviour change** for anything that read the value in English.

#### Terminal

- **A `Terminal` never announced its new output.** Each completed line went to
  a polite `Role::Status` live region as its value. A `Status` is named by its
  label, and every platform adapter announces a live node's name, so no
  platform said the line; only the automation ring, which read the value,
  recorded it. The line is now the region's name.

#### Automation

- **`list_live_regions` listed live regions no screen reader can reach.** It
  listed every node that declared a politeness: a hidden one, one inside a
  hidden subtree, and the framework's two announcer nodes, which are hidden
  while they have nothing to say. It now lists only the ones
  `accesskit_consumer`'s filter keeps, which are the ones a platform adapter
  walks and can announce. The set comes from
  `teksilo_core::accessibility::audit::nodes_in_filtered_tree`.
  **Behaviour change.**

## [0.13.1] - 2026-09-22

### Fixed

- **`cargo teksilo build-vectors` now runs on a Windows checkout.** Git for
  Windows checks the tree out with CRLF, and `corpus/index.json` holds exactly
  one raw newline — so the round-trip guard saw a one-byte difference and
  refused to run, reporting that the Rust schema and `tools/build_corpus.py`
  had diverged. They had not: the checkout had rewritten the file. The guard
  now compares what the generator wrote rather than what Git handed the
  platform. `cargo-teksilo`'s two corpus tests failed on Windows only for the
  same reason and pass there now.
- **Line endings are pinned to LF for everyone.** A new `.gitattributes`
  normalises the working tree on every platform, so a Windows clone means the
  same bytes as a Linux one. This is what the corpus depends on: `index.json`
  is compared byte for byte, and every file the corpus quotes is reconstructed
  line for line by `cargo teksilo show`. Existing Windows working trees keep
  their CRLF files until refreshed — `git rm --cached -r . && git reset --hard`,
  or a fresh clone.

## [0.13.0] - 2026-09-19

### Added

- **`cargo teksilo` — agent tooling for apps that depend on Teksilo.** An agent
  working inside this repository has the guides, the worked examples, the skill
  and the automation harness. An agent working in someone's Teksilo *app* had
  none of them: `docs/` ships in no crate, every example crate is
  `publish = false`, and the only probe harness lived in one app's repository.
  `cargo install cargo-teksilo` closes that gap, and every answer is matched to
  the Teksilo the app actually resolved.

  - `cargo teksilo symbol <Name>` — the exact public API of a type, read from
    the resolved sources. Works with a crates.io, git or path dependency and
    needs no checkout: where one is reachable it runs that checkout's own
    extractor, otherwise it stages a throwaway repository shaped like this one
    around the registry sources. A bare name resolves against **every** teksilo
    crate, so `cargo teksilo symbol ListModel` answers with teksilo-data's
    without `--crate data`; the note on stderr says which crate answered, and
    names the others when more than one defines that name. Types declared
    directly in a crate's `lib.rs` are included — teksilo-webview's `WebView`
    among them. This is the one subcommand that needs `python3` on PATH.
  - `cargo teksilo search "<question>"` — retrieval over the 70 hand-written
    guides and 56 worked example crates, which reach no consumer today. Hits are
    ranked and carry no score: neither ranker produces a confidence, and the
    lexical and hybrid numbers are not in the same unit.
  - `cargo teksilo show <path>` — the document behind a search hit, in full,
    offline: `cargo teksilo show docs/scroll-area.md`, or just the lines the hit
    cited (`--lines 166-172`, 1-based and inclusive, as `search` prints them).
    Nothing is re-fetched — a chunk already carries its own text and the line
    range it occupied, so the document is *reassembled* from the corpus, byte
    for byte. It closes the last version-binding hole in the tool: the path a
    hit cites exists in no consumer's project, and fetching it from `blob/main/`
    or the published book serves `main` rather than the version the app pinned.
    `search` now says so in a footer line, and `--list` prints every path.
  - `cargo teksilo probe` — writes the automation probe harness into
    `scripts/teksilo_probe/`, so an agent can drive the running app and assert
    on it. Generated files are checksummed: a local edit is reported rather
    than silently overwritten, and your own probes live outside the generated
    tree and are never touched.
  - `cargo teksilo setup` — the above, plus the Teksilo briefing for **every
    coding agent this project already configures, each in that agent's own
    format**: the full four-file skill into `.claude/skills/teksilo/` where it
    is native, and a self-contained ~40-line brief into
    `.cursor/rules/teksilo.mdc` (MDC frontmatter), `.windsurf/rules/teksilo.md`
    (`trigger:` frontmatter), `.github/copilot-instructions.md` and `AGENTS.md`
    (each a `<!-- BEGIN teksilo -->` region) where it is not. These tools share no
    format, so copying the skill directory into `.cursor/` would accomplish
    nothing; the brief never refers to the skill, which on those machines is
    not installed. It also pre-fetches the search encoder (~129 MB, into the
    per-user cache, `--no-model` to skip), so the first `search` does not stall
    on a download — and a failed fetch is a warning, because `search` degrades
    to BM25 by design.

    Three things it now refuses to do. It **never writes `$HOME` from project
    scope** — the previous version silently installed into `~/.claude/skills/`
    when the project had no `.claude/` of its own, a machine-wide change from a
    project-scoped command; `--user` is now the only path that reaches the home
    directory. It **never prompts where nobody can answer**: with no terminal on
    stdin the question is an error naming the flag that would have skipped it
    (`-y`, or `--user` when there is no `Cargo.toml` anywhere above the working
    directory), because CI and agents run this and a hang is worse than a
    failure. And it **never writes before showing the plan** — every path, every
    detected vendor, and the download with its size, then a confirmation. Runs
    are idempotent: a second run reports `unchanged` and leaves the shared files
    byte for byte, including whatever the project wrote outside the markers.

    **Cline** is served too, and is the one vendor whose path is read off the
    disk rather than fixed: `.clinerules/teksilo.md` where that directory
    exists, a marker region where `.clinerules` is a plain file — which it may
    be, and which has nowhere to put a file of our own — and
    `.cline/rules/teksilo.md` where that is the only layout present.

    The order is deliberately **not** newest-first. Cline's source calls
    `.cline` `CLINE_CONFIG_DIR` and `.clinerules` `DEPRECATED_CONFIG_DIR`, so
    the newer path looks like the obvious target; but the VS Code extension was
    hardcoded to `.clinerules` and ignored `.cline/rules/` outright
    (cline/cline#14186), the cross-surface fix reached `main` only in September
    2026 and is in no released build, and Cline's own docs still say its Rules
    panel creates new workspace rules in `.clinerules/`. Preferring the modern
    name would install, in the most-used Cline surface, a file nothing reads —
    which this tool holds to be worse than installing nothing, because it
    reports success. Reachability beats recency, and a test says so, so that a
    later tidy-up does not quietly invert it.

    No frontmatter in any layout: Cline's docs say a rule without one is always
    active, so its absence is what keeps the brief unconditional.

    Two things it refuses to do, both found by adversarially reviewing the
    command against itself and reproduced before being fixed. It **will not
    rewrite a shared file it cannot read whole**: `AGENTS.md` and
    `copilot-instructions.md` belong to the project, and reading them with
    `read_to_string(..).unwrap_or_default()` turned "cannot decode" into "the
    file is empty" — one Latin-1 byte was enough for `setup -y` to replace a
    project's rules with nothing but its own region, silently, reporting
    success. It now stops and names the file. And its stray-pruning compares
    filenames **case-insensitively**: the skill is written through
    `fs::write` but audited through `read_dir`, and on APFS or NTFS those
    disagree about case, so an exact comparison classified a file it had just
    written as a leftover and deleted it — leaving a skill with no `SKILL.md`
    while reporting `updated`. Folding can only err towards keeping a stale
    file, which is recoverable; deleting a live one is not.

    In user scope it reaches three agents — Claude Code (`~/.claude/skills/`),
    Mistral Vibe (`~/.vibe/AGENTS.md`, or `$VIBE_HOME`) and opencode
    (`~/.config/opencode/AGENTS.md`, or `$XDG_CONFIG_HOME`) — each **gated on
    its directory already existing**, so a home directory gains no config
    directory for a tool that was never run. Both environment overrides are
    honoured: Vibe relocates its entire state directory through `VIBE_HOME`, so
    writing the default path for a user who moved theirs would leave a file
    nothing reads. What was not found is reported with the path that was
    checked, not with a label that would send a relocated install looking in
    the wrong place. The rest have nowhere to go, which is likewise reported
    rather than silently narrowed: Cursor's user rules are edited in its
    settings UI, Copilot's personal instructions live on github.com, Windsurf's
    global rules are one file at
    `~/.codeium/windsurf/memories/global_rules.md`, and a repository-root
    `AGENTS.md` is per-repository by definition.

  **`cargo teksilo status`** reports what is actually installed — every agent, in
    both the project and the user scope, plus the search model, the Python 3
    interpreter `symbol` needs, and where each sits on disk. It reads only, and
    its exit code never depends on what it finds.

    It is the **dry run of `setup`**, not a second opinion: every agent row comes
    from `setup::inspect`, the read-only twin of the function that decides
    whether a write is needed, and the two share their content computation. A
    test pins the equivalence for every form — *`inspect` reports current exactly
    when `setup` would report `unchanged`* — because a status that computed
    "installed" its own way would eventually disagree with the command it claims
    to predict, and the disagreement would surface as a user following advice
    that does nothing.

    Three words (`here` / `not here` / `n/a`) and a reason beside each, because
    three words cannot carry the difference between *Cursor is not used in this
    project* and *Cursor is used here and has no brief*, and that difference is
    the whole of what to do next. An install from an older release reads `here`,
    since it is being read right now, with "from another release" in the detail —
    calling it anything else would be false. And `n/a` always says why: a bare
    one beside Windsurf would read as "Windsurf has nothing", when in fact it
    keeps a global file this tool declines to write.

    Its own command rather than `setup --status`: it reports on both scopes while
    `setup` is scope-selected, so the flag would have to mean something the
    unflagged command does not. Keeping the read-only thing out of a writing
    command's flag space also leaves no `--status -y` to reason about.

    "Reads only" is enforced rather than asserted. It asks cargo for the
    resolved version through `--locked`, because plain `cargo metadata`
    *resolves* — it creates a missing `Cargo.lock` and rewrites a stale one,
    which a command whose headline claim is that it touches nothing must not
    do. A project without an up-to-date lockfile therefore gets an honest
    "unknown" rather than a lockfile it never asked for. Paths reaching the
    report from `$VIBE_HOME`, `$XDG_CONFIG_HOME` or the working directory have
    their control characters escaped, so a newline in one cannot split a row
    and let its tail pose as another agent's line.

  Version binding is the design constraint, not a detail. The tool reads the
  app's `Cargo.lock`, and a minor or major mismatch **refuses** instead of
  answering — serving 0.12 answers to an app on 0.9 is worse than serving
  nothing, because `SplitView` was deleted outright in favour of `Splitter`
  between them and the wrong answer reads exactly like the right one. A
  patch-level difference warns and proceeds.

  **What a refusal can tell a model to run depends on which side of this
  release the app sits on**, because `cargo-teksilo` is the first crate in this
  workspace without version parity across the framework's history: it did not
  exist before 0.13.0, and no tag before that carries a
  `crates/cargo-teksilo` directory. `cargo-teksilo-fmt` is the contrast — it
  has been published at every teksilo version since 0.9.0, which is exactly why
  a `--version <app>` line can be emitted there unconditionally and cannot be
  emitted here. So there are three regimes. **Below 0.13.0 no install command
  exists at all**, and the refusal says so and says to move the app forward;
  printing routes that cannot work is worse than printing none, because the
  model spends its turn on them and then falls back on its own memory of the
  API anyway, which is the single failure this tool exists to prevent. **At
  0.13.0 or above both routes print unconditionally** — `--version <v>
  --locked` from the registry, or `cargo install --path
  <checkout>/crates/cargo-teksilo --locked` from the framework tree — because
  whether a given version reached crates.io is not observable offline, and an
  app pinning teksilo by `path` or `git` resolved a version that was never
  published at all. **An unparseable version gets the checkout routes**, which
  are the ones that do not depend on the registry. Where routes print, the
  refusal also names the two ways to keep apps on different minors working at
  once — installing the second with `--root <dir>` and putting that
  `<dir>/bin` first on PATH for that tree, or running the tool straight out of
  a checkout with `cargo run -p cargo-teksilo`.

- **`teksilo-corpus`** — the guides and the worked examples, chunked into a
  retrieval index and published per release so `cargo` resolves the corpus
  matching an app's Teksilo. It is one file: a chunk carries its own text, and
  its `path` names the original in this repository (`docs/scroll-area.md`,
  `examples/simple_button/src/main.rs`), so a search result cites something that
  opens. Data only; it carries no ML dependency. A guide's closing navigation
  footer — "See also", "Reference", "Code references" — is carried under its own
  `kind` and excluded from retrieval rather than from the corpus: `show`
  reassembles a document from its chunks, so dropping one truncates the file,
  while indexing one lets a short list of links outrank the prose it points at.

- **The automation probe harness**, written into a consumer's project by
  `cargo teksilo probe`. Python, stdlib only, embedded in the binary rather than
  published to an index — so it is version-matched by construction, lands in the
  repository where an agent reading that repository can see it, and needs no pip
  or virtualenv. It supplies the JSON-RPC client every probe previously
  hand-rolled, a tool surface generated from `TOOL_CATALOG` (so it cannot drift
  from the bridge), and the virtualized-view navigation rules that are otherwise
  rediscovered one misdiagnosis at a time: an off-screen row has no AT node, a
  row scrolled back into view is a new widget with a new id, and clicking a row's
  reported bounds below the viewport hits empty chrome. Three worked examples
  ship with it and run against this repository's own example apps in CI.

- **Three new guides**: [Agent tooling](docs/agent-tooling.md) documents the
  above; [Scroll areas](docs/scroll-area.md) is the reference the docs did not
  have — scrolling was covered only by an architecture chapter and the
  kinetic-scroll physics, so an ordinary "how do I make this scrollable?" had
  nothing to land on; and [Docking layout](docs/docking.md) is the consumer
  guide for `DockingLayout`, which had only a generated catalog page and a
  design note written for reviewers rather than for users. Those gaps were found by
  running retrieval tests over the corpus, not by reading the table of contents.
  The design note is gone; its unstarted work is now
  [Horizontal activity rail (backlog)](docs/docking-horizontal-rail.md), which
  the retrieval index leaves out — a proposal written in the instruction voice
  reads as shipped API to whatever retrieves it, and this one outranked the
  guide's own activity-rail section.

- **`tools/extract_widget_api.py` now covers every crate with a public API**
  (30, up from 4), including the types a crate declares in its own `lib.rs`.
  mdBook catalog generation stays scoped to the four cataloged crates, so
  `docs/` is unchanged; the rest are queryable through `--crate`, `--list`,
  `--all` and by name.

- **`cargo-teksilo` ships a README**, so a crates.io reader learns what the
  `semantic` default feature pulls in, that `--no-default-features` is a fully
  working tool, where the model cache lives, and that `symbol` needs
  `python3` — none of which was readable outside this repository.

### Changed

#### Breaking changes

- **`OverlayDismissCallback` is `Rc<dyn Fn(DismissReason, &mut EventContext)>`**,
  where it was `Rc<dyn Fn()>`. A closure that wants neither gains two ignored
  parameters — `Rc::new(move |_, _| …)`. The new `#[non_exhaustive]`
  `teksilo_core::overlay::DismissReason` names the route that closed the
  overlay: `Escape`, `OutsidePress`, `PointerLeave`, `Cascade` or
  `Programmatic`. Both `OverlayRequest::on_dismiss` and
  `ModalRequest::on_dismiss` carry the new type. The five `OverlayManager`
  dismissal methods that do not name a reason — `dismiss`, `dismiss_top`,
  `dismiss_all`, `dismiss_except` and `dismiss_with_focus_restore` — keep the
  signatures they had and report `Programmatic`; their `dismiss_because`,
  `dismiss_top_because`, `dismiss_all_because`, `dismiss_except_because` and
  `dismiss_with_focus_restore_because` twins take the reason instead.
- **Dismissing an overlay straight through `OverlayManager` no longer runs its
  `on_dismiss`.** Code reaching through `WidgetTree::overlay_manager_mut()` to
  dismiss calls `WidgetTree::dismiss_overlay` instead. The ordering is
  unchanged — the callback still runs during dismissal, before focus returns to
  the trigger.
- **`MessageBoxResult::dismissed_by_escape` is replaced by `dismissal`.**
  `MessageBoxDismissal` is `#[non_exhaustive]` and names the route — `Button`,
  `Escape`, `ClickOutside` or `Programmatic` — where the boolean could only say
  Escape or not, while its own rustdoc claimed to cover a click outside that no
  code path could produce. `result.dismissed_by_escape` becomes
  `result.dismissal == MessageBoxDismissal::Escape`; where the flag was read as
  "the user did not choose a button", read `result.was_dismissed()`. `button`
  is unchanged, and still carries the escape-button resolution on every route.

### Fixed

- **A screen reader can open a `Dialog` that has a custom trigger.** The
  trigger published a named `Role::Button` node that advertised no actions at
  all and answered none, so assistive technology could see the control and not
  press it — while a mouse worked. This reached the ordinary
  `Dialog::new(label)`, not only an explicitly hand-built trigger, and the same
  shape affected `Snackbar` and a `PopoverWidget` over a custom trigger. The
  node now advertises `Action::Click` and acts on it.
- **A modal presented as a native OS window honours its `ModalCloseBehavior`.**
  Escape did nothing to it on macOS and Windows — including in the default
  configuration, where `Dialog` asks for `EscapeOrClickOutside` and the
  presentation resolves to a real window. The click-outside half stays
  unavailable there, deliberately: the OS blocks the parent window while the
  modal is up, so there is no outside to click, and `ClickOutside` now means
  the same as `Manual` for that presentation. Linux and the BSDs were never
  affected — they present modals in-tree, where Escape already worked.
- **A `MessageBox` dismissed with Escape or a press outside reports its
  answer.** The dialog closed and the application was told nothing —
  `on_result` ran only when a button was pressed, so a "Save changes?" prompt
  could be waved away and leave the caller with no idea whether to save. This
  is the in-tree presentation, which is what `ModalPresentation::Auto` resolves
  to on Linux and the BSDs — every unix but macOS — and so was the default
  behaviour there; `ModalPresentation::InTree` reaches it on any platform. Both
  routes now resolve the escape button and report it, with
  `MessageBoxResult::dismissal` naming which one closed the dialog, and a
  button that has already answered is not reported over. Distinct from the
  native-window fix above, which made Escape close such a window; this makes
  the closing report.
- **An `InputDialog` dismissed without a button press reports the
  cancellation.** `None` is this dialog's cancellation payload, so a dismissal
  that reported nothing was indistinguishable from a dialog still sitting open
  — against the type's own contract, which promises the callback runs exactly
  once when the user accepts or cancels. Escape, a press outside and an
  application-driven dismissal now all deliver `None`. The in-tree presentation
  only: a natively presented `InputDialog` — macOS and Windows — still reports
  nothing when Escape closes it, and `MessageBox` reported correctly on that
  presentation already.
- **A `CommandPalette` runs its `on_dismiss` when the palette is dismissed.**
  The hook fired only when a command was actually invoked, so a caller that
  installed it to release whatever opening the palette reserved was never told
  about the two commonest ways of closing the palette. `CommandPalette`
  presents in-tree on every platform, so every platform was affected. It now
  runs on every route, and once only.

## [0.12.1] - 2026-09-18

Five assistive-technology fixes and a Windows start-up fix. In a rich-text
document, tables and blockquotes reached the accessibility tree as nothing at
all, and text typed inside a cell, a heading or a blockquote was never
announced; a `TreeTableView` row reported neither its outline level nor whether
its branch was open; a `TreeView` row that was still loading claimed to be at
the top level. On Windows, an app whose Vulkan driver cannot build wgpu's
device now starts, and Direct3D 12 is tried before Vulkan.

### Changed

- **Behaviour change.** On Windows, Direct3D 12 is tried before Vulkan and
  OpenGL. Other platforms keep the order they already had: Metal first on macOS
  and iOS, Vulkan first elsewhere. `WGPU_BACKEND` still overrides all of it.

### Fixed

- **A screen reader can read the tables and blockquotes in a rich-text
  document.** Neither reached the accessibility tree at all: the walk over the
  document flow handled only ordinary blocks, so a table's cells and a
  blockquote's paragraphs contributed no text — the characters were simply
  absent from what a screen reader could read, with or without structure
  around them. A table is now exposed as `Role::Table` / `Role::Row` /
  `Role::Cell` carrying its dimensions, each cell's coordinates and any spans,
  and a box per cell — over every row and column track a merged cell covers. A
  blockquote is announced as `Role::Blockquote`.
- **Typing inside a table cell, a heading or a blockquote is announced.** A
  text run's text-change event is routed by the platform adapters to its
  filtered parent, and dropped unless that node supports text ranges — which
  none of `Role::Cell`, `Role::Heading` and `Role::Blockquote` does. Runs
  beneath all three now sit under a text container that does, so edits there
  reach the platform instead of going silent. Each keeps its own node, so
  heading navigation and quote announcement are unaffected. The same fix
  applies to `CodeEditor` and `PlainTextEditor`, whose headings had it too.
- **A screen reader announces which level of the outline a `TreeTableView` row
  is on.** The depth was published on the row — but the row is an ancestor of
  the cell the view puts the accessibility cursor on, and an ancestor is the
  one place NVDA will not read a level from, so the number reached nobody. The
  tree column's cell now carries the level too. Windows only, because it is the
  only platform whose AccessKit adapter exposes a level at all; elsewhere the
  indent is what carries depth, as before.
- **Opening or closing a `TreeTableView` branch is announced.** The expand
  state was published on the row, and a row is not the node the view puts the
  accessibility cursor on. Toggling a branch moves that cursor nowhere, so
  there was no focus change to carry the news and the property change was
  raised on an element the screen reader was not on: opening and closing a
  branch was silent. The tree column's cell now carries the state, so the
  change lands on the node the user is actually on. Expand and collapse from
  assistive software keep working as before, on the cell as well as the row.
- **A `TreeView` row that is still loading no longer claims to be at the top
  of the outline.** A row whose data had not arrived published "level 1,
  item 1" as fact, so a placeholder four levels down announced itself as a
  root. It now publishes neither until its metadata resolves, and then
  announces the truth.
- **An app starts on a machine whose Vulkan driver cannot build wgpu's
  indirect-call validation pipelines.** It died inside `request_device` with
  "buckets are not empty, at least one BGL has not been unregistered" —
  before the adapter search could reach another backend — and no window ever
  appeared. That validation is now off unless `WGPU_VALIDATION_INDIRECT_CALL`
  asks for it; this renderer issues no indirect draws for it to check.

## [0.12.0] - 2026-09-17

`teksilo-scene` becomes an editor. A selection can be moved, resized and
rotated — with a pointer, from the keyboard and through a screen reader — where
a heavyweight item could not be dragged with a pointer at all. `SceneCard` is
the container a note lives in, and a card can hand its height to its own words.
One picker now answers every "what did the pointer hit?", over one geometry
descriptor that also brings Qt's four selection modes and a lasso. A pan over
fifty thousand *lightweight* items costs what a pan over an empty scene costs,
and the cards nobody can see cost no accessibility node and no Tab stop. Around
that: a third
paint band and a surface for ink that is still wet, every position the OS
batched, a seam a data layer can reverse a scene edit through, and a finger that
can pan a `SceneView` with selection or magnetism switched on.

Breaking, pre-1.0: `PathItem::new` takes one argument, `SceneItem` loses two
methods and gains one, `Path::commands` is a method, `item_change_signal`
carries an envelope, and several types the crate hands *to* consumer code are
now `#[non_exhaustive]`. Each one is listed under **Breaking changes** below
with what to do about it.

### Added

#### Moving, resizing and rotating a selection

- **A selection transform controller**, opted into per view with
  `SceneView::transform_controller(TransformConfig)`: a frame around the
  selection, eight resize handles, a rotate handle and a body drag. Knobs for
  aspect lock, centred scaling, rotation snaps and their tolerance, a minimum
  size, handle and padding sizes, edge auto-pan distance and speed, and a
  `chrome` closure for painting the whole thing yourself. `on_start` /
  `on_change` / `on_end` report the gesture; `LivePreview` decides whether the
  content moves under the frame or only the frame does.
- The model is written **once, when the gesture ends**, through
  `Scene::apply_transform_delta`. One gesture is one step to reverse, `Esc`
  cancels with nothing to roll back, and a sample costs a relayout rather than
  one model write per selected item.
- **Keyboard**: the transform key (`t` by default) enters the controller, `Tab`
  cycles the handles, the arrows move or resize by a step, `Esc` cancels.
- `SceneView::transform_session_signal()` publishes the live gesture, for an
  app-owned inspector, status bar or size readout.
- **Assistive technology**: every handle is published with a role, a name and —
  where it drives a single number — a value. `Increment` and `Decrement` move
  the coordinates that handle actually drives, and each two-dimensional handle
  also advertises a named action per direction (`TransformStep`), so a width can
  be changed without a height. `TransformLabels` takes `tr!` like every other
  label.
- **Both tiers move.** A heavyweight card is draggable with a pointer at last;
  the lightweight drag, the `Alt`+arrow nudge and the controller all go through
  one door.
- `SceneView::transform_enabled_signal()` and `magnetism_enabled_signal()` for a
  toolbar to bind. Turning either off takes its chrome off the screen *and* out
  of the published accessibility tree.

#### `SceneCard`, and a card that sizes itself to its content

- **`SceneCard`** — the container a heavyweight item usually wants: a surface, a
  header that is the grab handle, a selection ring, three `CardMode`s
  (`Idle` / `Selected` / `Editing`) and one named `Role::Group`, with no opinion
  about what is inside it. Dragging the title strip moves the card; dragging the
  prose inside it selects text; a trailing header slot holds buttons that can be
  clicked — even with the jitter a real click carries — without starting the
  drag. Slots take a widget, a boxed widget or an id, and `surface(..)` replaces
  the chrome entirely.
- **`SizePolicy`** on a heavyweight entry — `Fixed` (today's behaviour and the
  default), `HeightForWidth` (the width is authored, the height follows the
  words) and `Intrinsic` (the entry shrink-wraps its widget). Set it with
  `Scene::set_size_policy` / `SceneModel::set_size_policy`, or with
  `SceneCard::height_for_width()`. Only the cards a pass lays out are measured,
  so an off-screen card keeps the estimate it was created with and a pan still
  costs the viewport. `Scene::set_measured_size` is there for an app measuring
  something the framework cannot.
- **A `SceneView` answers `ScrollIntoView`.** A caret moving inside an embedded
  editor pans the camera to follow it, with no app wiring, and an outer
  `ScrollArea` is re-targeted to where the card will land. It honours reduced
  motion, which the public `SceneView::ensure_visible` cannot.
- Double-clicking a card puts the caret in its body even when that body is a
  `Switcher` that swaps the editor in on the next pass.

#### One geometry, one picker

- **`SceneItem::shape() -> ItemShape`** is how an item describes its geometry: a
  rectangle, a rounded rectangle, an ellipse or a path, with a fill rule and an
  optional stroke band in local or screen units. It replaces `shape_contains`
  *and* `clone_shape_test`, which had to be kept in agreement by hand.
- **Qt's four `ItemSelectionMode`s** — `IntersectsItemShape` (the default),
  `ContainsItemShape`, `IntersectsItemBoundingRect` and
  `ContainsItemBoundingRect` — chosen per view with
  `SceneView::marquee_selection_mode`, or per call on `Scene::items_in_region`
  and `colliding_items_with`.
- **`SceneRegion` is a rectangle *or* a path**, so a lasso is expressible:
  `SceneRegion::lasso(path)`, `::stroke(path, width)`, `::from_screen_rect(..)`.
  A rotated marquee stops over-selecting.
- **One resolver decides every pick.** `PaintKey` — band, then `z`, then
  insertion order — is the order the tap, the drag, the hover, the cursor, the
  tooltip, the hold and `Scene::item_at` all read. `claims_press` and
  `hit_testable` are public for a consumer running its own pass.
- `PathItem` carries a real `fill_rule`, used by painting and hit-testing alike,
  and `hit_stroke_width` for a hairline that should still be clickable.
- **On `Path`**: `flatten(tolerance)` (a polyline per subpath),
  `exact_bounds(tolerance)` (a tight box, where `bounds()` returns the
  control-point hull and so over-reports every curve),
  `contains_point(p, rule, tolerance)`, the `Subpath` type, and `arc_to_cubics`
  shared rather than duplicated inside the path atlas.

#### Ink

- **A third paint band.** `SceneLayer::Interleaved` orders a lightweight item
  against the heavyweight cards by `z`, so a dried stroke can sit above note A
  and below note B — which two bands could not express. The pointer still
  resolves through the lightweight hit snapshot, the item keeps its own
  accessibility node, and the Tab ring does not move.
- **`WetLayer`** — a surface for content being authored right now, installed
  with `SceneView::wet_layer(..)`. It repaints on its own without re-running the
  band beneath it, and always sits above every card and interleaved item and
  below the `Over` band. `WetLayer::request_repaint(ctx)` from a pointer
  handler; `WetNode` names the nodes it owns.
- **`EventContext::coalesced()`** — the positions the OS batched into the packet
  being dispatched, oldest first, each with its own timestamp *and* its own
  pressure and tilt. A drawing surface fans out over it and then over
  `pointer_position()`. Opt the producer in with
  `TeksiloAppBuilder::pen_batching(PenBatching::Coalesce)`, which emits one
  dispatch per pen drain instead of one per packet and never folds a transition.
  Not `on_drag` — the drag recogniser swallows every move inside its slop and
  reports the press position, so the start of a stroke never arrives there.
- **[docs/ink.md](docs/ink.md)** — the shape of a drawing tool on a scene page:
  what to read a stroke from, what to put on which band, and what a wet stroke
  costs.

#### A seam a data layer can reverse a scene edit through

Undo itself stays in the data layer; this crate ships no stack and no `undo()`.
What it ships is a record complete enough to invert.

- Every notification carries a transaction id, a `ChangeSource`
  (`User` / `Programmatic` / `Remote`), a `HistoryMode`
  (`Record` / `RecordPreserveRedo` / `Ignore`) and an `ephemeral` flag. The
  framework stamps its own gesture commits `User`, so an app can tell a finished
  drag from a programmatic move.
- **One write scope is one transaction**, so removing a subtree is one change to
  reverse rather than N. Group several calls with `SceneModel::transaction` /
  `user_edit`; nesting joins rather than splits. `SceneTransaction::abandon`
  tags a cancelled interaction, and `squash` (off by default) folds a
  transaction's repeated writes to one quantity into its endpoints.
- **`Scene::take` / `Scene::restore`** — an owning salvage door. `take` hands
  back the item box, its magnets (ids included) and its whole slice of the
  logical accessibility tree; `restore` puts it back at the **same** `ItemId`
  and the same place in the reading order, which is what selection, magnets,
  relations and every app side-map are keyed on. `Scene::remove` is the same
  call with the salvage routed to the edit sink, and `RemovedItem::detach()`
  forgets the recorded parent so a salvage restores at root level — or into a
  different scene.
- **`SceneModel::set_edit_sink`** receives one owning `SceneTransactionRecord`
  per committed transaction, with the scene unborrowed so the sink may read and
  write it; the sink's own writes are journaled in turn. `transaction_signal`
  fires once per transaction, after the sink.
- `Scene::replace_item` swaps a lightweight item's box while keeping the entry,
  the id and everything keyed on it. `Placement { parent, z, local_pos,
  transform }` is written as one property with `set_placement`, plus
  `reparent_keeping_scene_pos` for a drag into a group that must not move the
  item, and `z_between`, which reports when `f32` precision has run out at a
  locus.
- `ItemChange::is_edit()` separates the scene's edits from the derived
  notifications emitted beside them, so an app counting the changes in an edit
  gets the number the transaction record has.
- Accessibility readers to match the writers: `Scene::a11y_live_of` /
  `a11y_landmark_of`, and `a11y_relations` / `a11y_live_of` /
  `a11y_landmark_of` / `a11y_categories_of` on `SceneModel`.

#### Saying what a change should become before it is applied

- **A geometry constraint on `SceneModel`** — one closure that rewrites a
  gesture's proposed geometry *before* anything is applied, so snap-to-grid,
  axis lock and page clamping reach the drag ghost instead of correcting it a
  frame late. Install it with `SceneModel::set_geometry_constraint`; it is handed
  a `ProposedChange` (the scene read-only, the roots the gesture moves, and the
  gesture's start and proposed frames in scene coordinates) and returns a
  `ChangeVerdict` — `Accept`, `Adjust(frame)` or `Reject`.
- It governs the transform controller on both tiers, the lightweight item drag
  and the `Alt`+arrow nudge, for pointer, keyboard and assistive technology
  alike. An app driving its own drag calls `SceneModel::constrain_move` /
  `constrain_frame`. Programmatic mutators are never constrained.
- **`SceneModel::downgrade()` and `WeakSceneModel`** — a non-owning handle. A
  constraint normally needs no handle at all (`ProposedChange::scene` is the
  whole read surface); this is for a policy object that holds one for its other
  work, and it is what keeps such a closure from leaking the scene that owns it.

#### Core, platform and canvas

- **`Widget::accepts_child_hit(child, point)`** — a parent can veto one child for
  one point, which is the per-point question `hit_transparent` (a per-node
  declaration) cannot ask.
- **`EventContext::dispatch_target()`** — the arena's own answer to who the press
  landed on, so a container need not guess it from a rectangle.
- **`EventContext::release_cursor()`** — withdraws a handler's cursor override so
  the node-declared one resolves again.
- **A pan claimant may carry its own drag.** A node that declares a `PanClaim`
  and also installs `on_drag` now competes as both: the pan wins at `pan_slop`,
  and a hold arms the node's own drag. `LongPressRole::DragHandle` declares that
  the hold inside a subtree belongs to that drag, and applies to a direct
  pointer only — a mouse enrols no pan member at all, so nothing about it
  changes.
- **Every pen sample carries the device's own time.** `PenPacket::device_time_ms`
  plus a shared `back_date` rule that places a drained batch on the tree's
  timeline, wrapped 32-bit counters and missing clocks included. Windows now
  drains `GetPointerPenInfoHistory`, so a digitizer running above the message
  rate delivers every sample with its own pressure and tilt rather than one per
  message.
- `AccessNodeBuilder::set_custom_actions` advertises `Action::CustomAction`
  itself, so a node's custom actions are reachable rather than decorative.
- A stroke carrying a `StrokeStyle` — dashed, dotted, or a custom cap or join —
  reaches the screen from every canvas stroke primitive, not only `stroke_path`.

#### Accessibility

- **`SceneMinimap` is operable.** Given an `on_click` it is focusable, publishes
  a `Role::Group` node, advertises `Click` and the four scroll actions, pans with
  the arrow keys and recentres on `Home` / `Enter` / `Space`. `access_readout`
  supplies the phrasing for where the viewport sits, as a `MinimapReadout`, so it
  can be localised.

#### Demos

- **`cargo run -p scene-ink`** — every pen sample through `ctx.coalesced()`, a
  `WetLayer` that repaints alone, and a dried stroke interleaved between two
  notes.
- **`cargo run -p scene-corkboard`** — beats are `SceneCard`s with real editors:
  drag the title strip to move one, drag its prose to select text, pan a beat
  off-screen and keep typing (the camera follows the caret), and "Fit to text"
  hands every beat's height to its words. Both panes carry a transform
  controller, and one "Snap to grid" toggle drives a single geometry constraint
  shared by both.

### Changed

#### Breaking changes

- **`PathItem::new(path)`** takes one argument and derives its own bounds. Drop
  the `local_bounds` you were passing; if you were widening it to make a hairline
  clickable, use `hit_stroke_width` instead.
- **`ItemFlags::NEGATIVE_Z_BEHIND_PARENT` is removed.** It was declared and
  documented but read by nothing, and `PaintKey` is flat, so it could not have
  been honoured without making the key hierarchical. Nothing observable changes;
  delete the reference.
- **`PointerSequence::has_deferred_grab()` is now `has_deferred_grab_for(id)`.**
  A deferred grab is a statement about the node it arms, not about the press:
  read sequence-wide it suppressed every descendant's touch long-press and
  context menu. Pass the node you are asking about.
- **`SceneItem::shape_contains` and `clone_shape_test` are gone**, replaced by
  `shape() -> ItemShape`. An item that overrode either overrides `shape()` now;
  the default returns the item's `local_bounds`, which is what an item that
  overrode neither already got.
- **`SceneItem::set_fill` / `set_stroke` return `AppearanceWrite<T>`** instead of
  `bool`, carrying the value they overwrote — the only place a journal can get it
  from. `AppearanceWrite::Refused` is the default, and is what an item with no
  such slot should keep returning.
- **`SceneItemA11yContext` publishes scene coordinates.** `screen_bounds`,
  `local_to_screen`, `advertised_bounds` and `bounds_space` are replaced by
  `scene_bounds` and `local_to_scene`; the camera is declared once as an
  AccessKit transform above the whole subtree. An item emitting its own
  sub-element geometry stops projecting through the view transform and publishes
  in scene space. `A11yBoundsSpace` is removed with them.
- **`item_change_signal` carries a `SceneChange`** — the change plus its
  transaction envelope — rather than a bare `ItemChange`. Observers read
  `notification.change`.
- **`ItemChange` is no longer `Copy` or `PartialEq`** (it carries owned values
  now) and gains `PlacementChanged`, `ItemReplaced`, `HandlersChanged`,
  `MeasuredSizeChanged` and `SizePolicyChanged`. `TransformChanged`,
  `PayloadChanged` and `AppearanceChanged` carry both sides of the value they
  replaced.
- **`Path::commands` is a method.** Read with `commands()`, append with `push`,
  build from a vector with `Path::from_commands`. The field backs a rolling
  content stamp that a public `Vec` could not be kept in step with.
- **`PointerSample::coalesced` is a `Vec<CoalescedSample>`**, not a tuple — each
  batched position keeps its own axes, which the tuple dropped.
- **`PenPacket::time: EventTime` is replaced by `device_time_ms: Option<u32>`**,
  the device's own counter rather than a stamp with an unknown epoch, and
  `ToolEvent::Frame` carries `time_ms`. A backend reports what the device said;
  `back_date` places the batch on the tree's timeline.
- **`#[non_exhaustive]` on the types the crate hands *to* consumer code**, so a
  new field is not a source break next time: `ItemChange`, `SceneLayer`,
  `MagnetVisualState`, `MagnetRef`, `MagnetConnection`, `MagnetSnap`,
  `MagnetMarker`, `MagnetFeedback`, `SceneItemPaintContext` and
  `SceneItemA11yContext`. A `match` gains a `_` arm; a struct literal becomes the
  type's `new` constructor, which every one of them now has, with its fields
  still public.
- **`WidgetPlacement` gained a `dormant` field and is now `#[non_exhaustive]`.**
  A `place_children` implementation reads and writes the slice it is handed and
  is unaffected. What breaks is a struct literal — `WidgetPlacement { id, origin,
  size }` — which becomes `WidgetPlacement::new(id, origin, size)`. The field
  arrives pre-set to the child's current state, so a parent that ignores it
  changes nothing; it is read only for a parent whose `culls_children` returns
  `true`.
- **`SceneCard` has one method per slot.** `header_id`, `header_boxed`,
  `header_trailing_id`, `header_trailing_boxed`, `body_id` and `body_boxed` are
  gone; `header`, `header_trailing` and `body` each take a widget, a
  `Box<dyn Widget>` or a `WidgetId`. Drop the suffix — `.body_id(id)` becomes
  `.body(id)`. This follows the slot-twin purge the widget catalog went through
  in 0.11.0, so a card addresses its slots the way every other container does,
  and last call wins where before an id set after a widget left both stored.

#### Performance

- **A pan costs the viewport, not the model.** One pan sample over a scene with a
  50 000-item **lightweight** off-screen tail went from **16 ms to 1.8 µs** —
  what a pan over an *empty* scene costs. The two hit snapshots are pure
  functions of the model, so they are built once and patched per item from the
  change stream instead of being rebuilt on every sample.
  (`crates/teksilo-scene/tests/pan_scaling_probe.rs`.)
  A pan over off-screen **heavyweight cards** is cheaper than it was but is not
  flat: the cards are parked rather than laid out, and what remains is the
  framework's own walk over their arena nodes.
  (`crates/teksilo-scene/tests/heavyweight_retention_probe.rs` prints both.)
- **Off-screen cards park.** At 50 000 of them the published accessibility node
  count and the Tab-stop count equal the on-screen counts exactly, and the
  accessibility walk no longer grows with the tail. A card carrying focus or a
  live pointer is pinned wherever the camera goes.
  (`crates/teksilo-scene/tests/heavyweight_retention_probe.rs`.)
- **Moving one item costs the moved subtree, not the scene.**
  `Scene::set_local_pos` walks a kept adjacency list instead of rebuilding a
  scene-wide parent map on every move.
  (`crates/teksilo-scene/tests/mutation_scaling_probe.rs`.)
- **A hover no longer costs the selection.** `Scene::selection_roots` is linear
  rather than quadratic in the number of selected items, and the transform
  controller resolves it once per hover instead of thirteen times per pointer
  move. With the whole of a 5 000-item scene selected, one `selection_roots`
  call went from 177 ms to a figure that no longer grows with the selection, and
  a drag sample in a scene with no constraint installed no longer pays for the
  selection at all.
  (`crates/teksilo-scene/tests/selection_roots_scaling_probe.rs`,
  `transform_scaling_probe.rs` print the tables.)
- **A growing wet stroke is no longer quadratic.** The renderer's path-mask cache
  keys on a rolling content stamp, so a cache hit does not scale with the path's
  length. (`crates/teksilo-render/tests/wet_stroke_cost.rs`.)

#### Scene

- **An `item_change_signal` observer may read and write the scene.** Changes are
  queued and drained after the mutation's borrow drops — the `teksilo-data`
  mutate-then-notify discipline — so the snap-to-grid pattern the docs describe
  is implementable at last. An observer that panics mid-flush costs that
  observer's delivery and nothing else. A `CascadeBudget` bounds one drain's
  observer-generated deliveries (100 000 by default, flat, a knob on
  `SceneModel::set_cascade_budget`), and its diagnostic names the subject that
  piled up.
- **`SceneListAdapter` keeps each row's `ItemId`.** A row whose content changed,
  and every row an insert or a removal shifted, keeps the id it had — so
  selection, magnets and accessibility parenting survive a data change instead of
  being retired with it. Each source change is one transaction.
- `ItemChange::VisibilityChanged` is emitted by **every** door that flips
  `IS_VISIBLE`, not only `set_flag`, and is a derived notification rather than an
  edit. Hiding a card is one recorded edit and two notifications whichever door
  it went through; it used to be two edits through one and one through the other.
- `Scene::set_z` ignores a write only when the entry already holds that exact
  value. Its old `f32::EPSILON` test never fired at 1e6 and swallowed millions of
  distinct floats near zero — including the midpoints `z_between` hands out, so a
  caller was told there was room and then got a silent no-op.
- A rotation applied by `apply_transform_delta` that also moves the item now
  emits `TransformChanged` as well as `LocalPosChanged`; `set_transform` ignores
  a write that changes nothing.
- `ItemChange::HandlersChanged` carries the handler sets it replaced.
  `handlers_mut` cannot know what the caller does with the `&mut` it hands out,
  so it announces without recording.
- A geometry constraint returning `Adjust` with a frame that cannot be applied —
  non-finite, or a negative extent — is refused rather than stored, and a debug
  build panics naming the frame. A `NaN` frame used to be kept verbatim, which
  removed the item from hit-testing, from the marquee and from every spatial
  query for good, with nothing raised to say so.
- `SceneModel::constrain_move` / `constrain_frame` called inside an open write
  scope panic naming the rule instead of reporting `RefCell already mutably
  borrowed`.
- **An `Over` item occludes a card only if it would act on a press.** A
  decorative halo over an embedded note takes nothing and focus stays in the
  note; a hover affordance is not a press claim, and neither is accepting a drop.
- **A marquee no longer takes screen-pinned items.** `commit_marquee` runs
  through `Scene::items_in_region`, which answers in scene coordinates, and a
  pinned item does not live there — it holds a fixed place in the viewport, so
  the scene rectangle a rubber band describes says nothing about whether the
  pointer crossed it. Dragging a band across the page therefore leaves a pinned
  badge alone where it used to select it. Selecting one deliberately still works
  by click. There is no screen-space region query yet; the deferral is recorded
  at the query's own site.
- **`SceneView::retention_margin(px)`** sets how far outside the viewport a
  heavyweight card stays mounted — the band that keeps a fling from mounting and
  unmounting cards under the finger. Default 96 screen px; read it back with
  `current_retention_margin`.

#### Platform

- `PlatformWindow::reconfigure_surface` answers `bool` rather than `()`:
  `false` means the display server is gone and the caller should wind down
  instead of asking for another frame, which would return to the same place.
- `FrameOutcome` gains `DisplayLost`, distinct from `Error` and
  `NeedsReconfigure` because it is terminal: reconfiguring or redrawing after
  it spins the loop.


### Fixed

#### Scene

- **A `SceneView` with selection or magnetism switched on can be panned with a
  finger.** The view's own drag was enrolled in a way that was never resolved, so
  its recogniser latched at the mouse slop and the pan was never evaluated.
- **A won pan no longer fires the claimant's own `on_tap`** — panning a list with
  a finger used to tap a row on the way past.
- **A revoked drag no longer leaves the item displaced.** The scene had no
  `PointerCancel` arm at all, so a cancelled touch or a lost capture left the
  item wherever the last sample put it.
- **Leaving a `SceneView` clears what leaving it should clear.** Its
  `PointerLeave` arm had never run, so an item stayed hovered for ever, the
  tooltip stayed armed, and the cursor stayed on `Grab` with the pointer outside
  the view.
- **Two `ItemFlags` do what their documentation says.** An item without
  `IS_VISIBLE` is no longer clickable or draggable, and one without `IS_ENABLED`
  no longer eats a click.
- **Paint order and hit order agree.** Equal-`z` ties were resolved to the
  bottom-most item, exactly inverting paint, and the scene's two pickers were
  wired to opposite halves of the dispatch pipeline — so a tap beat every card
  and a drag lost to every card.
- **A card's published rectangle tracks the camera.** A heavyweight item's
  accessibility bounds were its arena rectangle in scene coordinates while a
  lightweight item's were projected to the screen, so the two tiers disagreed and
  a pan, zoom or rotation moved neither. Both are published in scene space now,
  under one declared camera transform, and an accessibility hit-test at the
  painted position finds the card.
- **A selection change no pointer made** — "select all", a search result, or the
  other pane of a shared `SceneSelection` — reaches the published accessibility
  tree. Both panes kept describing the previous selection, at its previous
  position.
- **An item's height can be changed from assistive technology.** `Increment` on a
  "Resize top" or "Resize bottom" slider reported the action handled, announced
  an unchanged number and moved nothing.
- **`Esc` cancels a transform the pointer started**, not only one begun from the
  keyboard.
- **A revoked contact ends the gesture through the cancel path**: `on_end` fires
  with `TransformOutcome::Cancelled` and the edge auto-pan stops. The pan used to
  keep tweening for up to thirty seconds with no input, and the hook an app tears
  its overlay down on never fired.
- `TransformConfig::min_size` floors only the axes the dragged handle drives.
  Dragging the bottom edge of a 2 × 100 hairline used to widen it to 4.
- A geometry constraint that accepts a change leaves a magnetised drag alone on a
  **rotated** item; the comparison deciding whether the rule had overruled the
  magnet was exact across a rotation round-trip that is not bit-exact.
- A banded region — a lasso drawn as a stroke — is answered in the frame where
  its band is round, so an anisotropic scale no longer lets a containment query
  pick an item the matching shape query then rejects. A zero-length segment, and
  an open polyline's phantom closing chord, are both handled.

#### The minimap

- **It stays inside its own frame.** Zooming in projected the viewport rectangle
  outside the widget, and nothing clipped the result, so the minimap painted over
  its neighbours — a caller-supplied border by half its own width, unclamped. The
  projection now runs through the union of content and viewport, and the widget
  clips its own paint.
- Its content extent is computed in one place; paint and click used to carry
  separate copies of the same formula.

#### Rendering and canvas

- **A dashed or dotted stroke reaches the screen.** `stroke_rect`,
  `stroke_rounded_rect`, `stroke_circle`, `stroke_ellipse` and `draw_line` all
  accepted a `StrokeStyle` and dropped it, along with `line_cap` and `line_join`.
  A styled stroke now routes to the tier that carries the pattern, and a solid
  one keeps its cheaper tier.
- **The renderer no longer paints what hit-testing cannot see.** A path opening
  with an arc was rasterised with an injected `MoveTo` at the bitmap origin,
  drawing a long spoke from there to the shape — visibly painted, and a click
  inside it hit nothing. The atlas opens each implicit subpath where the
  flattener opens it.
- `Path::transformed` and `Path::flatten` agree about where a subpath begins.
  They did not for an arc following a `Close`, so rotating a two-loop lasso
  changed what it selected.

#### Input

- **Pen samples are spaced by the device's own clock.** Both pen backends stamped
  zero and the translator then stamped one instant across a whole drained batch,
  so velocity, smoothing and prediction were all computed from zero spacing. On
  Windows the batch was also genuinely one packet per message — the history
  buffer was never drained.
- **`PointerSample::coalesced` reaches a handler.** It was populated and then
  discarded, with no accessor and two constructors writing it out empty, so a
  tool written the obvious way drew at frame rate on a 200–360 Hz digitizer.

#### Platform

- **A compositor that exits no longer takes the application down as a crash.**
  When the display server goes away, the window surface stops answering for the
  adapter it was matched against, and the next `Surface::configure` failed
  inside wgpu's default error handler, which panics. What the user saw was
  `wgpu error: Validation Error / In Surface::configure / Surface does not
  support the adapter's queue family`, and what that reads like is a GPU
  mismatch. It is not one: `request_adapter` filters candidates on the very
  query that fails there, so the adapter had answered yes to the same question
  moments earlier. The message cost a real investigation, and a bug report
  aimed at the renderer for a compositor crash.
  Every `Surface::configure` now runs inside an error scope, so the failure is
  returned rather than thrown. A surface with no formats left for the adapter
  it was matched against is reported as a lost display server, in those words;
  anything else keeps wgpu's own message, so a genuine mistake is not
  relabelled as a dead compositor.
- The same event reaching winit first is no longer a panic either. Both Linux
  backends report a failed dispatch or flush as `ExitFailure(errno)`, and
  `run` treated every error alike. A lost display connection now exits
  cleanly with one line naming the cause; `NotSupported`, `Os` and
  `RecreationAttempt` stay fatal, since those are genuine startup faults where
  a backtrace is the useful answer.
- **Opening a window no longer kills a GNOME session on a machine with no
  touchscreen.** The Wayland drag-and-drop backend asked the seat for its
  pointer and its touch as soon as a window attached, on the reasoning that a
  seat lacking the capability would hand back a proxy that never emits. The
  protocol says otherwise: `wl_seat::get_touch` without the touch capability is
  a `missing_capability` error, and the client is the one at fault. Mutter
  (GNOME 50) does not answer with that error — it serves the request out of a
  `MetaWaylandTouch` whose list head is still all-zero, because the `wl_list`
  is initialised only when a touch device appears, and writes through it.
  gnome-shell takes SIGSEGV, and since the compositor *is* the session, every
  window on the desktop dies with it, a second or two after the app opened
  one. KWin is unaffected, which is what made it read as a compositor bug
  rather than ours. Both objects are now bound from the `wl_seat::capabilities`
  event instead, and released when a capability is withdrawn — the press
  serials `start_drag` needs arrive on the same terms as before, since a seat
  that can start a drag is by definition one that announced the capability.
  (This is the cause of the lost display server the two entries above learned
  to survive.)


### Known limitations

- **An assistive-technology probe and the pointer still disagree over an
  *interactive* scene overlay.** `accesskit_consumer` resolves a hit by walking
  a node's children in reverse, and the scene emits its children in band order,
  so the heavyweight tier wins an explore-by-touch probe whatever band the
  overlay is in and whatever it claims. The press-claiming rule below fixes the
  *pointer* answer, not this one. Closing it means reordering an AccessKit child
  list, which is also the reading order — an accessibility-tree decision rather
  than a hit-test one. Documented in `docs/teksilo-scene-a11y.md` and pinned by
  `what_the_press_claiming_rule_does_and_does_not_reconcile`, so it cannot drift
  silently.


## [0.11.0] - 2026-09-16

The `teksu!` DSL reaches the code applications are actually made of: a call to
your own `fn row(..) -> impl Widget` is a child, where before only a stock
widget was. Alongside it the container API loses its twin methods, so a slot has
one name and that name takes a `WidgetId` or a widget.

Underneath, the GPU floor drops to what the renderer actually needs. A machine
with no Vulkan driver could not open a window at all; it now falls back to
OpenGL, which is what an older GPU has.

### Added

#### teksu

- A lowercase identifier that continues into a call, a method chain or an index
  at body position is a child: `VStack { my_row(x) }` and
  `VStack { row(1).spacing(4.0) }` compile. A lowercase identifier standing
  alone is still the argument-free property.
- A keyword-rooted head is a child too: `self.row(x)`, `Self::header()`,
  `crate::ui::header()`, `super::row()`.
- `#{ expr }` carries a widget value as well as a `WidgetId`, which is how a
  Rust struct literal is passed at body position: `#{ Card { title: t } }`.

#### Widgets

- `on_change` on `Checkbox`, `Toggle`, `RadioButton` and `Slider`, taking the
  value the activation produced and an `EventContext`, so flipping one can send an intent, set the
  theme or open a window — things a bare `Signal` write cannot do, because an
  observer receives only `&T`. They fire for the pointer, for `Space`, for an
  assistive-technology `Click`, and, for a checkbox inside a data-view row, for
  `Space` on that row. They do **not** fire for programmatic writes to the bound
  signal: there is no event in flight to carry, and the signal remains the
  source of truth. A tristate checkbox reports a `bool` too, since activation
  cycles `Checked` ↔ `Unchecked` only. `RadioButton` reports only a real change,
  so re-activating the selected button is silent. `Slider` reports every value a
  drag produces, but not a write that changes nothing (a drag past the end, a
  snap onto the grid point already held); it has no commit-on-release callback,
  so once-per-interaction work still belongs on the signal.
- `StandardListItem::on_checkbox_toggle` and the `StandardTreeItem` forwarder,
  which make the embedded checkbox's `on_change` reachable — the canonical row
  builds its own checkbox, so it was the one place the callback could not be
  installed. To *read* check state, `CheckedModel` remains the answer: it is the
  source of truth and it survives row recycling, which a per-row callback does
  not.
- `child_opt` on every container that has `child`, 38 of them, up from 7. A
  bare `teksu!` `if` now works inside a single-child wrapper, not only inside a
  stack.
- 45 accumulator builders gained the plural twin that a `for` loop needs:
  `tabs`, `static_tabs`, `panes`, `items`, `actions`, `lines`, `rails`,
  `docks`, `radios`, `full_width_rows`, `add_children` and others.

#### Core

- **Behaviour change.** The row-activation marker a widget publishes with
  `BuildContext::set_keyboard_toggle`, and the fallback passed to
  `EventContext::row_space_activate`, are now `Rc<dyn Fn(&mut EventContext)>`
  rather than `Rc<dyn Fn()>`. `Space` on a data view's focused row therefore
  runs with a context, which is what lets a row checkbox fire `on_change` on
  that path instead of only under the pointer.
- `Box<W>` implements `Widget` for any `W: Widget + ?Sized`, so a boxed widget
  goes wherever a widget goes and adds no arena node.
- `HandlerSet::merge_under` composes two handler sets, the later declaration
  winning.

#### Documentation

- The `teksu!` reference leads with the real widget catalog: worked examples for
  `ListView`, `TreeView`, `TableView`, `TabWidget`, `FormLayout`, `MenuList`,
  `Toolbar`, `Switcher` and `DockingLayout`, and a section listing only what
  genuinely does not work, with the compiler's own text.
- The spec gains a **Why there is no v4** section: what the DSL was measured to
  cost, the four charges against it that measurement did not support, the three
  silent-wrong-program bugs and the one grammar rule that did stand, and the two
  exits that stay available if the question reopens. It replaces the standalone
  decision note, which is removed.
- The binding trap is written down in both documents: binding names are one flat
  namespace per block, so two bindings sharing a name alias, both attach sites
  resolving to the later widget while the earlier one is built and attached
  nowhere.
- A root `NOTICE` file records what the theme presets derive from and on what
  terms: the WinUI theme-resource values under Microsoft's MIT licence, the
  Material 3 baseline tokens from documentation Google publishes under CC-BY-4.0,
  the macOS preset's mix of published and measured numbers, and every bundled
  font with its licence, its copyright line, and whether it is embedded by
  default.
- The trademark policy gains a **Third-party trademarks** section: it names the
  owners of macOS, Fluent, Material Design and Int UI, states that Teksilo is
  neither affiliated with nor endorsed by any of them, and separates the
  `fluent` theme preset from Project Fluent, the unrelated localization system
  behind Teksilo's translations.
- The two bundled-font license files are corrected. `roboto-LICENSE.txt` carried
  a summary of Apache-2.0 -- four of its nine clauses, paraphrased, with the
  redistribution conditions collapsed to one line -- where section 4(a) requires
  a copy; it now carries the complete text. `noto-LICENSE.txt` named one project
  and one year for two faces; it now carries the copyright line each font's own
  name table declares, `notofonts/arabic` 2022 and `notofonts/hebrew` 2024.

### Changed

- CI runs `cargo teksilo-fmt --check` over `crates` and `examples`.
- `FormLayout::lines` takes `impl IntoTeksiChild` in both columns; the field
  column used to require `impl Widget`, so a loop holding ids could not use it.
- `teksu-language-spec-v3.md` is the design rationale and names
  `teksu-macro-reference.md` normative for behaviour; fourteen divergences from
  the implementation are corrected, and appendices A.2, A.3, A.4 and A.6 are
  marked superseded where they describe an API that has since been removed.

### Removed

**Behaviour change, breaking.**

- `StandardTreeItem::on_toggle` / `on_toggle_rc` are renamed
  `on_chevron_toggle` / `on_chevron_toggle_rc`. They are the expand / collapse
  control, and the name became ambiguous the moment a row gained
  `on_checkbox_toggle`.

- `add_child`, `child_id`, `child_boxed` and every `*_id` slot twin, 80 methods.
  Call the slot by its own name instead: `.child(id)`, `.content(id)`,
  `.header(id)`, `.pane(id)`. Every widget-accepting slot method takes
  `impl IntoTeksiChild`, which is implemented for `WidgetId` and for every
  `Widget`. Not affected: `shortcut_id`, `static_tab_with_id`, the alternate
  constructors (`from_id`, `new_id`, `around_id`, `custom_id`,
  `with_child_id`), and `Breadcrumb::item_id`.
- `WidgetBuilder::dim_when_inactive` and `dim_when_inactive_default`. Wrap
  instead: `DimWhenInactive::new().factor(f).child(w)`.
- `Checkbox::labels_hidden(bool)` is now `Checkbox::labelled_externally()`, the
  name and shape `Toggle` already used for the same thing: the control's
  accessible name comes from an ancestor, so it renders no label of its own.
- `FormLayout::line_ids` no longer adds one row from two ids; it takes an
  iterator of `(label_id, field_id)` pairs and adds a row per pair. The
  single-row form is gone because `line` takes `impl IntoTeksiChild` and accepts
  a `WidgetId` directly.

### Fixed

#### Platform

- **Teksilo opens a window on a machine with no Vulkan driver.** OpenGL is the
  only backend such a machine has left — an older GPU, a VM whose guest driver
  stops at GL — and it was never handed the platform's display connection, so it
  could render offscreen but never present to a window. Startup died before the
  first frame with `incompatible_surface_backends: GL`. Confirmed fixed on both
  Wayland and X11 with the Vulkan driver removed.
- Adapter selection is a search rather than a single request. An adapter that
  enumerates but cannot open a device no longer ends the process, and an
  explicit software fallback is tried before giving up — the resilience the
  offscreen test device already had, and the window path did not.
- wgpu's own environment variables take effect: `WGPU_BACKEND`,
  `WGPU_POWER_PREF` and the rest were silently ignored, leaving no way to move
  off a backend whose driver is the problem.
- The remaining "no GPU" failure names the backends tried, the errors each gave,
  and what to install, in place of a `Debug`-printed wgpu struct.

#### Rendering

- Neither atlas asks for a texture the device cannot allocate. Both grow toward
  a 4096-pixel ceiling that is this renderer's own, not a fact about the
  hardware, and a downlevel device can sit below it; the path atlas now caps its
  growth to what the device reports, and a glyph atlas that arrives oversized is
  skipped rather than failing the frame.

#### Core

- **Behaviour change.** A builder method with no inherent twin on
  `WidgetWithHandlers` wrapped an already-wrapped widget, and only the outer
  handler set reached the node, so `.on_tap(cb).clips_children_on(true)` never
  fired. Handler sets at any depth now arrive.
- Chaining past `dim_when_inactive` retargeted the rest of the chain at the
  wrapper, so `VStack::new().dim_when_inactive(0.7).child(a).child(b)` built
  `DimWhenInactive > b` and lost the stack and `a`. The method is gone;
  `teksilo-teksu-guard` fails the build if a `WidgetBuilder` method returns a
  foreign wrapper again.
- **ArrowLeft reaches a text editor again when a second one is mounted beside
  it.** The dispatch root reads the inline-start arrow as "back toward the
  parent overlay" once two or more non-host overlays are stacked, and it counted
  every band alike. Each mounted `RichTextEditor` keeps one full-viewport
  affordance host in the `TextAffordance` band for its selection handles, so two
  editors on one page — a prose column and the synopsis next to it — read as a
  submenu over its parent menu: the key tore down an affordance host and
  returned, and no editor in that window saw an ArrowLeft for as long as both
  were up. The count now considers `OverlayBand::Standard` overlays only, the
  same line `OverlayBand::dismissed_by_outside_press` already draws for presses.
  A submenu over a mounted affordance still closes on the back key.

#### teksu

- `teksilo-fmt` re-indented a multi-line expression child by the block's own
  indent on every run, walking a multi-line argument list further right each
  time.


## [0.10.0] - 2026-09-14

One strand above all: the input model is a pointer model. A touchscreen, a pen
and a trackpad reach the widget tree as real pointers, a density knob resizes
every target with a 24 dp conformance floor at each density, and a mouse behaves
exactly as it always has. Around it: touch and a stylus in the automation
bridge, a target-size gate over every shipped preset, a previewer that exports
at a chosen density, every SVG icon drawn pixel-exact, and a window that opens
on GLES-3.1 class hardware. The rich text editor takes text-document 1.12.2,
so a sentence cut at a paragraph's end is the sentence and nothing more.
Breaking, pre-1.0: the pointer-event variants gained fields, two positions were
renamed, `WebViewHandle` gained a required method and `MemoryShared` is
non-exhaustive;
[docs/porting-widgets-to-the-pointer-model.md](docs/porting-widgets-to-the-pointer-model.md)
is the contract for a widget crossing over.

### Added

#### Touch, pen and density

The framework's input model was a mouse: one pointer, always hovering, always
precise, always present. It is now a pointer model, and a mouse behaves exactly as
it always has.

- **Touch input, end to end.** A touchscreen's contacts reach widgets as real
  pointers with their own identities, and a finger scrolls, selects, reorders,
  edits text, pinches, drags to and from the OS, and reaches a 24 dp target. Each
  contact gets a fresh identity per press, so an OS that reuses slot numbers cannot
  make two gestures look like one, and up to a documented cap of simultaneous
  contacts are tracked at once.
- **Pen and stylus.** Pressure, tilt, barrel rotation, the eraser end, the barrel
  button, and proximity as a first-class state — a stylus hovers before it touches.
  Wayland through `zwp_tablet_v2`, Windows through a `WM_POINTER*` window subclass;
  no winit upgrade required.
- **Trackpad gestures.** Pinch, rotate and two-finger scroll reach the widget tree.
- **`TargetDensity` — Compact, Comfortable and Touch.** One knob resizes every
  interactive target, gap and padding in the application: 24 / 32 / 44 dp targets,
  6 / 10 / 16 dp grab handles, spacing ×1.00 / 1.15 / 1.30. Compact is the default
  and is today's layout unchanged. Set it with `Theme::with_density` or
  `WidgetTree::set_input_density`; the shipped Material 3, Fluent and macOS presets
  project their own dimensions onto the ladder rather than inheriting Teksilo's.
- **A 24 dp conformance floor at every density, never scaled** — WCAG 2.2 SC 2.5.8
  (level AA). A build-breaking test measures every named widget fixture against it
  at all three densities.
- **Three hit mechanisms with disjoint jobs**, so a small control is reachable
  without being redrawn: `Widget::hit_outset` widens a target for one pointer kind
  only, `Widget::target_regions` lets a self-painting widget say where its parts
  are, and a miss-only slop pass re-attributes a near miss for a coarse pointer.
  None of them moves a painted pixel.
- **Kinetic scrolling.** A fling coasts and settles on the platform's own curve —
  Android's `OverScroller` or iOS's bouncing simulation, chosen per host by default
  — with rubber-band overscroll available per surface. Nine scrollable surfaces
  adopt it from one implementation.
- **Touch text editing** in every editing surface: the caret lands on the release,
  a hold selects the word under the finger, two draggable handles adjust the range,
  a magnifier shows the text under the handle, and a selection toolbar offers the
  clipboard. `RichTextEditor`, `CodeEditor`, `PlainTextEditor`, `LogView`,
  `TextInput`, `PasswordField`, `SearchField`, `SpinBox` and the date/time family.
- **A hold is a route.** Where a widget installs no long-press handler of its own,
  the tree resolves a hold to the node's context menu, else its tooltip, else
  nothing — so a finger reaches both affordances that were previously mouse-only.
  `LongPressRole` overrides the choice per subtree.
- **`touch_action` and `pan_claim` node declarations**, the CSS `touch-action`
  model: a subtree can forbid panning or pinching over itself, and a scrollable
  declares which axes it claims. A press freezes the effective value, so nothing
  a widget does mid-gesture can change what that gesture was allowed to become.
- **A runtime kill switch.** With `InputTokens::touch_enabled` off, the platform
  translator drops touch input and the router installs no touch-only recognizers —
  a mouse-only fallback with no rebuild of the binary.
- **Input tracing.** `TEKSILO_TRACE_INPUT=samples|gestures|all` prints every
  sample, every arbitration decision and every cancellation, guarded so that a
  normal run formats nothing.
- **`examples/touch_playground`** — a live pointer inspector (identity, kind,
  primary flag, pressure, tilt, twist, contact patch, speed, the frozen touch
  action, the press and the capture), five arbitration scenarios each naming which
  contender won the press, a density toggle, an editable kinetic-constants panel,
  and a touch-text surface.
- **A Touch tab in `examples/widget_catalog`**, and `--density compact|comfortable|touch`
  on the catalog itself.
- **[docs/touch-verification.md](docs/touch-verification.md)** — the hardware
  procedure, and [its sign-off sheet](docs/touch-verification-signoff.md).

#### Automation

- **An agent can drive touch, a stylus and two contacts.** New operations:
  a whole multi-touch sequence in one call (reporting the arbitration after every
  step), a two-finger pinch, a fling described in simulated time, a long press
  held for the active profile's own threshold, a pointer cancelled the way the
  system cancels one, a query of every live pointer with its capture and
  arbitration state, and a density switch. `inject_pointer`, `inject_key` and
  `type_text` take a `command` modifier beside `ctrl`, because a chord declared
  `Ctrl+S` resolves to ⌘S on macOS.

#### Accessibility

- **The target-size gate measures every shipped preset**, not the default theme
  alone: Int UI, macOS, Fluent and Material 3, each at all three densities, with
  light and dark compared for identical geometry. The fixtures moved into a
  crate the theme crates can reach, so a preset is audited by the same 68
  fixtures the default theme is.

#### Previewer

- **`--export-docs` renders at a chosen density**: `--density=compact,touch`
  writes `docs/widgets/img/<slug>-touch.png` beside the canonical image, and a
  catalog page grows a `## Density` section when one exists. Compact stays
  canonical and keeps every existing filename, so nothing committed moves.
- `teksilo_preview::PreviewPass` — the density a preview renders at, and the
  image-naming rule that follows from it.

#### Documentation

- **`docs/porting-widgets-to-the-pointer-model.md`** — the contract for moving a
  widget onto the pointer model, as numbered clauses, each stating the rule, how
  to comply, and what checks it.
- Documented constants are asserted against the pages that document them. The
  density ladder, all three gesture profiles and the kinetic constants are read
  back from their own tables by a test, so a number lives in one place.

### Changed

- `text-document` 1.12.2 and `text-typeset` 1.11.1.

#### Core

**The pointer-model behaviour changes.** Each is a token or a knob an application
can override, and none of them changes what a mouse does.

- **A mouse drag never pans.** Dragging the content of a scrollable surface
  scrolls it only for a touch or a stylus; a mouse scrolls with its wheel, exactly
  as before. Widen it per surface with `ScrollHandlingOptions::pan_devices`.
- **A touch or pen drag of an *item* waits for a long press.** Reordering a list
  row, a grid tile, a tab or a table column with a finger means holding it first,
  because the surface underneath has first refusal on a direct pointer's press. A
  mouse still drags from the first few pixels. `DragActivation` on the node decides;
  `Auto` is the default and resolves per pointer kind.
- **A touch or pen press commits on the release.** Selection, activation and caret
  placement happen when the contact lifts, not when it lands — so a press that
  slides off its target commits nothing. A mouse commits on the press, unchanged.
- **A press outside an overlay dismisses it on the release, and the dismissing
  press does not reach what is under it.** Dismissing a menu and activating the
  control beneath it are now two presses, on every pointer kind.
- **`on_hover` never fires for a touch contact,** and neither do `hover_within`
  and the hovered signals. A contact does not hover — that is what a contact is —
  so an affordance revealed only on hover is unreachable with a finger.
  `InputTokens::reveal` promotes those affordances to always-visible at the Touch
  density, and the tree's hold route reaches a tooltip regardless.
- **A pen in proximity hovers, and hover belongs to one pointer.** On a machine
  with both a mouse and a stylus, each one's hover follows its own device instead
  of a single shared "hovered" notion.
- **`advance_time` ticks gesture recognizers.** A test that advances the virtual
  clock now sees a long press ripen, a fling coast and a hold dispatch; the clock
  is one axis for animation, gestures and kinetics rather than three.
- **`WidgetEvent::PointerDown`, `PointerUp`, `PointerMove`, `PointerEnter` and
  `PointerLeave` now carry `pointer: PointerInfo`**, so a handler can tell a
  finger from a stylus from a mouse without reaching for the context.
  `PointerEnter` and `PointerLeave` become struct variants; a pattern that named
  them bare now needs `{ .. }`.
- **`PointerMove` also carries `modifiers: Modifiers`.** A drag reads Shift and
  Ctrl from the move rather than from the press, so a modifier pressed mid-drag
  reaches the widget.
- New constructors keep a mouse-describing call site to one line and default the
  pointer to `PointerInfo::mouse` at the epoch: `WidgetEvent::pointer_move_with`,
  `pointer_enter`, `pointer_leave`, beside the existing `pointer_down`,
  `pointer_up`, `pointer_move`.
- **`WidgetEvent::Scroll::position` and `PointerCancel::position` are renamed
  `window_position`.** Both stay in window-logical coordinates — the router
  routes by the first, and the kinetic tracker behind a pan follows the pointer
  rather than the widget — and they are the only positional fields a handler
  receives that are not localised to it. The frame is now in the name, so a
  reader that needs content coordinates is told to convert at every use.
- **`GestureEvent::PinchChanged` carries deltas, and says so.** `scale` is the
  factor since the *previous* sample and `rotation` the twist since the previous
  sample, in radians; fold each in rather than assigning it.
- **A drag handler is told which device carries the drag at every stage**,
  including the per-layout tick. Those previously reported the mouse, so a finger
  never got the wider auto-scroll band.
- **A coarse pointer's drag preview is placed clear of the contact patch**; a
  mouse's is unchanged.
- **The widget under an inbound OS drag revises the OS accept state**, so the
  cursor no longer promises a drop the target refuses. An OS drag aborted over a
  second window of the same application tears that window's session down, and a
  drag exported from a finger on Wayland uses that finger's press serial.
- **`WindowOps::begin_os_drag` and `ExternalDndGuard::begin_drag` take the
  device's `PointerKind`**, and both gain `set_drop_accepted`.
  `ExternalDragEvent` gains a `Cancelled` variant.
- **`TouchSelection::on_long_press` takes a `pointer: PointerInfo`.** A hold is
  recognised by a timer, so the context it is dispatched under had no device to
  report; the holding contact's identity is now an argument.
- **`WebViewHandle::set_input_passthrough` is a new required trait method**, for
  out-of-tree backends. `WebViewEvent::EngineFocusChanged` is new.
- **`MemoryShared` is `#[non_exhaustive]`** and gained four fields.
- `InspectorState::toggle()` is public. New:
  `teksilo_widgets::drop_target::{band_depth, region_at_floored, region_rect_floored}`,
  `styles::recipe_menu_item_style::menu_item_height`.

#### Widgets

- **Nine surfaces pan under a finger** from one implementation: `ScrollArea`, the
  five data views and the three text surfaces, plus `Terminal` and `SceneView`,
  which claim their own axes.
- **The window chrome answers a finger.** A resize strip stays 6 dp of paint and
  reaches the density's target size for a coarse pointer; a hold on the title bar
  asks the OS for its window menu where the platform has one.
- **A `SplitButton` chevron takes a direct pointer's press up to the conformance
  floor**, borrowing from the action half; a mouse's boundary stays where it is
  painted.
- **A `TableView` header cell's filter zone is the density's target size**, and
  the cell reports its three parts through `Widget::target_regions`.
- **A `DropTarget`'s edge zones — and a docking pane's five — are floored per
  axis and capped at a third of the extent.** Not pointer-kind-gated: the acting
  zone and the painted one must be one rectangle.
- **List and tree row minimum heights, combo-box dropdown rows, calendar
  navigation arrows, menu rows and code-editor completion rows follow the
  density ladder.**
- **A finger's tap on a `ToolBox` header or a `RadioTile` leaves it untinted**
  rather than resting hovered.
- **A toast pauses on a press-and-hold and dismisses on a horizontal swipe** for a
  coarse pointer, because hover-to-pause is unreachable with a finger.
- **Every reorder a drag can do, a keyboard and a menu can do too** — WCAG 2.5.7.
- **`WebView` in the default `Native` input mode** answers a press itself and
  revokes the pointer's interaction, declares no touch default over its rectangle,
  and opts out of hit widening; `Transparent` asks the engine to stop taking
  input. Its subview is mirrored at the intersection with every clipping ancestor,
  hidden when out of view, and stood down while an interactive overlay covers it.
  Engine focus moves the toolkit's focus onto the frame.
- **The debug inspector opens from `InspectorState::toggle()` or a corner-grip
  hold**, its pressed rows take the density's target size, and it has a Pointers
  tab listing every pointer declaration in the application's tree.
- **The widget previewer takes `--density`, switches it live, and its canvas zoom
  works** (pinch and Ctrl-wheel).

#### Terminal

- **A finger pans the scrollback with a kinetic hand-off**, quantised to lines so
  a sub-line sample is banked rather than dropped. Double- and triple-tap select
  the word and the line, a hold opens the terminal's own menu, and selection
  handles snap to cell boundaries.
- **A finger is never reported to the child program as a mouse.** Touch reporting
  is its own policy, separate from the mouse reporting a program requests.

### Fixed

#### Rendering

- **CPU-rasterized paths, which is every SVG icon, draw pixel-exact.** A Tier-3
  path's coverage mask is rasterized on its own integer grid, but its quad was
  placed at `bounds × scale_factor` and sized to `ceil` of that — so the quad sat
  at a fractional device position, and at a fractional display scale it was not
  even the same size as its atlas region.
  Sampled through the atlas's linear filter, both errors smear. A 1 px hairline
  drawn this way peaked at **48 % coverage instead of 100 %**, and a 16 dp dashed
  ring lost its gaps entirely and read as a grey haze. The path pipeline now snaps
  the quad out to whole device pixels and bakes the bitmap against that same
  origin, so one texel lands on one pixel — the guarantee
  `QuadVertex::from_glyph_quad_transformed` has always given glyphs, which is why
  text was sharp and icons were not. The rect now travels with the raster as one
  `PathPlacement`, so the two can no longer be derived apart. Snapping is skipped
  under a transform, where the mask is being resampled anyway and rounding would
  make a translating path step between pixels instead of gliding; a gradient's
  geometry is re-based onto the snapped quad, so it lands in the same place either
  way.
- **Two path-atlas entries no longer share an edge.** The shelf packer placed them
  flush, so an edge fragment of any quad that is not pixel-exact on its region
  read a texel belonging to the *next icon* rather than transparent space. Each
  entry now reserves a one-texel transparent gutter, matching the glyph atlas.

#### Core

- **A two-finger pinch reaches the zoom it asked for instead of running into
  `max_zoom`.** A spread to twice the starting span now leaves a `SceneView` at
  exactly twice, whatever the sample rate; before, each sample carried the ratio
  to the *start* of the gesture and the handler multiplied every one of them in,
  so a single spread compounded to the product of its intermediate ratios.
- **A trackpad twist turns content by the angle the user twisted.** One degree of
  rotation on the trackpad rotated a `SceneView` by one radian — about 57° —
  because winit reports degrees and the payload is read as radians. The
  conversion now happens where the incoming unit is known.
- `GestureEvent::PinchChanged` and `PinchPhase::Changed` now document `scale` and
  `rotation`: both are deltas against the previous sample, and `rotation` is in
  radians. Fold each sample in (`zoom *= scale`, `rotation += rotation`) rather
  than assigning it.
- **`TouchPinchRecognizer::scale` and `rotation` are renamed
  `cumulative_scale` and `cumulative_rotation`.** They report the totals since
  the gesture started, which is not what a `PinchChanged` carries; the names now
  say which of the two a caller is reading.
- **A `PointerCancel` for a touch contact reported the mouse, with no position**,
  so an application branching on the cancelled pointer's kind behaved wrongly.
- **A container that preserves its children across a rebuild destroyed and rebuilt
  its whole subtree instead**, whenever any builder method wrapped it. That covers
  every `Switcher`, `TabWidget`, `SceneView`, `DockingLayout`, `Repeater` and
  `PopoverWidget` with a handler attached.
- **A wrapped widget's declared shortcuts never reached the registry**, so they
  were missing from the rebinding UI as well as from the keyboard; a wrapped widget
  that opts out of layout memoisation was memoised anyway; a wrapped title bar
  stopped publishing its OS caption regions — a defect its own documentation had
  written up as a rule. Wrapped dialog content stopped lending its title to its
  shell and stopped directing initial focus; the context-menu key opened the
  view's menu rather than the selected row's; a table read its body to assistive
  technology before its header; a scene stopped grafting its items into the
  application's accessibility tree; empty tooltip content still raised a bubble.
- **A coarser density could lower a grip's reach**: an outset and the miss-only
  slop pass were combined in the wrong order.
- **A caret could be left outside a viewport that shrank**; a shrink now records a
  reveal.
- **Two overlay-dismissal defects a mouse could feel**, and a hold dispatched
  under a mouse identity that was never there.
- **A target-size allow-list entry is held to the floor the audit judged against,
  not to the generic 24 dp table.** Under a theme raising
  `InputTokens::min_target_conformance`, a `PinnedDp::ClearsFloor` axis excused
  the very failures the raised floor exists to report.
  `TargetMeasurement`/`TargetViolation` carry the measured floor as
  `conformance_floor`, and every consumer reads it instead of re-deriving one.

#### Platform

- **Teksilo could not open a window at all on GLES-3.1 class hardware.** A
  window asked its device for `wgpu::Limits::default()`, which demands eight
  colour attachments. A Raspberry Pi 4's V3D driver allows four, so device
  creation was refused and the application panicked before its first window
  existed. Nothing in the renderer wanted that headroom: every render pass has
  one colour attachment, it binds no storage buffers, its widest uniform
  binding is 8 KiB and its widest shader carries ten inter-stage variables. A
  window now asks for `downlevel_defaults` with the texture-dimension limits
  lifted to the adapter's own, which is the set the offscreen test renderer
  already opens with, so a frame that renders in a test renders in a window.
  An adapter sitting below even that floor gets one retry with its own reported
  limits, which cannot be refused on limit grounds.

#### Widgets

- **A Fluent list or tree row can be clicked again.** The preset's selection pill
  spans the whole row and was hit-tested ahead of the row's contents, so nothing
  inside a Fluent row — a checkbox, a disclosure arrow, a trailing button — took
  a press, by mouse or by finger.
- **A macOS switch and an icon button meet the 24 dp target floor.** Both are
  drawn at the size the preset asks for, Apple's 22 dp track and the 18 dp
  compact icon button among them; the node around the chrome is what grew.
- **Three controls showed no pressed appearance for a pointer at all** — the
  `ToolBox` header, `RadioTile`, and the calendar's month/year cell, the last only
  ever writing `false` into its own state.
- **A declared row height inside the row-metrics dead band was silently refused**,
  so an exact-height list reported a different total height from a uniform one.
- **A leaf row in a tree table no longer carries an invisible pointer target.**
- **The previewer's knob rows were unreachable by any pointer, mouse included**: an
  infinite height cap inside a scroll area measured `inf` tall, its `y` resolved to
  `NaN`, and a rectangle containing `NaN` contains no point.
- **With the debug inspector installed, switching density destroyed the wrapped
  application's tree.**
- **Four dimensions a shipped theme sets now reach the screen.** A table header
  cell pads by the Fluent gutter (12 dp, not 8); a calendar's navigation arrows
  are drawn the size macOS asks for (20 dp, not 24); a search field's suggestion
  rows carry the macOS row gutters (8 x 3 dp, not 10 x 4). Each was written on
  the theme's recipe and discarded, because the widget measured the shipped
  Int UI constant instead of asking the style. `TableStyle`, `CalendarStyle` and
  `SearchFieldStyle` now hand the dimension over, and a custom style keeps the
  ladder it had unless it says otherwise. Int UI at the default density is
  unchanged.
- **A theme may draw a control below the 24 dp target floor without costing the
  app its WCAG 2.2 SC 2.5.8 conformance.** A calendar's navigation arrow keeps
  whatever size the theme asks for and sits inside a box that reaches the floor,
  so macOS's 20 dp stepper is drawn at 20 dp and still answers a 24 dp press —
  from a mouse as much as from a finger. Under Int UI, whose arrow is already
  the floor, nothing moves at any density.

#### Terminal

- **A terminal placed anywhere but the window's top-left corner accepted no
  pointer input at all.**
- **The wheel scrolled the scrollback backwards**, and its report to the child
  program named cell (0, 0) instead of the cell under the pointer — so a
  full-screen program that splits its window scrolled the wrong pane.
- **Dragging a selection handle moved the selection to the neighbouring row.**

#### Previewer

- **The toolbar's Export PNG ignored the live density**, so exporting while the
  previewer was set to Touch produced a Compact image — over the Compact
  filename. The density now reaches both the render and the name.

#### Text

- **A sentence cut at a paragraph's end is the sentence, and bold lands on the
  selection.** From text-document 1.12.2. Once anything had been typed earlier
  in the document since the last deletion, a use case addressing a range read a
  block start the keystrokes had left behind, so Cut and Copy at a paragraph's
  end took the paragraph break and the head of the next paragraph along with
  the sentence, and bold, italic, replace and "make a list" landed the same
  number of characters late. In a document holding a table the caret now also
  reaches the last characters after it, and a selection to End includes them.
- **`--all-features` builds.** Five `fonts-*` features named a Noto face that is
  not in the repository, and because `include_bytes!` resolves at compile time,
  enabling one was a hard build error — so those five features, both
  `fonts-all` meta-features, and any `--all-features` build of the workspace
  had never compiled on any revision. A face is now embedded only if its file
  is present; enabling a feature without it warns and names the path to drop it
  at, rather than failing the build.
- **Dragging the caret handle moves the on-screen keyboard's candidate window
  with it.** Every editing surface reported the caret's position when a press
  placed it, and none did when a finger *dragged* it, so composing Japanese,
  Chinese or Korean after a handle drag put the candidate list at the caret's
  old position. `RichTextEditor`, `CodeEditor`, `PlainTextEditor`, `TextInput`,
  `PasswordField`, `SearchField`, `SpinBox` and the date/time family; the
  assistive-technology route onto the same handle reports it too.
- **The code editor's gutter returned from inside a clip scope when no text
  backend was installed**, leaving the render frame unbalanced.
- **A `PlainTextEditor` could neither replace nor suppress the context menu its
  documentation offered it**, and the `CodeEditor` family gained the right-click
  menu it never had.
- **`--all-features` builds.** Five `fonts-*` features named a Noto face that is
  not in the repository, and because `include_bytes!` resolves at compile time,
  enabling one was a hard build error — so those five features, both
  `fonts-all` meta-features, and any `--all-features` build of the workspace
  had never compiled on any revision. A face is now embedded only if its file
  is present; enabling a feature without it warns and names the path to drop it
  at, rather than failing the build.

#### Documentation

- **A `ScrollArea` in the `Thin` scroll-bar mode documented a keyboard route it
  does not have.** The bar's arrow / `Home` / `End` / `Page` handlers sit on a node
  that cannot take focus, under every mode — the same limit the
  `ScrollBarPolicy::AlwaysOff` documentation states from the other side.
- **`PointerInfo::primary`'s documentation described the wrong one of three
  similarly-named things.** The field is the W3C *per-kind* flag — a mouse and a
  first finger are both primary at once on a hybrid machine — and the rule it
  carried ("exactly one live pointer, a mouse always wins") belongs to the pointer
  table's own election.
- **`StandardListItem`'s documentation claimed the row reads its recipe's
  projected heights.** It projects the module constants instead; those two recipe
  fields have no reader.
- **The soft-keyboard documentation claimed a touch-placed caret leaves a stale
  IME area standing.** Each editing stack reports the area from its own touch path;
  the unused core method is superseded rather than missing.
- **Two committed catalog images had gone stale**, one of them contradicting a
  safety fix: the message-box preview showed **Yes** as the accented default
  after the widget had switched to **No**, and the colour-picker preview predated
  the widget growing its HSV row.
- **Catalog pages linked to rustdoc module pages that do not exist on docs.rs**,
  because the module is crate-internal; each link now resolves to the nearest
  published module.

#### teksu

- **`teksu!` bodies could not place `gesture_dead_zone` or `keyboard_capture`
  before a child**; both failed with "no method named `child`". A compile-time
  guard now keeps the DSL's builder-method list complete.
- **31 `teksu!` UI fixtures had never run** — a glob naming a directory that does
  not exist, which trybuild reports as "no tests enabled yet" and then passes —
  and one of them was asserting a caret column rustc does not emit.
- **A bare child in any popover produced the generic error** instead of the slot
  hint.

### Known limitations

- **The hardware sign-off is not done.** Everything above that a headless Linux
  host can check is checked by the suite;
  [docs/touch-verification.md](docs/touch-verification.md) is the procedure for
  the rest, and its [sign-off sheet](docs/touch-verification-signoff.md) is empty.
  One question in it can only be answered on a macOS trackpad: whether a positive
  trackpad rotation delta should be negated at the platform seam.
- **A running application does not switch density because a finger arrived.**
  `DensityPolicy::FollowLastPointer` has an ingress and no writer; what a stray
  tap should cost and when hysteresis commits are unanswered.
- **Overscroll is published but not painted.** `ScrollableAxes::overscroll`
  carries the value; nothing renders a stretch or a glow.
- **A plain `ScrollArea` has no keyboard scroll route.** Assistive technology can
  scroll it and the data views bring their own key handling, but a keyboard user
  facing a scroll region whose content holds no focus has none — a pre-existing
  WCAG 2.1.1 gap, now written down.
- **A finger cannot drag a window on Wayland.** No protocol for it exists.
- **The target-size gate is green for three crates' named fixtures**, not for the
  framework: four crates that own targets have no fixture list, and the lists
  install one preset.
- **Reordering a data-view row with a finger is unreliable on short rows.** A
  deferred drag is revoked if the first sample after the hold leaves the pressed
  row, and the touch drag slop is wider than a default tree row.
- **`cargo check --workspace --all-features` does not build on any revision**,
  including before this release: five of the seven optional fallback font faces
  are named by the build and were never committed.
- The full ledger, each entry with its measurement, is
  [Touch & pen](docs/touch-and-pen.md) §10.

## [0.9.5] - 2026-09-11

Three main strands: text ranges on every visible label, one chord table for
the bounded-scalar controls, and a submenu that survives the diagonal to
reach it. Plus `stretch_last_column` on the tables, an assistive-technology
whole-value write that commits, and a `FormLayout` that keeps its rows
across a rebuild. One breaking change: `TextInputField::on_access_set_value`
now returns whether the host accepted the string.

### Added

#### Bounded-scalar controls

See [docs/range-keyboard.md](docs/range-keyboard.md) for the full chord table.

- `common::range_nav`: the arrow, page and edge chords of a control that holds
  one bounded number, as pure functions over
  `(key, modifiers, kind, axis, direction)`. `Slider`, `SpinBox`, `ScrollBar`,
  the colour picker's hue and alpha strips, the `Splitter` handle and the dock
  resize handle each hand-rolled the same eight-key match and gave four
  different answers for it. Like `list_nav`, it takes no platform convention:
  Qt, GTK4, the Win32 trackbar and `<input type=range>` bind these keys
  identically on all three desktops.
- `PageUp` / `PageDown` in `Slider`, with `Slider::page_step` to size the jump.
  Unset it defaults to ten times the effective `step`, so with the default step
  of 1 % of the range a page is 10 % of it — WebKit's and Blink's rule for
  `<input type=range>`, `QAbstractSlider::pageStep`, `GtkScale`'s page
  increment, and the same `10 x` rule `SpinBox::page_step` already used. The
  slider was the only one of Teksilo's bounded-scalar widgets with no coarse
  step at all.
- `Slider` publishes `numeric_value_jump` and services `Action::SetValue`,
  taking either an `ActionData::NumericValue` or a numeric string and snapping
  to `step` exactly as a drag does. AT-SPI publishes the `Value` interface off
  `numeric_value` alone, so Orca's `Value.SetCurrentValue` already reached the
  widget and was dropped; macOS gates `setAccessibilityValue:` settability on
  the advertisement, which the node now carries.

#### Widgets

- **`Alt+ArrowDown` opens the popup and `Alt+ArrowUp` closes it** on
  `ComboBox`, `DateEdit` and every `PopoverButton` / `PopoverIconButton` — so
  `ColorEdit`, whose documentation has promised the chord since it was written.
  The Win32 / WinForms / WPF drop-down chord, the Win32 `DateTimePicker` chord,
  and the W3C ARIA combobox pattern: "displays the popup without moving focus",
  which is why the modified form exists beside a bare `ArrowDown` that both
  opens and advances.
- `TextInputField::on_access_set_value` (forwarded by `TextInput`): handle an
  assistive technology's whole-value write, given the string it set. Unset for
  a field whose bound `Signal<String>` **is** the value — the write has already
  landed and there is nothing to derive. `SpinBox` and the four date and time
  editors install one, because their text is only a projection of a typed
  value. The string is handed over rather than read back from the signal
  because the field defers its document→signal sync to the next frame tick, so
  a host reading the signal there would parse the text from *before* the edit.
- **`F4` toggles the popup** on the drop-down *fields* — `ComboBox` and
  `DateEdit` — the Win32 / Qt / WPF chord. Deliberately not on `PopoverWidget`,
  which also backs toolbar chevrons and menu buttons and carries no such
  convention. An app that registers `F4` as a `Shortcut` keeps it: shortcuts
  resolve before the focused widget sees the key, and `Alt+F4` is unaffected.

#### Accessibility

- **Every visible label that owns its own accessible name is reviewable by
  character, word and line**, routable on a braille display and trackable by a
  screen magnifier. `TextWidget` — the building block behind every label in the
  framework — now carries AccessKit text runs with real per-character extents,
  so a screen reader's review cursor can walk a paragraph, a braille cell can
  be routed to the word under it, and `AXBoundsForRange` / UIA's `TextPattern`
  / AT-SPI's `GetCharacterExtents` all answer. Before, a label was one
  unreviewable chunk; only the editors and the terminal exposed ranges, and
  even those exposed no geometry.
- `TextWidget::geometry_handle`, for a control that owns its name but paints
  its text through a hidden label. `Badge`, `GroupHeader` and the scene's
  `TextItem` use it and are now reviewable too.
- `teksilo_core::accessibility::text_runs`: the shared emitter every text
  surface goes through, and `teksilo_core::accessibility::audit`, which reports
  labels that repeat an ancestor's name, labels with no text ranges, and runs
  that disagree with the node they hang off.
- `DocumentFlow::block_line_geometry` and the `*_with_geometry` layout methods
  in text-typeset 1.11, which is where the per-character extents come from.

#### Data views

- `TableView::stretch_last_column` / `TreeTableView::stretch_last_column`:
  the last column in display order takes the width the other columns leave
  (Qt's `stretchLastSection`). Positional — it follows a reorder — and the
  stretched column has no grip of its own.

#### Core

- `EventContext::arm_overlay_safe_region` / `overlay_safe_region_armed`: an
  open overlay can claim a "safe triangle" from the point the pointer left its
  anchor to its own near edge. While the pointer is inside it, the overlay's
  pointer-leave grace is held off; leaving it starts that grace and coming back
  cancels it. Any widget whose hover would tear the overlay down asks whether a
  traversal is under way at all and stands aside for as long as one is, leaving
  the dismissal to that one re-evaluated grace. Bounded to 600 ms so it stays a
  travel allowance. Used by cascading submenus.
- `EventContext::show_overlay_after_replacing_siblings`: like
  `show_overlay_after_with_focus`, but the anchor's sibling overlays are
  dismissed when the overlay actually shows rather than when it was requested
  — the hover-switch that must not fire for a pointer merely passing through.

#### Automation

- `teksilo_automation::client`: the binary name, the matching version and the
  `cargo install` command for `teksilo-automation-mcp`, plus a `$PATH` lookup
  for it. Pure `std`; nothing spawns a process, because the only caller is on an
  app's startup path.

### Changed

#### Widgets

- `TextInputField::on_access_set_value` (and `TextInput`'s forwarder) now takes
  a callback returning `bool` — whether the host accepted the string — so the
  field can report a refused write to the assistive technology instead of
  claiming success. **Breaking.**
- `ScrollBarStyle::make_body` documents its right-to-left obligation, the way
  `SliderStyle::make_body` does: a horizontal bar's `scroll_ratio == 0.0` is the
  start of the content, which is the right-hand edge there, and the widget
  cannot enforce that a custom style mirrors.

#### Accessibility

- Dialogs and groups are named by their visible title through a `labelled_by`
  relation rather than by a copy of it, so the title stays reviewable in its own
  right. Live regions — `Banner`, `Toast`, `MessageBox` — keep their own name
  instead, because the announcement path reads a node's own value.
- The code editor and log view no longer announce "line 42 of 200". The ordinal
  needed a `Role::Paragraph` node per line, and that node is what dropped every
  text-change event on all three platforms; line position is available through
  line navigation everywhere, and in `CodeGutter`.
- `TextLayout` carries an optional `geometry`, `AccessNodeBuilder::build`
  returns a fourth element (the widget-local rects of any synthetic children),
  and `MockTextBackend` measures characters rather than bytes and treats a
  newline as a hard break.
- A markup label with `TextOverflow::Ellipsis` now measures as one line, which
  is how it was already painted.

#### Automation

- **The bridge announce now says when the MCP client is not installed**, and how
  to get it. An app needs nothing installed to be automatable: the bridge is
  compiled into the debug build and binds its own endpoint, and
  `teksilo-automation-mcp` is only the client an agent drives it through. That
  asymmetry is easy to get wrong from outside, and a harness that got it wrong
  ended up carrying its own copy of where to look, which version to ask for, and
  an account of this crate's protocol history. The absence is now reported where
  the instruction to use the client is given.

### Fixed

#### Bounded-scalar controls

- **`Ctrl` / `Alt` / `Super` chords no longer drive a bounded control.**
  `Ctrl+ArrowUp` stepped a `SpinBox`, `Ctrl+Home` drove a `Slider` to its
  minimum, `Ctrl+PageDown` scrolled a `ScrollBar` and `Ctrl+ArrowRight` resized
  a `Splitter` — each answering the key and reporting it handled, so the chord
  never reached the application's own Shortcut/Action pipeline. They fall
  through now, which is what `QAbstractSpinBox` does. `Shift` is deliberately
  untouched: it is not a distinct chord on any of these controls, and the
  single-line field binds nothing to `Shift+Up`, so rejecting it would only
  leave the chord dead. **Behaviour change.**
- **A `Splitter`'s resize arrows move the divider the way they point**, on
  both axes and in both layout directions. Two separate inversions: the pane
  order and the drag math have been mirrored since the widget shipped — the
  direction is written into a cell the drag path reads — but the keyboard was
  not, so `ArrowRight` pulled the divider left in a right-to-left window; and
  `increase` is axis-relative, so reading it as a geometric direction made
  `ArrowDown` shrink the pane above a *vertical* divider where it had always
  grown it. Both now resolve through `towards_trailing`, which is named for
  what it answers — "does the leading side get bigger" — rather than for a
  screen axis it does not track once the layout direction is folded in.
  Direction is read at event time, so a locale flip needs no rebuild.
  **Behaviour change.**
- **A `Trailing` or `Bottom` dock side resized in the wrong direction from the
  keyboard.** Its handle sits on the side's inner edge, so growing the side
  moves the handle *towards* the centre; the pointer path already inverted per
  side (`side_main`) and the arrows did not, so they moved the handle the way
  they did not point. **Behaviour change.**
- **A right-to-left `Slider` read every one of its three surfaces
  left-to-right.** A horizontal slider's minimum sits at the *leading* edge,
  which is the right one there, so the fill now grows leftward from a thumb
  that travels the other way, the click-to-jump reads from the other end, and
  the arrows follow. All three key off one answer, read at paint and at event
  time: mirroring any of them alone would leave the `Left` key and a leftward
  drag moving the thumb in opposite directions. A custom `SliderStyle` carries
  the same obligation, which its trait now states. **Behaviour change.**
- **A right-to-left horizontal `ScrollBar` disagreed with its own
  `ScrollArea`.** The area had already mirrored — it anchors content to the
  right and grows `scroll_x` leftward — while the bar placed its thumb at
  `bounds.x + offset`, so at `scroll_x = 0` the content showed its beginning
  and the thumb sat at the far end of the track, and a track click paged the
  wrong way. The thumb, the drag delta, the track-click direction and the
  arrows now key off one predicate. **Behaviour change.**
- `ScrollBar` no longer claims `Action::SetValue`. Its node is `set_hidden()`
  and it never advertised the action, so the arm was unreachable except to
  answer `Handled` to a `SetValue` bubbling up from a descendant and drop it.
- **The right-to-left horizontal `ScrollBar`'s *painted* thumb now mirrors
  too.** The widget's hit-test and drag were mirrored; the style that draws the
  thumb was not, so the thumb was rendered at one end of the track while the
  region that grabs it sat at the other, and it jumped the moment it was
  touched. Both painters in the default `RecipeScrollBarStyle` read
  `PaintContext::layout_direction`, and the trait now states the obligation the
  way `SliderStyle`'s does. **Behaviour change.**
- **A horizontal `ScrollBar`'s `PageUp` scrolled forward and its `PageDown`
  back**, the opposite of the vertical bar beside it. The page keys name a
  direction in the *content*, not on screen, so unlike the arrows they do not
  follow the bar's orientation — reading `increase` geometrically for them was
  what inverted the horizontal case. **Behaviour change.**
- **A vertical `Slider`, `HueStrip` and `AlphaStrip` put their maximum at the
  top.** All three grew their value downward, so `ArrowUp` — which the chord
  table reports as an increase — moved the thumb *down*: the keys and the
  pointer drove the control in opposite directions on screen. Qt's `QSlider`,
  GTK4's `GtkScale`, the Win32 trackbar and `<input type=range>` all put a
  vertical minimum at the bottom, so the geometry moved rather than the chord
  table. The hue strip's rainbow is reversed to match. **Behaviour change.**
- **`Ctrl+Enter` on a `Splitter` divider or a dock resize handle reaches the
  application.** Both matched `Enter` before any modifier test, so the chord
  collapsed a pane or hid a side and reported the key handled — the one row of
  the documented chord table those two did not honour, while every other key on
  them did. **Behaviour change.**
- **An assistive technology's `Slider` write lands on the grid its arrows
  walk.** The advertised `numeric_value_step`, an arrow press and `Increment`
  all move by the effective step — 1 % of the range when none is configured —
  while `SetValue` snapped to the *configured* step, so on a stepless slider a
  write landed between two values every other path could reach, and a screen
  reader then announced a number its own Up arrow could not produce.
  **Behaviour change.**

#### Widgets

- **A `SpinBox` now handles the `Action::SetValue` it advertises.** It carried
  the advertisement — which macOS needs, since AXValue settability is gated on
  it — while its handler could not see an `ActionData` payload at all, so every
  assistive-technology write was dropped: a `NumericValue` from macOS's
  `setAccessibilityValue:` with an `NSNumber` or from AT-SPI's
  `Value.SetCurrentValue`, and a `Value` string from an `NSString` or from
  Teksilo's own automation `set_value` tool, which was a silent no-op against
  every `SpinBox` in the catalog. Both shapes now go through the widget's own
  commit: a number is clamped and published as sent, a string takes the parse
  `Enter` takes, so a custom `value_from_text` and the locale's decimal
  separator are honoured and an unparseable one is *reported* unhandled rather
  than failing quietly. A read-only spin box advertises and services none of
  `Increment` / `Decrement` / `SetValue`.
- **An assistive-technology `SetValue` aimed at a composite's inner text node
  now commits.** A `SpinBox` or a date editor publishes two nodes — the value
  root and the `Role::TextInput` beneath it — and setting the field replaced
  the displayed string and stopped there, so the typed value stayed stale
  until the next blur and `on_value_changed` never fired. The field commits
  through its host now, by the same parse, clamp and revert `Enter` performs.
  A plain `TextInput` is unchanged: its bound signal already *is* the value.
- **A modified letter chord over a focused `ComboBox` no longer changes the
  selection.** Type-ahead matched any key carrying a character and returned
  `Handled`, so an unregistered `Ctrl+C` appended `c` to the prefix, jumped the
  value to the first item starting with it, and blocked every ancestor
  `on_key`. `Ctrl` / `Alt` / `Super` now fall through — `Shift` still types, as
  in `MenuList` — and the same rule covers the arrow, `Home`/`End` and page
  keys, so `Ctrl+Home` no longer picks item 0.
- **`ComboBox` arrows stop at the ends instead of wrapping.** Its page keys
  already clamped, so the widget disagreed with itself inside one handler, and
  wrapping means one keypress too many sends a setting from "Never" to
  "Always". Win32, `QComboBox`, GTK, the ARIA listbox pattern and Teksilo's own
  `ListView` all stop at the ends; menus still wrap, because a menu is a list
  of commands rather than a value. **Behaviour change.**
- **`Alt+ArrowDown` on a `DateEdit` no longer steps the date.** The segment
  stepper reads only `Shift`, so the chord two module docs promised as "opens
  the calendar popover" quietly moved the value back one day. Other `Ctrl` /
  `Super`-modified arrows now fall through instead of stepping.
- **`DateEdit` and `TimeEdit` documented step sizes they have never applied.**
  Both describe *segment*-relative stepping now — one unit of the field under
  the caret, ten with `Shift`, ten on a page key and a hundred on
  `Shift`+page — which is what the code does and what `QDateTimeEdit` does.
  `TimeEdit::step_minutes` is documented as the inert builder it has been since
  segment stepping replaced whole-value stepping.
- `SplitButton`'s `ArrowDown` accepts a bare or `Alt`-modified chord, as its
  comment always claimed; `Ctrl+ArrowDown` and `Cmd+ArrowDown` no longer open
  the menu.
- `SpinBox::read_only` documented that keyboard and button stepping still
  worked. It never did — the keys, the wheel and the buttons are all gated,
  which is what `QAbstractSpinBox::readOnly` does too.
- **A read-only `SpinBox`, `DateEdit`, `TimeEdit`, `DateTimeEdit` or
  `DateRangeEdit` refuses an assistive technology's write on its *inner text
  node* as well as on its root.** Not advertising an action is not the same as
  refusing it: an adapter dispatches what the technology asks for, and AT-SPI
  publishes `EditableText` off the interface set rather than off the action
  list, so a write aimed at the field went straight past the composite's
  read-only gate and rewrote the value. **Behaviour change.**
- **An assistive technology's whole-value write reports the host's verdict.**
  `SetValue` on a text-projected composite answered `Handled` whatever the
  parse said, so `"twelve"` in Orca's value entry and in macOS's
  `setAccessibilityValue:` both read back as success while the field quietly
  reverted to the value it still held. `TextInputField::on_access_set_value`
  now returns whether the host accepted the string. **Behaviour change.**
- **`AltGr`-composed characters reach a `ComboBox`'s and a `MenuList`'s
  type-ahead.** Both refused the union of `Ctrl` / `Alt` / `Super`, and `AltGr`
  arrives as `Ctrl+Alt` on Windows, X11 and Wayland alike — so every character
  behind it (`@` on a German layout, `€` on a French one, `ą` on a Polish one)
  was dead for type-ahead while the unshifted ones kept working. `Ctrl` alone
  and `Alt` alone stay refused. **Behaviour change.**

#### Data views

- `TableView` / `TreeTableView`: dragging a column divider now moves it with
  the pointer. Only the columns after the divider reflow; a `Flex` column
  before it is frozen at its current width in the same `column_widths_signal`
  write. Before, the resized column's delta was shared with every flex column,
  so the divider lagged the pointer (or stood still) while the dividers before
  it slid the other way.
- `TableView` / `TreeTableView`: dropping a dragged column at the very front
  or the very end of a strip with no pinned pane on that side moves it there
  instead of pinning it `Leading` / `Trailing`.
- `TableView` / `TreeTableView`: the header strip's column separators now
  follow a column resize and a horizontal scroll. The strip's bounds change on
  neither, so its cached paint was replayed and the lines stayed where they
  were while the cells moved.
- **A row height a view *declares* is laid out at the height it declares.**
  `ListView::item_height_fn`, `TableView::row_height_fn` and `GridView`'s exact
  `item_height` fed the offset table through the same setter a measurement pass
  uses, whose sub-pixel epsilon exists to stop a re-measure from oscillating the
  scroll anchor. So a declared height within `0.01` px of the table's internal
  placeholder was discarded as jitter and the row kept the placeholder — a list
  of 1.005 px rows laid every row out at 1.0 px, and the arithmetic uniform-height
  mode describing the same geometry then disagreed with it. Declared heights and
  measured ones now take different doors.

#### Core

- **An assistive-technology `Action::Focus` on a composite lands on the node
  that takes the keys.** The dispatcher services `Focus` itself and focused the
  targeted node directly, while `ctx.request_focus` has always walked to the
  first focusable descendant. So an assistive technology — or the automation
  `focus_node` tool — aimed at a `SpinBox`, `DateEdit` or `TextInput` parked
  focus on a composite root that accepts no keystrokes, and, because
  `on_key_preview` fires only on *strict* ancestors of the focused node,
  disarmed the composite's own stepping keys in the process. The walk is taken
  only from a node that **advertises** `Action::Focus`, which is what a
  composite does and a `Panel`, a `GroupBox`, a landmark or a label does not:
  walking in from any non-focusable node moved the keyboard onto the first
  control inside it — one the technology could have named itself and did not —
  and then reported success.
- **An app-installed `.on_access_action(..)` fires on a widget that services
  the payload shape itself.** The dispatcher called
  `on_access_action_request` *instead of* `on_access_action` whenever the
  former was set, so adding an application handler to a `Slider`, `SpinBox`,
  `TextInput`, `CodeEditor` or `TabBar` produced a handler that never ran, with
  nothing at the call site to say so. Every installed slot now fires for one
  dispatched action, and the action counts as handled if any of them says so.

#### Menus

- **Menu item labels line up on one leading inset again under the Fluent and
  macOS presets.** Both stack the shared menu row over their own highlight
  rect, and the row was measured at its content width and centred there — so
  each label sat further in by half its own slack, ragged from row to row.
- **`MenuItemRecipe`'s `icon_column_width`, `padding_horizontal` and
  `separator_height` now reach the parts of the row they describe.** The
  leading icon/check column, the trailing chevron column and `MenuSeparator`
  read the active style through the new
  `MenuItemStyle::metrics() -> MenuItemMetrics` (defaulted, so an existing
  style needs no change) instead of the IntUI module constants. macOS menus
  get their 14 dp check column and 11 dp separator, Fluent its 11 dp trailing
  column and 3 dp `MenuFlyoutSeparatorThemePadding`.
- **A submenu survives the diagonal to reach it.** The safe triangle now
  holds off the submenu overlay's own pointer-leave grace, not just a sibling
  row's hover-switch: the grace used to start the moment the pointer left the
  trigger row and closed the submenu 150 ms later, mid-flight, however slowly
  the user was travelling. It is armed where the diagonal starts — the point
  the pointer leaves the trigger — so a submenu opened by click, Enter or
  ArrowRight is covered too, and it expires after 600 ms so a parked pointer
  releases the menu instead of pinning it.
- **The submenu no longer goes the instant the pointer leaves the trigger
  row.** The safe triangle is a needle at its apex, and the sample that decided
  whether the user was heading for the submenu was the first one off the row —
  a pixel or two out, where the cone is a few degrees wide. For a menu wider
  than its submenu is tall, which is the usual shape, any departure steeper
  than about 30° missed, and one miss was final: the region could only ever be
  armed as the pointer left the row, so nothing could re-arm it. Straying out
  of the cone now merely starts the ordinary close delay, and heading back in
  cancels it; a sibling row stands aside for the whole traversal instead of
  ruling on that one sample. A change of mind still closes the submenu, one
  close delay later rather than at once.
- **Crossing a neighbouring submenu trigger no longer closes the submenu you
  are walking to.** The hover-switch between two submenu triggers now dismisses
  the open submenu when the new one *opens*, not when the pointer first touches
  the row — so passing through costs nothing and settling still swaps on the
  same frame.
- **A submenu trigger stays highlighted while its submenu is on screen.** It
  used to un-highlight as soon as the pointer left the row, which is the whole
  time the submenu is up, leaving the open panel with no visible parent. Its
  `set_expanded` was reporting collapsed over the same window.

#### Accessibility

- Eleven labels announced their control's name a second time — in `Banner`,
  the docking activity bar (twice), `MessageBox`, `SearchField`, `Stepper`, the
  calendar's zoom cell, a tab header, `Toast`, `Toggle` and `TooltipWidget`.
  They are hidden now; the control keeps the name.
- The privacy-settings rows painted their label twice on screen.
- A markup label announced its raw source — brackets, parentheses and URL and
  all — instead of its rendered text, and its links were labelled with a
  mis-sliced fragment of that source and re-identified whenever the label
  rewrapped.
- A label bound to a signal changed on screen without telling assistive
  technology, and a widget that moved, resized, or was re-themed or re-scaled
  went on reporting the position and metrics it had before.
- A `labelled_by` relation pointing at a node absent from the tree — a dormant
  tab panel, a pruned stack — crashed the consumer as it built the name.
- The rich-text and code editors emitted no text-change events at all: their
  runs sat under `Role::Paragraph` nodes, and a run's update routes to its
  filtered parent, which supports no text ranges.

#### Layout

- `FormLayout` survives a rebuild. Its rows are handed in once and cannot be
  reconstructed, so it now re-attaches them rather than re-deriving them —
  before, any rebuild (a locale switch, a signal on an ancestor) left an empty
  form measuring zero by zero, and took whatever the user had typed with it.

## [0.9.4] - 2026-09-05

One fix, to the fallback the automation bridge takes when `$TMPDIR` is too deep
to hold a socket: it could not bind at all on macOS, and on Linux it reached
for a permission change on `/tmp` that was never its to make.

### Fixed

#### Automation

- **The short `/tmp` fallback tried to make `/tmp` itself owner-only.** The
  bridge drops to `/tmp/tka-<pid>` when `$TMPDIR` would overflow `sun_path`,
  and then tightened that path's *parent*. On macOS it could therefore never
  bind: `/tmp` is a symlink, which the check rejects by design, the target
  not being what it would be protecting. On Linux the same call chmods a
  `1777` `/tmp` to `0700` — `EPERM` for a normal user, and a silent success,
  for every other process on the machine, for one running as root. Only the
  bridge's own descriptor directory is tightened now; the per-process socket
  directory underneath is still created `0700`.

## [0.9.3] - 2026-09-04

Three unrelated strands: one keyboard contract for the five data views, MCP
automation on Windows and macOS, and a text field that stops painting a
selection it does not own.

### Added

#### Data views

See [docs/data-view-keyboard.md](docs/data-view-keyboard.md) for the full chord
table.

- `common::list_nav`: the edge-and-page chords as pure functions over
  `(key, modifiers, view kind)`, with `_for` twins so both platform branches
  are reachable from one host's test run.
- `SelectionModel::extend_to_additive`, and `Ctrl+Shift`+navigation on top of
  it: a second disjoint range can be built without losing the first.
- `Ctrl+Shift+A` deselects everything, in all five views.
- Tree expand chords in `TreeView` and `TreeTableView`: `*` expands a subtree,
  `+` and `-` one level, `→` on an open node moves to its first child.
- macOS aliases, all previously dead: `⌘↓` opens the focused row, `⌘↑`
  collapses or ascends, `⌥→` / `⌥←` expand or collapse a subtree.
- `PageUp` / `PageDown` in `ScrollBar` and `MenuList`; `Home` / `End` /
  `PageUp` / `PageDown` in `CommandPalette`.

#### Automation

- `teksilo-platform::automation_transport`, behind the `automation` feature:
  the per-OS endpoint the live bridge binds. Unix domain socket, Windows named
  pipe.
- `teksilo-automation::wire`: framing, token handshake and endpoint descriptor
  in pure `std` + `serde`, shared by both ends of the bridge.
- `install_automation_bridge_in_debug()` works on Linux, macOS and Windows, and
  is still a no-op in release on all three.
- The app publishes an endpoint descriptor at
  `<runtime dir>/teksilo-automation/<pid>.json`, so attaching needs nothing
  scraped from stderr.
- `--attach` (newest live app), `--attach-pid <pid>` and `--list` in
  `teksilo-automation-mcp`; `--connect <endpoint> --token <uuid>` remains as
  the explicit escape hatch.
- `command` modifier on `inject_key`, `inject_pointer` and `scroll`: the
  platform's primary accelerator, Control on Windows and Linux and ⌘ on macOS,
  where `ctrl` stays literal.
- Screenshots return a `{width, height, scale}` block beside the image. Pixels
  are physical; every other coordinate in the toolkit is logical.
- Error codes `GPU_READBACK_FAILED`, `BRIDGE_TIMEOUT`, `BRIDGE_DROPPED`,
  `BAD_REQUEST` and `BRIDGE_IO`, as constants rather than message strings.
- A `test-automation` CI job on Linux, macOS and Windows, driving a real window
  over the real endpoint and carrying the release canary.

#### Core

- `EventContext::focused()`, the widget that held focus when the event batch
  began. A widget-scoped shortcut is matched before the focused widget is
  offered the key, so a container that binds a key its own children also handle
  had no way to tell whether one of them was standing under it. Part of the
  same per-dispatch snapshot as the pointer position and the overlay bounds, so
  it reads `None` in a hand-made `EventContext`, and it is the focus as of
  dispatch time rather than a live read: a handler that has already called
  `request_focus` still sees the old value, which is the useful reading for one
  deciding whether to act at all.

### Changed

- **One GPU device per process**, shared by every window (`teksilo-platform`)
  and every offscreen renderer (`teksilo-render`), instead of one per window
  and one per caller. Each still gets its own surface and its own `Renderer`,
  so no caller sees another's cached glyphs; a window the shared adapter cannot
  present to gets its own device.
- `wait_for_condition` spends its budget as simulated frames rather than wall
  clock, so the same budget buys the same number of frames on every platform.
  The wall-clock backstop is 1× the budget, floor 250 ms. **Behaviour change.**
- The live bridge's reply wait is bounded at 15 s, reported as
  `BRIDGE_TIMEOUT`. **Behaviour change.**
- The transport's `probe` answers `Live` / `Busy` / `Dead` rather than a bool.
  **Behaviour change**: only an unambiguous absence unregisters an app.
- `try_read_texture_rgba` returns a `Result`, so a lost device costs one
  screenshot rather than the thread. **Behaviour change.**
- Offscreen renderers take the adapter's real limits rather than
  `downlevel_defaults`, which capped textures at 2048 where the path atlas
  grows to 4096.
- The Unix runtime directory on macOS is `$TMPDIR`, not the shared `/tmp`.
- `PlatformWindow::new` and `new_with_a11y` share their GPU and swapchain
  setup, and differ only in whether an AccessKit adapter is attached.

### Fixed

#### Data views

- **A `Shift` range could only grow.** `Shift+End` then `Shift+Home` selected
  the whole collection instead of reversing. Ranges are recomputed from a
  committed base. **Behaviour change** for anything driving
  `SelectionModel::extend_to` directly.
- **`Ctrl`+`Home` / `End` / `Page` moved the selection.** They move the cursor
  and leave the selection alone. **Behaviour change.**
- **`TableView`'s `Home` moved the column in every mode**, including the
  default `MultiRow`, which also made `Shift+Home` a no-op there. Scope follows
  the cursor topology. **Behaviour change.**
- **`GridView` paged by an estimated row height** and scrolled twice per
  keypress, and its `Home` / `End` stopped at the ends of a reflow row rather
  than the first and last tile. **Behaviour change.**
- **`GridView`'s `Ctrl+A` ignored the selection mode**, and its `Space` never
  toggled.
- **An open cell editor lost keys to the table**: `PageDown` inside an editor
  paged the cursor out from under the edit.
- **`TreeTableView`'s `←` was a dead key on every leaf**, and its expand /
  collapse arrows ignored the modifiers, so `Shift+→` opened a row instead of
  extending the selection.
- **`TreeView`'s expand / collapse arrows ignored the layout direction**, so a
  right-to-left tree collapsed on the wrong key.
- **Type-ahead could not reach an accented label**: case was folded with
  `to_ascii_lowercase`, which leaves `É` alone on both sides.
- **A selectable `TableView` exposed no selection to Windows assistive tech.**
  It announces `Role::Grid` now, and its cells `Role::GridCell` in a
  cell-selection mode. No effect on macOS or AT-SPI, where the roles coincide.
- **`TreeTableView` advertised an expand it never performed**, so Windows could
  ask a row to open and watch nothing happen.
- **`TableView`, `TreeTableView` and `GridView` ignored
  `Action::ScrollIntoView`**, so assistive tech had no way to bring a row or a
  tile into view.

#### Automation

- **Two concurrent offscreen renders killed the process** roughly a quarter of
  the time on a WARP host: two D3D12 WARP devices rasterizing at once fault
  inside `d3d10warp.dll`. Closed by the shared device above. The same fault was
  latent, never live, for windows.
- **`create_test_renderer` reported "no GPU" on machines that have one.**
  Adapter selection is a search: preferred, then an explicit software
  fallback, so a host that enumerates an adapter it cannot open still gets a
  device.
- **A value-taking flag with its value missing** (`--connect`, `--attach-pid`,
  `--token`) quietly started the demo server. It is an error.
- **`--list` poisoned the `--attach` that followed it** on Windows. The pipe
  server recycles an instance a client opened and dropped before it could be
  connected, and `connect` waits out `ERROR_PIPE_BUSY`.
- **A stale descriptor, left by an app that exited without unwinding, was
  offered as live.** Descriptors are probed and pruned; one that probes `Busy`
  is kept.
- **A descriptor outlived a failed bridge start**, handing `--attach-pid` an
  endpoint that answers nobody. It is retracted if the accept thread fails to
  spawn.
- **A read deadline on the Unix socket expired as `WouldBlock`**, not as the
  `TimedOut` the transport trait documents and the Windows named pipe already
  returns.

#### Widgets

- **A text field that does not hold focus no longer paints its selection**, in
  `TextInput`, `PasswordField`, `SpinBox`, `SearchField`, `DateEdit` /
  `TimeEdit` / `DateTimeEdit`, `HexColorInput` and `FilePickerField`.
  **Behaviour change**, visual only: the selection state is unchanged, a field
  that keeps focus while its window goes inactive still dims rather than hides,
  and `RichTextEditor`, `CodeEditor` and `LogView` are unaffected.
- **Enter in a `MessageBox` answers for the focused button**, and falls back to
  `default_button` only when the focus is not on one of the box's own buttons.
  The shortcut carrying Enter is widget-scoped, so it was matched before the
  focused button was offered the key, and the default answered for a button the
  user had deliberately tabbed to. **Behaviour change**, and the case it was
  destructive in is the ordinary one: a `YesNo` confirmation defaults to No, so
  standing on Yes and pressing Enter closed the dialog reporting No, and the
  caller saw a user who had declined. Escape is unchanged, Space was always the
  focused button, and a box whose focus sits on its checkbox still gets the
  default.

### Security

All of these concern the automation bridge, which is debug-only.

- The Windows named pipe carries an **owner-only DACL** built from the process
  token's SID. The default descriptor grants read access to Everyone and to the
  anonymous account. `PIPE_REJECT_REMOTE_CLIENTS` is set as a second layer.
- The Unix socket is `0600` in a `0700` per-process directory, created before
  the descriptor is published.
- The endpoint descriptor, which carries the token, is created with
  `create_new` and its mode in the same `open` rather than written and then
  `chmod`ed, so it also refuses a symlink planted at its path.
- An existing runtime directory must be a real directory rather than a symlink,
  and a mode reachable by others is tightened to `0700`. The documented
  fallback for a Unix with neither `$XDG_RUNTIME_DIR` nor `$TMPDIR` is the
  shared `/tmp`.
- The token handshake carries an end-to-end deadline, so a peer dripping one
  byte per timeout cannot hold the single connection slot.

### Known limitations

- A selected *row* still reports no `IsSelected` on Windows: `Role::Row` is
  absent from `accesskit_windows`' selection-item list, which carries its own
  `// TODO: tables (#29)`.
- `Role::TreeGrid` maps to `NSAccessibilityTableRole`, so a `TreeTableView`
  reads flat under VoiceOver where a `TreeView` does not.
- Keyboard access to the column header — sort, resize, reorder — is missing,
  and remains a WCAG 2.1.1 / 2.5.7 exposure.


## [0.9.2] - 2026-09-03

Accessibility. Every entry changes what a screen reader says, and four of them
fix something that reached no assistive client at all.

### Added

- `TextInput::field_id()`, the id of the inner node that actually takes focus,
  so a form can send focus to the field a validator refused and a modal can
  pick one field out of several. Take the slot before `ctx.add`, read it after,
  as with `caret_setter` and `handle`. `TextInput` also answers
  `initial_focus_hint` with that field now.
- `WindowConfig::app_id`, the identity a desktop matches a window against.
  Defaults to `None`, which leaves winit's behaviour unchanged; set it to the
  basename of the installed desktop entry. One call covers Wayland and X11.
  Windows and macOS ignore it.

### Fixed

- **A labelled `TextInput` was nameless to every screen reader**, on every
  platform, always: the name was written to the composite's own
  `Role::GenericContainer` node, which `accesskit_consumer`'s filter excludes
  unconditionally. It goes on the inner field now, and is locale-reactive where
  the old snapshot was not. `TimeEdit`, `HexColorInput`, `FilePickerField`,
  `SearchField` and `FontPicker` forward into the same call and are fixed with
  it; an explicit `access_label` still wins. **Behaviour change** in what a
  screen reader says.
- **`PreviewCanvas` and the inspector body named a `GenericContainer`** too.
  Both are `Role::Group` now.
- **Nothing focusable advertised `Action::Focus`**, so assistive technology
  could not put focus in a `ListView`, `TreeView`, `TableView`,
  `TreeTableView`, `GridView`, `MenuList` or `OverlayTrigger` — each of which
  is the only focusable node in its subtree. It is derived from the arena's
  focusable flag now rather than left to each widget;
  `access_remove_action(Action::Focus)` still takes it away. **Behaviour
  change:** every focusable node carries the action.
- **No data view told a screen reader which row was current.** Arrowing through
  a `ListView` was silent to NVDA. All five views nominate the current row as
  their `active_descendant`, gated on the view holding focus, and reveal it
  when they take focus; `ListView` also reveals on a selection change.
  `TableView`'s reveal is vertical only. **Behaviour change.**
- **A selection move replaced every realized row node**, resetting the scroll
  offset and the keyboard anchor and handing the accessibility tree fresh node
  ids on every keystroke. `ListItemWrapper` is the rebuild boundary now, so
  only the two rows whose state flipped are rebuilt.
- **A multi-select `ListView`, `TreeView` or `TreeTableView` reported that it
  could hold one row.** `multiselectable` is published, gated on the selection
  mode — `accesskit_windows` picks `ElementAddedToSelection` over
  `ElementSelected` from that property, so publishing it unconditionally would
  raise the wrong event. `TableView` is left out: `Role::Table` exposes no
  selection for the property to describe.

### Known limitations

- `TreeView` publishes no `size_of_set`. A flattened tree cannot express "the
  2nd of 5 siblings" from one container value; that needs a `Role::Group` per
  expanded branch, which changes the tree shape for every tree widget.


## [0.9.1] - 2026-09-02

Accessibility again, plus the documentation sweep that followed it and four
independent fixes. Two of the accessibility entries change what every existing
application announces.

### Added

- **A context menu can be opened from the keyboard**, on every widget that
  already has a `.context_menu(..)`: the dedicated Menu key (`Key::ContextMenu`,
  new), Shift+F10, and Ctrl+Shift+M on macOS, where neither of the other two
  exists. Modifiers are matched exactly and the chords sit below shortcut
  resolution, so an application that binds Shift+F10 keeps it. **Breaking:**
  `Key` is not `#[non_exhaustive]`, so the new variant breaks an exhaustive
  match, and a settings file containing it does not load on an older build.
- `Widget::context_menu_key_target`, default `None`, so a focusable container
  can nominate a descendant as the menu target. All five data views implement
  it against their keyboard cursor, falling back to the first selected row; the
  menu is anchored at the target's own bounds.
- `ctx.announce(msg)` and `ctx.announce_with(msg, Politeness)`, on
  `EventContext`, `BuildContext` and `WidgetTree`: an announcement that reaches
  a screen reader on all three platforms, including repeats of the same
  message. Takes `impl Into<String>`, so `tr!(..)` works directly. Messages
  queue rather than replace one another. Do not pair it with a `show_toast()`
  on the same path — a toast is already a live region, and the user hears
  everything twice.
- `WidgetTree::accessibility_tree_snapshot`, which builds a `TreeUpdate` for
  inspection without caching it, bumping the AT version, recording
  announcements or advancing the live regions.
- `WeakEditorHandle` and `EditorHandle::downgrade`, for a rich-text handler that
  must not keep its own editor alive. Handlers stored on the editor's state
  (`on_image_activated`, `on_link_activated`, `on_change`, `on_text_inserted`,
  the image resolver and the rest) make the state own itself if they capture a
  strong handle, and nothing afterwards can break the ring.
- crates.io metadata on every crate: `homepage`, `documentation`, `keywords`
  and `categories`, so each links to its own docs.rs page. The workspace root
  gained `homepage = "https://teksilo.rs"` and the canonical `repository` URL.

### Changed

- `text-document` 1.12 and `text-typeset` 1.10.
- **A failed `assert_node` is a failure on every automation transport.** It
  returns `AutomationReply::Err { code: "ASSERTION_FAILED" }` rather than
  `Ok(AssertionResult { passed: false })`, so `isError` falls out for the MCP
  server and the socket bridge alike. A property assertion against a node that
  is not in the tree is `NOT_FOUND`; `kind: "exists"` against a missing node
  stays `ASSERTION_FAILED`. **Breaking** for callers that matched on `Ok` and
  read `.passed`.
- **`MessageBoxButtons::YesNo` and `YesNoCancel` default to No**, which also
  takes initial focus. Enter on a confirmation dialog declines. `Ok`,
  `OkCancel`, `SaveDiscardCancel` and `RetryIgnoreAbort` are unchanged. Put Yes
  back with `.default_button(StandardButton::Yes)` where the question is safe.
  **Behaviour change.**
- **A font face is registered without copying its bytes.** `FontFaceSpec::data`
  is `text_typeset::SharedFontData` (`Arc<dyn AsRef<[u8]> + Sync + Send>`,
  re-exported from `teksilo-text`), and `VecFontRegistrar` uses
  `register_font_shared`. A `&'static [u8]` now shares rodata instead of being
  copied twice and held for the life of the process. **Breaking** for code that
  names the field's type; `Arc<Vec<u8>>` still coerces. Worth 12.8 MB of
  baseline in a downstream application bundling eight serifs.

### Fixed

- **No collection ever announced "of N", and `ListView` announced nothing at
  all.** AccessKit resolves `size_of_set` by walking *up* from an item, unlike
  ARIA's per-item `aria-setsize`, and all fifteen of Teksilo's writes were on
  items, so the property was inert everywhere. The count now sits on the
  container in `ListView`, the tab bar, the combo panel, `SearchField`'s
  suggestions, the code editor's completion popup, `GridView`, `ColumnFlow`,
  `SegmentedControl`, `RadioTileGroup`, the stepper and the docking rail.
  `ListView` rows publish their position in the **model**, not in the realized
  window. `TreeView` and `TreeTableView` drop a write no adapter read. See
  0.9.2's known limitation for what a flattened tree still cannot say.
  **Behaviour change.**
- **Every position, row, column and level was one too high.** AccessKit's
  `position_in_set`, `row_index`, `column_index` and `level` are 0-based where
  their ARIA counterparts are 1-based, and both consuming adapters add the 1
  back, so the first tab of five announced as "tab 2" and a root tree item as
  "level 2", with no way to say "level 1" at all. macOS reads none of the four,
  which is why it went unseen there. The public API stays 1-based and the
  conversion happens once at the `AccessNodeBuilder` boundary; a new
  `set_level` joins the other three, and the heading clamp no longer makes
  `<h1>` unreachable. `aria_ordinal_conventions.rs` fails the build if a widget
  reaches past the typed setter. **Behaviour change** across all fourteen
  affected widgets; remove any hand-compensation for the old behaviour.
- **53 places where the documentation contradicted the code.** Among them:
  `teksilo = "0.7"` in three places in the app guide; a claim that builder-call
  order is irrelevant, when `install_toast_default()` panics unless
  `.application(..)` came first; a minimal-build recipe that dropped the `i18n`
  feature and so did not compile; `Expand::flex`'s own doc writing `flex(0)`
  against an `f32`; and 19 uses of non-existent `ButtonVariant` names in the
  teksu spec. Four dead fields went with them, the two competing `typos`
  configurations became one at the repository root, and the widget catalog is
  regenerated.
- **A keystroke recoloured the whole rich-text document.** A block with no
  overlay reported a miss rather than the no-op it had performed, and the frame
  loop treats a miss as grounds to fall back to `flow_snapshot()`.
- **A closed window never released its event subscriptions**, holding whatever
  its closures captured for the life of the process:
  `TreeAppContext::subscription_callbacks` is shared by every window and was
  purged per-widget only. Plain entries carry a window id now.
- **A long category label starved a chart's plot to nothing.** A tilted x-axis
  band was subtracted from the chart height with no floor, and both `BarChart`
  and `LineChart` return early on a zero-height plot — no bars, no grid, no
  axis, no diagnostic. The band is capped at half the chart height and labels
  ellipsize into the width they are granted.
- **Observing a derived signal panicked.** `Signal::observe` rejected anything
  built with `map`, `zip`, `and` or `not`, so the macOS native-menu bridge
  aborted the process before the first window was drawn, through an
  Objective-C frame it could not unwind. Derived signals subscribe on every
  upstream root now. `flat_map` still reports `ReadOnly`, having no fixed root.
- **The X11 drag-and-drop teardown is confirmed by the server**, so `XdndProxy`
  cannot outlive the window it points at and leave the property aimed at a
  destroyed window. 6 failures in 300 runs before, 0 in 600 after.
- **Publish order ignored dev-dependencies and the umbrella crate**, which is
  what failed three crates mid-release for 0.9.0.


## [0.9.0] - 2026-08-31

### Added

- **The framework's own strings speak twenty-one more languages.** ar-SA,
  cs-CZ, da-DK, de-DE, el-GR, es-ES, fi-FI, he-IL, hu-HU, it-IT, ja-JP, ko-KR,
  nb-NO, nl-NL, pl-PL, pt-PT, ro-RO, ru-RU, sv-SE, tr-TR and uk-UA join English
  and French in `framework_locales()`, covering all 308 user-facing strings.
  Each carries its real CLDR cardinal plural categories rather than a copy of
  English's one/other shape.
- `runtime_override` takes a directory as well as a single `.ftl` file, and the
  watcher follows every resource under it. Reloading one file used to replace
  the locale's whole bundle, dropping every key its siblings defined.
- **A number can be read back in the locale it was written in.**
  `NumberSymbols` recovers a locale's separators, signs and digits from ICU's
  own formatted output, so the display and parse directions cannot drift, and
  it works in strings rather than `f64`, so an `i64` past 2^53 keeps its
  precision. `SpinBox` gained `.localized(bool)`, **default on**, and
  `.use_grouping(bool)`, default off as in Qt. **Behaviour change:** a French
  user sees and types `12,5`.
- **The widget catalog renders its own pictures.** `teksilo-widgets-previewer
  --export-docs` renders every registered widget through the production wgpu
  renderer into `docs/widgets/img/`, headless and with no display server; 95 of
  the 136 pages open with a preview. The images are committed rather than built
  in CI, which has no GPU adapter.
- `WidgetTree::text_surfaces()`, `focused_text_surface()` and
  `focused_is_text_surface()`, so an application owning a global `Ctrl+Z` can
  route it to whatever is being edited, including surfaces it did not build.
- **Cell editing with click triggers and persistent focus.** A per-column
  `EditTriggers` bitmask chooses single or double click,
  `on_cell_edit_dismissed` ends an edit on a click away, and
  `BuildContext::focus_into` keeps keyboard focus in a tree-table editor across
  rebuilds. Double-click-to-edit no longer
  costs the row selection on the first click.
- `Button::icon_keeps_color`, for a glyph whose colour is the information — a
  filter chip's tag colour, a legend swatch, a status disc. The same opt-out
  `MenuItem` already had.

### Changed

- **Overlay and tooltip bodies are built on first use** rather than on every
  rebuild of their owner. New `BuildContext::add_deferred`, `add_deferred_boxed`
  and `DeferredSubtree`. The old `ctx.add` + `set_dormant` pair paid a full
  `build()` for content the user may never open, which is invisible in a dialog
  and dominant in a virtualized collection.

### Fixed

- **A language switch did not reach the date and time fields.** Each derives its
  convention — pattern, first day of week, 12- vs 24-hour clock — once in
  `build()`, and `set_locale` deliberately does not rebuild.
- **Escape reached neither past a tooltip nor out of a text field**, two
  independent swallows: the dismissal path treats a hover-shown tooltip as a
  dismissal target and returns, and a focused `TextInputField` had no `Escape`
  arm, so it fell into the printable-character branch. Every dialog whose
  cancel sits outside a field had the same hole.
- **A menu row activated by keyboard panicked on `open_window`** while the same
  row opened its window fine by mouse: the synthetic click was drained through
  `WidgetTree::click`, the test entry point, which installs `NoopWindowOps`.
- **A drop into a folder looked like a drop after it.** Three verdicts shared
  one visual: the `Into` box was painted at the row's exact bounds, so its
  edges occupied the pixels the `Before` and `After` lines use.
- **The four Level-A findings of the 2026-08-28 internal accessibility
  assessment**, including a keyboard trap in `Terminal` (WCAG 2.1.2), which
  answered `Handled` to every `KeyDown` so no chord could leave the widget. The
  assessment document was re-checked against source and reframed as an internal
  engineering assessment — not an ACR, not a VPAT, not third-party verified —
  with a section stating what was not done.


## [0.8.0] - 2026-08-27

Two complete design languages, and the macOS accelerator convention that
building them exposed.

### Added

- **The macOS (Aqua / Dark Aqua) preset**, replacing a stub: opt in with the
  umbrella crate's `theme-macos` feature, reach it as
  `teksilo::prelude::macos::{light, dark}`. 28 style slots, eight of them real
  `impl FooStyle` blocks (push-button bezel, accent focus ring, `NSSwitch`,
  14 dp checkbox and radio, field focus halo, slider knob, menu row, list-row
  selection capsule). The full AppKit vocabulary — four label grades, two
  selection families, the bezel description, the eight System Settings accents
  — is on the `MacOsPalette` extension. `light_with_accent(Color)` /
  `dark_with_accent(Color)` and the `SystemAccent` enum rebuild the accent
  family; `linkColor` deliberately does not follow. Apple publishes almost none
  of this, so every literal is tagged at its definition `[HIG]`, `[measured]`
  or `[derived]`, and four deviations from Apple's own numbers carry the
  contrast measurement that forced each.
  **Known limitations:** the OS accent is not read (the platform layer returns
  only light/dark on macOS); vibrancy uses each material's opaque fallback; the
  `TableView` / `GridView` selection band is an accent wash rather than the
  capsule; San Francisco is named under the optional `system-fonts` feature,
  not bundled.
- **The Fluent (Windows 11 / WinUI 3) preset**, likewise, behind
  `theme-fluent`, as `teksilo::prelude::fluent::{light, dark}`. Every colour is
  transcribed from WinUI's own `Common_themeresources_any.xaml` in `#AARRGGBB`
  notation, so the file diffs against the theme dictionary line by line. 25
  style slots, eight real `impl FooStyle` blocks (button elevation edge,
  two-tone focus ring, `ToggleSwitch`, filled unchecked checkbox and radio,
  field accent underline, two-circle slider thumb, neutral menu hover, list-row
  selection pill). `FluentPalette` carries the graded control fills, on-accent
  strokes and system fills `ColorTokens` has no slot for.
  `light_with_accent(Color)` / `dark_with_accent(Color)` rebuild the accent
  family, leaving every neutral untouched.
  **Known limitations:** Mica and Acrylic use the opaque fallback WinUI itself
  falls back to; Segoe UI Variable is named under `system-fonts`, not bundled.
- `StandardMenu::settings_intent("app.settings")`, with `.settings(label)` for
  the text: a **Settings…** row in the macOS App menu, under About, on ⌘, —
  placement an ordinary `MenuEntry` cannot reach. Unlike Quit there is no system
  fallback, so leaving it unset omits the row rather than rendering a dead one.
- The automation `scroll` op takes `ctrl` / `shift` / `alt` / `meta` beside
  `dx` / `dy`, mirroring `inject_key`. All default false. A probe could
  previously describe Ctrl+wheel but not perform it.
- `StandardItemStyle::selected_label_role` and
  `MenuItemStyle::highlighted_label_role`, both defaulting to `None`, so a
  design language with a solid selection fill can recolour the text on top of
  it. A row builds its label before any `make_body` runs, so it could not.
- `TwistArrow::color(impl Into<ColorProp>)`, taking any colour, role or signal.

### Changed

- **A declared `Ctrl` shortcut fires on ⌘ on macOS.** `KeyStroke::ctrl(Key::F)`
  is read as the platform's primary accelerator, the convention Qt spells
  `Qt::CTRL` and the one the native menu bar always applied when building key
  equivalents. **Behaviour change on macOS:** the chord no longer fires on
  physical ⌃. Opt a chord out with `ShortcutBuilder::literal_modifiers()` —
  Ctrl+Tab, and anything whose ⌘ form the system takes. User overrides stay
  literal, so ⌃F is still bindable. New API: `Modifiers::COMMAND` /
  `Modifiers::command()`, `KeyStroke::command()` / `command_shift()` /
  `with_command_convention()`, `Shortcut::declared_keystrokes()`,
  `ShortcutBuilder::literal_modifiers()`.
- The widget catalog's `ThemeSwitcher` and `--theme` offer macOS and Fluent
  (`macos-light`, `macos-dark`, `fluent-light`, `fluent-dark`). Theme
  restore-on-launch moved to a named `theme_from_id`, and the example gained
  its first tests, including a persist/restore round trip over every offered
  preset.

### Fixed

- **Caret motion follows the platform's own layout.** macOS spreads word, line
  edge and document across three modifiers on the arrows — `⌥←/→`, `⌘←/→`,
  `⌘↑/↓`, with `⌥↑/↓` for paragraph — where Windows and Linux use `Ctrl+←/→`
  and bare `Home`/`End`. Every text surface read a single accelerator flag, so
  `⌘←` jumped a word, `⌘↑` moved one line, and `⌥←` did nothing. The chords go
  through the new `common::text_nav`, fixing `RichTextEditor`, `CodeEditor` and
  everything on `TextInputField`; word-delete moves with them. `Alt+↑/↓` keeps
  move-line in the code editor on every platform. `LogView` is untouched, having
  no caret. `⌘⌫` (delete-to-line-start) is not implemented and falls through to
  a single-character delete.
- **Accelerator chords across the widget catalog**: select-all, the
  discontiguous-selection click, the marquee's additive modifier and Ctrl+Home /
  End all tested physical Control, so on macOS ⌘A did not select all while
  ⌃-click — the secondary click there — did extend a selection. The five data
  views, `Calendar` and the colour picker's swatch grid test
  `Modifiers::command()` now. The text surfaces tested `ctrl() || super_key()`,
  which also made the Win key act as Ctrl on Windows and Linux. The
  Explorer-style cursor pair, Ctrl+Tab, Ctrl+Space and the terminal's control
  codes deliberately stay literal and say so at their sites. Built-in
  context-menu labels built their accelerator text from hard-coded `Ctrl`, so a
  right-click read "Copy ⌃C" while the menu bar showed ⌘C.
- **A tree row's chevron ignored its row's colour.** `TwistArrow` painted a
  hardcoded `TextRole::Secondary`, which under a style that flips a selected
  row's label left a grey smudge on an accent capsule at roughly 2.5:1, under
  WCAG SC 1.4.11's 3:1 floor. `StandardTreeItem` hands it the label's own role.
- **The `SpinBox` mouse wheel was inverted** against `QAbstractSpinBox`,
  `GtkSpinButton`, WinUI's `NumberBox` and its own ArrowDown key: `ScrollDelta`
  is already negated so positive y increases a scroll *offset*, and `SpinBox`
  is the one consumer mapping a notch to a value. The wheel path had no test
  coverage; it has three.


## [0.7.0] - 2026-08-08

### Added

- **`DockAction`**, a dockless command button in the docking activity rail,
  added with `DockRail::action(..)` and rendered by the framework so it matches
  a real activity item. Deliberately more restricted than an activity: never
  draggable, never hidable, never overflow-parked.
  `DockActionPlacement::{Start, End, Pinned}` picks the cluster, `Pinned`
  sitting past the spacer at the rail's far edge. `DockActionId::named("…")` is
  a `const fn`, so ids are module-scope constants and stay stable across runs
  for the accessibility tree and the automation bridge.
  `DockAction::toggled(signal)` is **reflect-only** — the rail never writes it,
  so a derived signal is safe. Nothing about an action is persisted:
  `DockLayoutState` is unchanged and needs no migration, and `DockPolicy` has
  nothing to gate.
- `DockRail::leading_slot` / `trailing_slot`, reaching a side that shows its tab
  strip rather than its rail, where `top_slot` / `bottom_slot` could not. They
  are composed with the "hidden activities" hamburger `DockSidePanel` already
  spends the trailing slot on, so declaring one cannot silently drop the other,
  and a side with zero docks renders them rather than returning early. Their
  visibility contract is weaker than the rail slots' and documented as such:
  they live inside the collapsing content region and disappear with it.
- `ListModel::reconcile_by_key(new_items, key_fn)`, which diffs the live model
  against a new authoritative `Vec<T>` and emits minimal granular `DataChange`s
  — coalesced removes and inserts, single-row moves only where something is out
  of place — and **never** a blanket `Reset`, which would clear the user's
  positional selection on every peer write. This is what
  `PersistedListModel<T>`'s live-reload path is built on.
- `adjust_single_index_for_change` in `data_change.rs`, used by `ListView`'s
  focused-index tracking so a single-selection widget keeps its focus on the
  right row across a reload-driven reconciliation.

### Changed

- **`teksilo-settings` is cross-process safe by default**, not by opt-in.
  `SettingsStore`, `SettingsFile<T>` and `PersistedListModel<T>` — and
  `MruList<T>` and `WindowStateService` on top of them — merge every write
  against the document read fresh under an exclusive lock, and a peer process's
  write arrives live through the `Signal` or `ListModel` a caller is already
  bound to, via a new `notify`-based watcher. Nothing to remember to call,
  unlike `QSettings::sync()`. See [docs/settings.md](docs/settings.md).
  **Breaking:**
  - `SettingsFile::load_shared` is removed; `load` / `load_strict` are
    unconditionally correct, and dropped their now-meaningless `delay`
    parameter.
  - `MruList::toggle_pin` is removed, replaced by `set_pinned(key, bool)`. A
    toggle's effect depends on the state at the moment it runs, which a
    replayable write cannot assume.
  - `PersistedTreeModel<T>` and its `collection::tree` module are removed. They
    had no consumers and carried the whole-snapshot clobber the rest of the
    crate was being hardened against. Reintroduce it ops-based if a real
    consumer appears; do not resurrect the deleted version.
  - `Keyed` is a new trait (`type Key`, `fn key(&self) -> Self::Key`) that
    `MruEntry` now requires alongside its pin and touch methods. An existing
    `impl MruEntry` needs a companion `impl Keyed`.

  `SettingsFile::mutate`, every `SettingsStore` signal, and `MruList::add` /
  `touch` / `remove` / `clear` keep their call-site shape.
- `NotificationArchiveModel::remove(index: usize)` is now
  `remove_by_id(id: u64)`. **Breaking:** an index names a position a peer's
  insert, or this crate's own debounce, can invalidate before the removal
  runs.

### Fixed

- **The docking activity rail was an invalid ARIA `tablist`.**
  `DockActivityBar` set `Role::TabList` on its whole root, so the slots, the
  overflow trigger and the action clusters were non-`Role::Tab` children of a
  tablist, which the ARIA Tabs pattern forbids and real screen readers navigate
  poorly. The role sits on a `DockRailTabList` wrapping only the items now, with
  everything else a sibling under a property-free root the AT pass prunes.
  Restricting the AT children instead would have deleted those controls from the
  tree, trading a spec violation for a WCAG 2.1.1 failure. No app-facing API
  changed.


## Earlier history

Entries before this file was introduced are not backfilled; see `git log`
for the full history.

[Unreleased]: https://github.com/FernTech-EU/teksilo/compare/v0.13.1...HEAD
[0.13.1]: https://github.com/FernTech-EU/teksilo/compare/v0.13.0...v0.13.1
[0.13.0]: https://github.com/FernTech-EU/teksilo/compare/v0.12.1...v0.13.0
[0.12.1]: https://github.com/FernTech-EU/teksilo/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/FernTech-EU/teksilo/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/FernTech-EU/teksilo/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/FernTech-EU/teksilo/compare/v0.9.5...v0.10.0
[0.9.5]: https://github.com/FernTech-EU/teksilo/compare/v0.9.4...v0.9.5
[0.9.4]: https://github.com/FernTech-EU/teksilo/compare/v0.9.3...v0.9.4
[0.9.3]: https://github.com/FernTech-EU/teksilo/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/FernTech-EU/teksilo/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/FernTech-EU/teksilo/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/FernTech-EU/teksilo/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/FernTech-EU/teksilo/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/FernTech-EU/teksilo/compare/v0.6.2...v0.7.0
