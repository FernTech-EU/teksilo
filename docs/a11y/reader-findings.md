<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Screen-reader findings

On 25 and 26 September 2026 every example in the workspace was run through
the [reader harness](reader-harness.md) with Orca, on `main` at `261a218f`,
and what a screen-reader user got was compared with what they should get.
This page says how that was done, what was fixed, what stays open and what
was not measured. The pages under it hold every finding, one entry each.

Measured with Orca 46.1, KWin 6.6 (`kwin_wayland --virtual`), libatspi 2.52,
and AccessKit 0.25 through `accesskit_consumer` 0.39,
`accesskit_atspi_common` 0.20 and `accesskit_unix` 0.23.

## How the sweep was done

The examples were split into 23 groups, one page each below. For each group
one agent read the example, wrote scenarios stating act by act what a reader
should get (`tools/reader/scenarios/<group>.py`), ran them with Orca, most of
them two or three times, and recorded what the reader got. A second agent then
took every finding, ran it again (`tools/reader/scenarios/verify_<group>.py`),
read the Teksilo and adapter source it pointed to, and either confirmed it,
corrected it (its severity, its layer or its cause), or added what the first
had missed. No finding survived that did not reproduce.

Every entry says:

- **Act**: what was done, as keys pressed through the compositor, an AT-SPI
  action, or a focus request, which are three different things to a reader;
- **The reader should get** and **the reader gets**;
- **Platform**: where it was measured. Everything was measured on Linux. A
  claim about Windows or macOS comes from reading `accesskit_windows` 0.35 or
  `accesskit_macos` 0.27 and says so;
- **Severity**: *critical* blocks a task or loses data for a screen-reader
  user; *high* leaves them without what a sighted user gets on a common path;
  *medium* degrades it with a way round; *low* is polish;
- **Layer**: *framework* (Teksilo), *example* (the example's own code),
  *upstream* (AccessKit, libatspi or Orca), *harness*;
- **Evidence**: event lines from the AT-SPI listener, lines of Orca's debug
  log, and the source (`file:line`) that explains them. The run directories
  were not kept; the scenario named in the entry makes them again;
- **Status**: the commit that fixed it, or why it stays open.

K1 and K2 in some entries are the two fixes made before the sweep, which the
sweep ran against: K1 names each window by its title (`89a567cd`), K2 is the
announcer fix (`7545b207`).

## The count

| severity | framework | example | upstream | harness | all | fixed | partly fixed |
|---|---|---|---|---|---|---|---|
| critical | 28 | 1 | 5 | 0 | 34 | 27 | 2 |
| high | 146 | 15 | 26 | 0 | 187 | 64 | 9 |
| medium | 115 | 21 | 18 | 0 | 154 | 13 | 2 |
| low | 75 | 26 | 17 | 1 | 119 | 3 | 0 |
| all | 364 | 63 | 66 | 1 | 494 | 107 | 13 |

494 findings: 107 fixed, 13 partly fixed, 253 open in Teksilo, 63 open in the examples' own code, and 57 upstream. Every sweep finding was reproduced by the verifier; 85 of them were corrected on the way, and the verifiers added 80 the sweep had missed. The last page, the examples swept last, was swept on the build with the fixes and has no second agent's pass.

## What was fixed

Each fix is one commit on `main`, with its own CHANGELOG entry. Each commit's
message gives the test that was written first and seen fail on the old code,
the ways the fix was broken on purpose to see it fail again, and the harness
runs with Orca before and after. Each fix was then reviewed by an agent that
had not written it, which reran the tests and the scenarios; where the review
found a gap, its correction is part of the same commit.

| commit | what a reader gets now | findings | tests written first |
|---|---|---|---|
| `7545b207` | Every message the framework's announcer says is heard, not only the first of a session: each comes from a node the platform has never seen. | (before the sweep) | announcer_tests (5), accessibility_impl announcer tests |
| `89a567cd` | A window is announced by its title, not as a bare "frame". | (before the sweep) | window_name_tests (3) |
| `276ff85b` | Under Wayland, Orca hears every key typed into a Teksilo window: caret moves are spoken, typing is echoed, Orca's own commands work and the key they use no longer reaches the application. A password field's characters are withheld from the report. | [text-15](reader-findings/text.md#text-15) | key_report tests (49), among them the pairing, Num Lock, lost-release, wire-format and secure-field tests |
| `5c8ff299` | Checking a box, flipping a switch, choosing a radio button or moving a slider is heard at once, and a slider reads 0.3, not 0.30000001192092896. | [dialogs-09](reader-findings/dialogs.md#dialogs-09), [menus-15](reader-findings/menus.md#menus-15), [collections-02](reader-findings/data-collections.md#collections-02), [catalog-a-02](reader-findings/catalog-a.md#catalog-a-02), [catalog-a-15](reader-findings/catalog-a.md#catalog-a-15), [catalog-c-25](reader-findings/catalog-c.md#catalog-c-25), [radioclose-02](reader-findings/radio-close.md#radioclose-02), [misc-04](reader-findings/misc.md#misc-04), [sceneetc-11](reader-findings/scene-and-more.md#sceneetc-11) | `a_reader_is_told_each_change_as_it_happens`, `a_reader_is_told_each_change_as_it_happens`, `a_reader_is_told_each_change_as_it_happens` and 3 more |
| `b30770d5` | Tab away from a control whose tooltip is showing stays where Tab put it; a timed-out snackbar no longer pulls focus back. | [dialogs-06](reader-findings/dialogs.md#dialogs-06), [catalog-a-03](reader-findings/catalog-a.md#catalog-a-03), [catalog-c-02](reader-findings/catalog-c.md#catalog-c-02), [chrome-01](reader-findings/chrome.md#chrome-01) | `tabbing_away_from_a_shown_tooltip_keeps_focus_where_tab_put_it`, `tabbing_out_of_a_sticky_tooltip_keeps_focus_where_tab_put_it`, `a_timed_out_snackbar_leaves_focus_where_the_reader_moved_it` |
| `9636094c` | Each move of a menu's highlight is heard; a menu is named after what opened it; a submenu opened from the keyboard stays open. | [menus-01](reader-findings/menus.md#menus-01), [menus-03](reader-findings/menus.md#menus-03), [menus-08](reader-findings/menus.md#menus-08), [collections-05](reader-findings/data-collections.md#collections-05), [collections-m3](reader-findings/data-collections.md#collections-m3), [catalog-b-03](reader-findings/catalog-b.md#catalog-b-03), [tabs-08](reader-findings/tabs.md#tabs-08), [gridview-04](reader-findings/grid-view.md#gridview-04), [text-v03](reader-findings/text.md#text-v03), [docking-10](reader-findings/docking.md#docking-10), [docking-11](reader-findings/docking.md#docking-11), [chrome-17](reader-findings/chrome.md#chrome-17), [misc-07](reader-findings/misc.md#misc-07) | `every_highlight_move_puts_the_item_in_front_of_the_reader`, `the_reader_follows_the_highlight_past_a_hidden_row`, `each_item_says_where_it_stands_in_its_menu` and 4 more |
| `85624a1a` | A control a reader has met is heard again when its page, popup, calendar or scrolled region comes back: it reaches the platform under an id the platform never declared dead. | [dialogs-04](reader-findings/dialogs.md#dialogs-04), [datetime-02](reader-findings/datetime.md#datetime-02), [menus-02](reader-findings/menus.md#menus-02), [menus-04](reader-findings/menus.md#menus-04), [menus-05](reader-findings/menus.md#menus-05), [catalog-a-01](reader-findings/catalog-a.md#catalog-a-01), [catalog-a-M1](reader-findings/catalog-a.md#catalog-a-m1), [catalog-a-M2](reader-findings/catalog-a.md#catalog-a-m2), [catalog-b-01](reader-findings/catalog-b.md#catalog-b-01), [catalog-b-M2](reader-findings/catalog-b.md#catalog-b-m2), [catalog-c-10](reader-findings/catalog-c.md#catalog-c-10), [catalog-c-M1](reader-findings/catalog-c.md#catalog-c-m1), [spinbox-01](reader-findings/spin-box.md#spinbox-01), [password-02](reader-findings/password-field.md#password-02), [tabs-01](reader-findings/tabs.md#tabs-01), [tabs-13](reader-findings/tabs.md#tabs-13), [winintl-01](reader-findings/windows-i18n.md#winintl-01), [winintl-v-01](reader-findings/windows-i18n.md#winintl-v-01), [charts-M1](reader-findings/charts.md#charts-m1), [charts-M2](reader-findings/charts.md#charts-m2), [text-02](reader-findings/text.md#text-02), [docking-01](reader-findings/docking.md#docking-01), [docking-M1](reader-findings/docking.md#docking-m1), [console-07](reader-findings/console.md#console-07), [chrome-07](reader-findings/chrome.md#chrome-07), [chrome-v01](reader-findings/chrome.md#chrome-v01), [sceneetc-03](reader-findings/scene-and-more.md#sceneetc-03) | `a_page_shown_again_comes_back_under_ids_the_adapter_never_removed`, `a_control_scrolled_back_into_view_is_a_live_object`, `a_subtree_hidden_and_shown_again_comes_back_live` and 11 more |
| `b518253f` | A message raised as focus moves ("Moved to 2 of 3") is heard after the new focus, not cut by it. | [collections-04](reader-findings/data-collections.md#collections-04), [catalog-c-27](reader-findings/catalog-c.md#catalog-c-27), [tabs-03](reader-findings/tabs.md#tabs-03), [gridview-10](reader-findings/grid-view.md#gridview-10), [misc-09](reader-findings/misc.md#misc-09) | `a_message_raised_as_focus_moves_is_heard_after_the_move`, `a_message_raised_as_the_current_row_changes_is_heard_after_the_change`, `a_message_waits_for_focus_to_stop_moving` and 6 more |
| `98211359` | In every single-line field the caret, the selection, an empty field's first character and a password's mask reach the reader; no plaintext of a password reaches the bus as it is revealed or hidden. | [datetime-04](reader-findings/datetime.md#datetime-04), [menus-v3](reader-findings/menus.md#menus-v3), [newkit-06](reader-findings/new-widgets-kit.md#newkit-06), [newkit-M1](reader-findings/new-widgets-kit.md#newkit-m1), [spinbox-03](reader-findings/spin-box.md#spinbox-03), [password-01](reader-findings/password-field.md#password-01), [password-10](reader-findings/password-field.md#password-10), [tables-09](reader-findings/tables.md#tables-09), [text-09](reader-findings/text.md#text-09), [text-10](reader-findings/text.md#text-10) | `caret_moves_and_selections_reach_the_reader`, `a_caret_move_after_typing_reaches_the_reader`, `an_empty_field_reports_its_first_and_last_character` and 7 more |
| `c1a553ac` | The Settings page of the widget catalog (any `PrivacySettings` with telemetry) no longer panics in a debug build, and its switches are named by their row. | [catalog-c-01](reader-findings/catalog-c.md#catalog-c-01) | `each_consent_switch_is_named_by_its_row_label`, `a_privacy_settings_row_holds_the_invariants` |
| `a9f25fd0` | After a live language switch a spin box reads, steps and commits in the new language; a special value ("Auto") is heard as such. | [spinbox-02](reader-findings/spin-box.md#spinbox-02), [spinbox-04](reader-findings/spin-box.md#spinbox-04), [spinbox-05](reader-findings/spin-box.md#spinbox-05) | `a_french_decimal_typed_after_a_switch_to_french_is_the_decimal_point`, `focus_a_step_and_leaving_keep_the_language_switched_to`, `a_grouped_number_stays_french_as_focus_arrives_and_steps` and 2 more |
| `2d0446fc` | A date field's calendar opens on the field's date and never writes a date the user did not choose; the months and years views work from the keyboard and a reader; the fields are named. | [datetime-01](reader-findings/datetime.md#datetime-01), [datetime-03](reader-findings/datetime.md#datetime-03), [datetime-06](reader-findings/datetime.md#datetime-06), [datetime-07](reader-findings/datetime.md#datetime-07), [datetime-13](reader-findings/datetime.md#datetime-13), [catalog-b-11](reader-findings/catalog-b.md#catalog-b-11) | `the_months_view_never_commits_the_day_it_hides`, `an_arrow_in_the_months_view_is_a_focus_change_to_the_month`, `enter_on_a_month_shows_its_days_and_escape_goes_back_to_them` and 15 more |
| `de3bb295` | A custom trigger for a dialog, snackbar, wizard or popover is a named Tab stop; a popover with nothing focusable puts the reader on its named dialog, and Tab goes on from there. | [dialogs-01](reader-findings/dialogs.md#dialogs-01), [dialogs-02](reader-findings/dialogs.md#dialogs-02), [dialogs-03](reader-findings/dialogs.md#dialogs-03), [dialogs-v-02](reader-findings/dialogs.md#dialogs-v-02), [catalog-a-11](reader-findings/catalog-a.md#catalog-a-11), [catalog-c-07](reader-findings/catalog-c.md#catalog-c-07), [catalog-c-08](reader-findings/catalog-c.md#catalog-c-08), [catalog-c-11](reader-findings/catalog-c.md#catalog-c-11) | `a_custom_popover_trigger_is_a_tab_stop_heard_as_its_name`, `a_custom_dialog_trigger_puts_focus_on_its_named_button`, `a_custom_button_trigger_opens_its_snackbar_from_the_readers_click` and 9 more |
| `731cc2e1` | Focus in a rich-text editor, code editor, plain-text editor or log view lands on its text, which can be named (`.label(..)`). | [catalog-b-02](reader-findings/catalog-b.md#catalog-b-02), [text-01](reader-findings/text.md#text-01), [text-07](reader-findings/text.md#text-07), [console-01](reader-findings/console.md#console-01), [console-09](reader-findings/console.md#console-09), [sceneetc-05](reader-findings/scene-and-more.md#sceneetc-05) | `focus_on_a_composite_is_published_on_the_node_that_stands_for_it`, `a_focus_request_on_the_proxy_focuses_the_composite_it_stands_for`, `the_context_menu_the_composite_owns_is_offered_on_its_proxy` and 6 more |
| `c515e521` | Ctrl+Tab and Ctrl+Shift+Tab leave a code or plain-text editor, and the editor tells a reader so. | [console-02](reader-findings/console.md#console-02) | `ctrl_tab_leaves_the_editor_and_writes_nothing`, `ctrl_shift_tab_leaves_the_editor_backwards_and_dedents_nothing`, `the_editor_tells_a_reader_how_to_leave_it` and 2 more |
| `f9ffa98c` | Each combo box option is spoken as the arrows reach it, and only Enter commits it. | [menus-07](reader-findings/menus.md#menus-07), [menus-14](reader-findings/menus.md#menus-14), [menus-18](reader-findings/menus.md#menus-18), [catalog-a-09](reader-findings/catalog-a.md#catalog-a-09), [winintl-04](reader-findings/windows-i18n.md#winintl-04), [misc-05](reader-findings/misc.md#misc-05), [misc-22](reader-findings/misc.md#misc-22) | `each_option_is_spoken_as_the_arrows_reach_it`, `a_long_list_is_spoken_option_by_option_too`, `a_long_list_is_one_list_box_of_named_options` and 8 more |
| `0e2b6377` | A new toast is heard without the others being read again; focus stays where it was; a focused toast does not expire; the bell and log keep focus. | [toast-01](reader-findings/toast.md#toast-01), [toast-02](reader-findings/toast.md#toast-02), [toast-04](reader-findings/toast.md#toast-04), [toast-06](reader-findings/toast.md#toast-06), [toast-07](reader-findings/toast.md#toast-07), [toast-v1](reader-findings/toast.md#toast-v1), [toast-v2](reader-findings/toast.md#toast-v2), [catalog-c-13](reader-findings/catalog-c.md#catalog-c-13), [catalog-c-14](reader-findings/catalog-c.md#catalog-c-14) | `a_rebuild_restores_focus_into_the_innermost_ancestor_that_survived`, `a_new_toast_is_heard_and_the_toasts_already_shown_are_not`, `an_expiring_toast_leaves_the_others_unannounced` and 10 more |
| `8448bb9a` | A grid view reader stays on its tile when the selection changes, and hears the count in words. | [gridview-01](reader-findings/grid-view.md#gridview-01), [gridview-02](reader-findings/grid-view.md#gridview-02), [gridview-03](reader-findings/grid-view.md#gridview-03) | `an_assistive_focus_on_a_tile_leaves_the_keyboard_on_the_grid`, `a_selection_change_keeps_the_readers_tile_and_changes_its_state`, `the_count_is_said_in_words_with_no_translations_installed` and 4 more |
| `e7764b0f` | A screen reader can no longer click or focus the page behind an in-tree modal. | [dialogs-08](reader-findings/dialogs.md#dialogs-08), [radioclose-M1](reader-findings/radio-close.md#radioclose-m1) | `an_at_click_behind_a_modal_does_nothing`, `an_at_focus_request_behind_a_modal_leaves_focus_in_it`, `what_opens_over_the_modal_takes_requests` and 3 more |
| `27022d39` | A context menu opened with Shift+F10 or the Menu key acts where the caret is; closing a field's own menu does not select the whole field. | [text-v01](reader-findings/text.md#text-v01), [text-v02](reader-findings/text.md#text-v02) | `shift_f10_leaves_the_caret_where_the_reader_put_it`, `shift_f10_keeps_the_selection_the_menu_acts_on`, `shift_f10_leaves_the_caret_where_the_reader_put_it` and 5 more |
| `70183c50` | The chosen segment of a segmented control is read as checked. | [catalog-a-10](reader-findings/catalog-a.md#catalog-a-10), [charts-08](reader-findings/charts.md#charts-08), [sceneetc-12](reader-findings/scene-and-more.md#sceneetc-12) | `the_reader_hears_the_selected_segment_checked`, `a_segmented_control_raises_no_selection_changed` |

## What stays open

In Teksilo itself 265 findings stay open or partly fixed: 6 critical, 84 high, 102 medium, 73 low.

The critical and high ones in Teksilo itself, by what the reader lacks. Each
id links to its entry.

- **No route by keyboard or screen reader.** Lightweight scene items cannot be
  focused, selected or moved ([catalog-b-04](reader-findings/catalog-b.md#catalog-b-04), [sceneetc-01](reader-findings/scene-and-more.md#sceneetc-01)), nor can the
  previewer's navigator rows be activated ([sceneetc-02](reader-findings/scene-and-more.md#sceneetc-02)); a cross-view drag
  ([misc-08](reader-findings/misc.md#misc-08)), a tab moved to another group ([tabs-09](reader-findings/tabs.md#tabs-09)), a table's sorting,
  filters and column changes ([tables-05](reader-findings/tables.md#tables-05)), the tab strip's overflow list
  ([tabs-07](reader-findings/tabs.md#tabs-07)), the notification log's entries ([toast-12](reader-findings/toast.md#toast-12), [catalog-c-16](reader-findings/catalog-c.md#catalog-c-16))
  and the colour swatch grids ([catalog-b-07](reader-findings/catalog-b.md#catalog-b-07), [misc-12](reader-findings/misc.md#misc-12), [misc-13](reader-findings/misc.md#misc-13)) need a
  pointer; the magnet connect flow gives no feedback ([sceneetc-07](reader-findings/scene-and-more.md#sceneetc-07)).
- **A message that never reaches speech.** A snackbar, banner or toast is
  read by its title and not its message ([dialogs-05](reader-findings/dialogs.md#dialogs-05), [catalog-a-13](reader-findings/catalog-a.md#catalog-a-13),
  [toast-03](reader-findings/toast.md#toast-03)); the ColorPicker's "Color changed" ([misc-02](reader-findings/misc.md#misc-02),
  [catalog-b-06](reader-findings/catalog-b.md#catalog-b-06)), `LogView::announce_appends` ([console-08](reader-findings/console.md#console-08)) and the
  terminal's output ([console-04](reader-findings/console.md#console-04), [console-05](reader-findings/console.md#console-05), [console-V1](reader-findings/console.md#console-v1)) are not
  announced; a wizard's step ([catalog-a-M3](reader-findings/catalog-a.md#catalog-a-m3), [catalog-a-M4](reader-findings/catalog-a.md#catalog-a-m4)), the outcome of
  a shortcut rebind ([chrome-14](reader-findings/chrome.md#chrome-14), [chrome-15](reader-findings/chrome.md#chrome-15)), the unread count
  ([toast-10](reader-findings/toast.md#toast-10), [catalog-c-15](reader-findings/catalog-c.md#catalog-c-15)) and a date segment's step ([datetime-05](reader-findings/datetime.md#datetime-05)) are
  silent. Messages raised as focus arrives that are not the announcer's, a
  field's validation message or a message box's own live region, are still
  cut ([password-03](reader-findings/password-field.md#password-03), [password-05](reader-findings/password-field.md#password-05), [toast-05](reader-findings/toast.md#toast-05)).
- **State a reader cannot see.** A list's cursor without a selection
  ([collections-01](reader-findings/data-collections.md#collections-01), [catalog-c-26](reader-findings/catalog-c.md#catalog-c-26)), a row's check box ([collections-03](reader-findings/data-collections.md#collections-03)),
  a table's name, cursor and selection ([tables-01](reader-findings/tables.md#tables-01), [tables-03](reader-findings/tables.md#tables-03)), a chart's
  selected datum and its legend's actions ([charts-01](reader-findings/charts.md#charts-01), [charts-02](reader-findings/charts.md#charts-02)), a spin
  box's unit ([spinbox-06](reader-findings/spin-box.md#spinbox-06)), Caps Lock where it was on at launch
  ([password-04](reader-findings/password-field.md#password-04)) and on macOS at all ([password-v01](reader-findings/password-field.md#password-v01)), and an accordion's
  content, which sits inside its button where Orca's flat review does not
  look ([dialogs-v-01](reader-findings/dialogs.md#dialogs-v-01), [catalog-c-18](reader-findings/catalog-c.md#catalog-c-18)).
- **No name.** The ColorPicker's channels ([catalog-b-05](reader-findings/catalog-b.md#catalog-b-05), [misc-03](reader-findings/misc.md#misc-03)),
  ColorEdit ([misc-16](reader-findings/misc.md#misc-16), [catalog-b-M1](reader-findings/catalog-b.md#catalog-b-m1)), the SceneView ([sceneetc-09](reader-findings/scene-and-more.md#sceneetc-09),
  [sceneetc-04](reader-findings/scene-and-more.md#sceneetc-04)), the dock resize handles ([docking-05](reader-findings/docking.md#docking-05)), ShortcutSettings'
  buttons ([chrome-13](reader-findings/chrome.md#chrome-13)), InputDialog's field ([newkit-03](reader-findings/new-widgets-kit.md#newkit-03)), the previewer's
  knobs ([sceneetc-14](reader-findings/scene-and-more.md#sceneetc-14)), a composite tooltip read as "Tooltip"
  ([catalog-a-04](reader-findings/catalog-a.md#catalog-a-04)); names left in English in a French interface
  ([catalog-b-09](reader-findings/catalog-b.md#catalog-b-09)).
- **Focus lost or thrown after an action.** Hiding or moving a dock
  ([docking-02](reader-findings/docking.md#docking-02), [docking-04](reader-findings/docking.md#docking-04), [docking-06](reader-findings/docking.md#docking-06), [docking-M2](reader-findings/docking.md#docking-m2)), pinning or
  removing a recent project ([misc-10](reader-findings/misc.md#misc-10)), a snackbar timing out ([dialogs-06](reader-findings/dialogs.md#dialogs-06),
  [dialogs-07](reader-findings/dialogs.md#dialogs-07)), a banner dismissed ([newkit-01](reader-findings/new-widgets-kit.md#newkit-01)), Enter on a corkboard card
  ([sceneetc-06](reader-findings/scene-and-more.md#sceneetc-06)), Alt+letter on a collapsed menu bar ([chrome-16](reader-findings/chrome.md#chrome-16)).
- **Text without its structure.** Rich text runs its paragraphs together and
  drops lists, links and formatting ([text-03](reader-findings/text.md#text-03), [text-04](reader-findings/text.md#text-04), [text-05](reader-findings/text.md#text-05),
  [text-06](reader-findings/text.md#text-06), [catalog-b-10](reader-findings/catalog-b.md#catalog-b-10)); terminal rows are glued ([console-03](reader-findings/console.md#console-03)) and its
  way out is published only as a shortcut no adapter exports ([console-06](reader-findings/console.md#console-06));
  a wrapped Arabic paragraph's text is garbled ([winintl-02](reader-findings/windows-i18n.md#winintl-02)).
- **Other.** A tab move that did not happen is announced as done
  ([tabs-02](reader-findings/tabs.md#tabs-02)); a registry tooltip is not heard on the first visit
  ([chrome-02](reader-findings/chrome.md#chrome-02)); a live chart floods speech ([charts-03](reader-findings/charts.md#charts-03)); a close asked by
  the compositor shows no confirmation until later input ([radioclose-01](reader-findings/radio-close.md#radioclose-01));
  SearchField's suggestions are silent ([newkit-02](reader-findings/new-widgets-kit.md#newkit-02)).

Partly fixed, with the part left open in each entry: [dialogs-04](reader-findings/dialogs.md#dialogs-04) (the
popover's own focus), [catalog-b-03](reader-findings/catalog-b.md#catalog-b-03) and [chrome-17](reader-findings/chrome.md#chrome-17) (one Tab stop per menu
bar item), [collections-m3](reader-findings/data-collections.md#collections-m3) (context menus stay unnamed), [text-v03](reader-findings/text.md#text-v03),
[datetime-06](reader-findings/datetime.md#datetime-06), [datetime-07](reader-findings/datetime.md#datetime-07), [toast-02](reader-findings/toast.md#toast-02), [toast-07](reader-findings/toast.md#toast-07), [sceneetc-05](reader-findings/scene-and-more.md#sceneetc-05), and
[radioclose-M1](reader-findings/radio-close.md#radioclose-m1): a modal that opens as a native window, the default on
Windows and macOS, still lets a screen reader act on the window behind it.

In the examples' own code, the most common open finding is a control the
example leaves unnamed; two are critical or near it: the menus example's
context menus cannot be reached by keyboard ([menus-11](reader-findings/menus.md#menus-11)), and the native-menu
example's in-window commands do nothing on Linux ([chrome-18](reader-findings/chrome.md#chrome-18)).

## Upstream

The sweep traced 66 findings to code outside Teksilo. None has been
reported upstream yet: publishing an issue is a decision for the project, so
this is the list to report from. Where Teksilo can work round one, the entry
says so; the node-id fix above is such a work-round.

| Component | What it does | Findings |
|---|---|---|
| `accesskit_atspi_common` 0.20, `adapter.rs:90-111` | A node that leaves the tree is announced defunct, and the same id added again is never announced alive, so libatspi and Orca drop it for good. Teksilo now hands a returning node a new id. | catalog-a-01, catalog-a-M1, catalog-a-M2, spinbox-01, winintl-01, sceneetc-03, tabs-13 |
| `accesskit_unix` 0.23, `atspi/bus.rs:434-473` | The `Cache` `AddAccessible` / `RemoveAccessible` signals go out with a flattened body, which libatspi 2.52 refuses ("unknown signature"), so a client never learns of a removal. | catalog-b-M3, password-v03, winintl-v-03 |
| `accesskit_atspi_common` 0.20, `node.rs:300-386` (`state`) | No `EXPANDABLE`/`EXPANDED`, no `HAS_POPUP`, no `INVALID_ENTRY`; a disabled node is still `ENABLED` and `SENSITIVE` (`node.rs:376-380`). | menus-12, menus-17, catalog-a-05, catalog-a-06, collections-06, catalog-c-20, chrome-08, chrome-09, docking-07, dialogs-11, password-07, password-09, tabs-11, spinbox-08, tables-04, misc-19 |
| `accesskit_atspi_common` 0.20, `node.rs:650-658` | AT-SPI's `Value` interface carries a number only; a combo box's current text value reaches no AT-SPI client. Teksilo could publish it as the combo box's text. | menus-06, catalog-a-07, winintl-03, misc-06, text-14 |
| `accesskit_atspi_common` 0.20, `node.rs:959-977` | Relations: no `NODE_CHILD_OF` (tree level), `FLOWS_TO` or `MEMBER_OF` (radio group). | collections-m2, radioclose-05, sceneetc-08 |
| `accesskit_atspi_common` 0.20, `node.rs:415-436`, `494-521` | No `Table`/`TableCell` interfaces and no table attributes, so no header, row, column or size is spoken, and Orca treats a grid as a layout table. | tables-02, tables-04, gridview-09, datetime-10 |
| `accesskit_atspi_common` 0.20, `node.rs:532-545` | One action only (`click`): custom actions, Expand/Collapse and steps are not offered. | misc-25, gridview-11, sceneetc-27 |
| `accesskit_atspi_common` 0.20, `node.rs:1097` | No keyboard shortcut or access key. | catalog-b-19, chrome-25 |
| `accesskit_atspi_common` 0.20, `Selection` | The selection interface counts no selected child where the selected rows or tiles sit below a group. | tables-15, gridview-08 |
| `accesskit_atspi_common` 0.20, `adapter.rs:184-195`, `287-288` | Text changes are emitted for nodes outside the exported tree; Orca drains them as dead objects and lags. | spinbox-11, misc-24, winintl-15 |
| `accesskit_atspi_common` 0.20, `adapter.rs:78-80` | A selected item in newly added content raises a selection event that moves Orca's locus of focus off the real focus. | catalog-a-12, catalog-c-06, chrome-24, chrome-v03 |
| `accesskit_unix` 0.23, `interfaces/accessible.rs:64-67` | The object's `Locale` is always empty; the language is only a text attribute Orca 46.1 does not use. | winintl-16 |
| Orca 46.1 | Its own choices: a virtualized list's position counts only the realized rows; a password field's placeholder, a splitter's value and a tooltip resembling the button's name are not spoken; a rail activity switch is announced only after Space. | collections-09, password-v05, catalog-a-16, chrome-10, docking-09, chrome-23, docking-16, spinbox-v02 |
| libatspi 2.52 | In a Wayland session it gives Orca its legacy keyboard device, which hears only keys the application reports, and does not use KWin's `KeyboardMonitor`. Teksilo now reports keys. | text-15 |

## What was not measured

- **Windows and macOS.** Nothing was run there. What NVDA, JAWS, Narrator or
  VoiceOver get is read from the adapters' source only, and where the
  adapters differ from AT-SPI the entries say so. Two differences that bear
  on the fixes: `accesskit_windows` raises an announcement as
  `LiveRegionChanged` with no text of its own (`adapter.rs:256-263`), so a
  screen reader reads the node's name; `accesskit_macos` carries the
  announcement's text (`event.rs:65-99`). The key report
  (`teksilo_platform::key_report`) does nothing on either: NVDA and VoiceOver
  read the keyboard themselves.
- **Orca reading the keyboard itself.** On an X11 session, or with a libatspi
  that uses KWin's `org.freedesktop.a11y.KeyboardMonitor`, Orca hears keys
  without the application; neither was run. On this Wayland session Orca
  hears exactly the keys the application reports.
- **Other versions**: Orca before or after 46.1, another libatspi, GNOME's
  compositor. **Braille**: off. **Orca's own language**: where a distribution
  installs Orca's translations under `/usr/share/locale-langpack`, Orca 46.1
  stays English whatever `LANG` says, so every Orca line quoted here is
  English.
- **Pointer, touch and pen.** Acts were keys, AT-SPI actions and focus
  requests. A pointer was moved only where a hover mattered.
- **Timing on another machine.** A speech cut is estimated from Orca's log
  (see the guide); a race lost here may be won on a faster machine.
- **Three examples, whole or in part.** `telemetry-codegen` opens no window.
  `telemetry-teksilo` does not build: the workspace excludes it while its
  manifest inherits its version and dependencies from the workspace, its
  adapter crate pins `teksilo-core` and `teksilo-telemetry` at 0.6.2 against
  the tree's 0.13.1, and it expects `teksilo-collector-proto` outside the
  repository. `web-view-demo` was swept around its web view only: the private
  session has no `DISPLAY`, and the engine cannot embed under Wayland, so the
  page's own tree, and going into and out of it, were not observed.
  `automation_bridge_smoke` was swept with `--serve`; without it, it exits
  after its own smoke test.

## What the sweep found in the harness

The sweep's agents recorded 222 notes on the harness itself. They came down
to a handful of defects, all fixed in the harness before it landed: a key
client per key that made the seat flap until winit panicked (now one client
for a run); Orca's speech credited by the clock when its loop stalled for
seconds (now credited to the event that caused it, with a bounded wait for an
application that never goes quiet); speech from the settle before an act
credited to the act; a stopped run reported as a pass; a tree read from
libatspi's stale cache; an observation raised on every overlay close for a
focus loss Orca drops harmlessly; and a module that did not import taking the
whole scenario registry down with it.

## The findings, by example

- [Dialogs and popovers](reader-findings/dialogs.md): 15 findings, 1 critical, 10 high, 2 medium, 2 low
- [Date and time pickers](reader-findings/datetime.md): 20 findings, 3 critical, 5 high, 7 medium, 5 low
- [Menus and drop-downs](reader-findings/menus.md): 22 findings, 3 critical, 8 high, 7 medium, 4 low
- [Toasts and the notification log](reader-findings/toast.md): 19 findings, 1 critical, 10 high, 5 medium, 3 low
- [Lists and trees](reader-findings/data-collections.md): 18 findings, 9 high, 6 medium, 3 low
- [Widget catalog, first eight tabs](reader-findings/catalog-a.md): 28 findings, 3 critical, 12 high, 8 medium, 5 low
- [Widget catalog, middle eight tabs](reader-findings/catalog-b.md): 24 findings, 4 critical, 11 high, 8 medium, 1 low
- [Widget catalog, last six tabs](reader-findings/catalog-c.md): 37 findings, 1 critical, 14 high, 17 medium, 5 low
- [New widgets kit](reader-findings/new-widgets-kit.md): 14 findings, 5 high, 4 medium, 5 low
- [Spin boxes](reader-findings/spin-box.md): 13 findings, 2 critical, 3 high, 3 medium, 5 low
- [Password field](reader-findings/password-field.md): 19 findings, 8 high, 5 medium, 6 low
- [Tabs](reader-findings/tabs.md): 18 findings, 1 critical, 5 high, 7 medium, 5 low
- [Tables](reader-findings/tables.md): 19 findings, 5 high, 9 medium, 5 low
- [Grid view](reader-findings/grid-view.md): 17 findings, 1 critical, 5 high, 5 medium, 6 low
- [Radio tiles and close confirmation](reader-findings/radio-close.md): 7 findings, 3 high, 1 medium, 3 low
- [Windows and languages](reader-findings/windows-i18n.md): 22 findings, 2 critical, 4 high, 6 medium, 10 low
- [Charts](reader-findings/charts.md): 16 findings, 6 high, 5 medium, 5 low
- [Rich text and input methods](reader-findings/text.md): 22 findings, 2 critical, 12 high, 4 medium, 4 low
- [Docking](reader-findings/docking.md): 20 findings, 3 critical, 7 high, 5 medium, 5 low
- [Terminal, log view and code editor](reader-findings/console.md): 21 findings, 2 critical, 8 high, 5 medium, 6 low
- [Tooltips, menu bars and window chrome](reader-findings/chrome.md): 34 findings, 1 critical, 11 high, 13 medium, 9 low
- [Pickers, files and drag and drop](reader-findings/misc.md): 27 findings, 2 critical, 13 high, 8 medium, 4 low
- [Scenes, animation and the rest](reader-findings/scene-and-more.md): 32 findings, 2 critical, 13 high, 9 medium, 8 low
- [The examples swept last](reader-findings/remaining.md): 10 findings, 5 medium, 5 low
