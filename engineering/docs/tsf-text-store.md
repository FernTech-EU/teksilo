<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# A TSF text store on Windows: design

Status: a design. Phase -1 (7.1) is implemented; the store is not. Revised on 2026-10-28 after an independent review, against
`feat/system-text-input` at `4b1f697a` and the eight rounds of
`cross_tasks/system-text-input-windows-findings.md` (Windows 11 22621, French UI and AZERTY;
Japanese, Pinyin and Korean from round 4). The first version (`ee8716db`) knew three rounds.

What changed: the lock and notification model follows `RequestLock`'s contract (4.7, 4.10); reads
get a side-effect-free probe and writes a batch bracket (3, 4.9); focus moves in `settle_ime`, not at
the end of the turn (4.6); keys follow Chromium's model and eaten keys reach the Alt-tap state
(4.3); messages delivered during TSF calls have their own rules (4.4); the platform-neutral half is
a section of its own (3); the plan, the size and the measurements are redone (7); the open questions
are answered (8); where this document departs from the review is listed in 9.

Marks: what a text service or Windows will do and nobody has measured is **to be measured**, with an
id (M1 to M14, 7.2). An API fact this design rests on is checked against Microsoft's documentation
or a production store (Chromium `ui/base/ime/win`, Firefox `widget/windows`, NVDA's `tsf.cpp`), and
the source is named where it decides something.

## 1. The problem and the evidence

winit 0.30 reads the text input system through IMM32 only. Windows 11 serves such a window every
text service through CUAS, TSF's emulation of IMM32, in a reduced form.

- **Voice typing (Win+H) and the emoji panel (Win+.) arrive identically**: one `WM_IME_COMPOSITION`
  with `GCS_RESULTSTR` alone (0x800) between a start and an end, no preedit (rounds 1 to 3: 9 emoji,
  5 phrases of 74 characters, then 2 phrases and 3 picks). Flags alone do not separate them from an
  IME: Korean syllables and Pinyin punctuation also commit with 0x800 (round 4). What holds for every
  commit seen: the panel and voice typing commit with no `GCS_COMPSTR` in the same session and no
  `VK_PROCESSKEY` since the previous commit; every IME commit had one of the two.
- **Nothing in TSF tells voice from emoji.** At five commits in round 3 the compartments (speech,
  handwriting, keyboard, at the global, thread, document and context levels), the profile and the
  foreground window were byte for byte identical. The Japanese, Pinyin and Korean IMEs all report the
  same composition owner, `{1713DD5A-68E7-4A5B-9AF6-592A595C778D}`, which is the value of
  `GUID_PROP_INPUTSCOPE`, not any TIP's CLSID (rounds 2, 4).
- **What the user gets**: no interim text (phrases land whole, about every 6 s, where Notepad shows
  words as they are spoken); spoken commands typed as text ("corriger test", "annuler ça"); a spoken
  full stop after a space ("tester . "); Windows's flyout *Certaines fonctionnalités de saisie vocale
  risquent de ne pas fonctionner dans cette application*; and no `EditSource::Dictation` (round 3:
  67 characters, all IME).
- **The window's context is CUAS's transitory one** (static `0x4 [TRANSITORY]`, a document
  compartment `TRANSITORYEXTENSION_PARENT`, which Microsoft documents as the parent document manager
  of a transitory document). This is probably **not** the cause of the symptoms: Chromium's
  production store also reports `TS_SS_TRANSITORY` (`tsf_text_store.cc` `GetStatus`, with a TODO to
  remove it for Korean reconversion), and voice typing is reported to work fully in Chrome (a user
  forum, not verified). The likely cause is "no store at all". Phase 0 judges by the symptoms, not
  the flag; this store does not set `TS_SS_TRANSITORY` (Firefox does not either).
- **TSF calls deliver messages.** A TSF call made from the STA waits in a modal loop that delivers
  the messages other threads send meanwhile. In round 4 the trace's compartment probe, run before
  winit had the message, let the next Korean syllable's start and composition reach winit before the
  commit, which lost the commit (fixed in `2aaae965`). A store makes such calls constantly (4.4).

Since the first version the IMM path also changed: a composition can replace text
(`ImeComposition::replacement`, `b5aa2835`, `506b5de7`); a click or focus change keeps the
composition's text in place and ends it through `finish_composition`, which on Windows was
`ImmNotifyIME(NI_COMPOSITIONSTR, CPS_CANCEL)` (`df05fb36`, `6f2d84fc`, passing in rounds 6 and 7,
with the Korean IME committing on mouse-down by itself) and now completes it with `CPS_COMPLETE`,
so the field keeps what the input method commits, as Notepad does (Pinyin's "nihao" for "ni'hao",
round 12): a temporary subclass reads the result before winit, and the application drops winit's
late copy of it (`text_input_hook/imm_completion.rs`); typing batched before a click goes in where
it was typed (`05e91aa1`); only Alt pressed alone opens the menu bar (`8db5bdce`, `7d70a074`, rounds
5 and 8). One IMM bug stays open: Ctrl+Z past an IME commit replays the composition's intermediate
states as text (round 6). The store path must not inherit it.

## 2. What a TSF-aware application gets

| Feature | IMM-only today | With a store (expected) | Teksilo wants |
|---|---|---|---|
| Interim dictation text | none | the speech service composes its hypothesis and rewrites it (M1) | yes |
| Spoken commands (select, delete, correct) | typed as text | reads, `SetSelection`, `SetText` (M1, French and English) | yes |
| The "some features may not work" flyout | shown | gone (M1) | yes |
| Dictation reported as dictation | impossible | maybe; no production store does it (5) | yes |
| Reconversion (Japanese 変換, Korean Hanja on a committed syllable) | `IMR_RECONVERTSTRING` unanswered | a TIP reads the selection and composes over it; ATOK and Mozc still ask `WM_IME_REQUEST` under TSF (Chromium) | yes |
| Candidate placement | one rectangle, `IMR_QUERYCHARPOSITION` unanswered (round 5 counted 320) | `GetTextExt` per range | yes |
| Emoji panel | works; a pick is lost with no text field | inserts through the store (M5) | keep |
| Input scope (password, number, URL) | none: winit's `set_ime_purpose` is a no-op | `GUID_PROP_INPUTSCOPE` | yes |
| Touch keyboard autocorrect, prediction, hardware-keyboard suggestions | off, presumably | per field (8, Q4; M7) | opt in |

## 3. Layering: the half that is not Windows-specific

Every text protocol Teksilo targets is pull-and-write, with an edit bracket and a duty on the
application to report its own changes: TSF (reads and writes under a lock, notifications outside
it, UTF-16), macOS `NSTextInputClient` (bracketed by the key event, UTF-16), Android
`InputConnection` (`beginBatchEdit`/`endBatchEdit`), iOS `UITextInput` (the `inputDelegate`
bracket), Wayland text-input-v3 (push only, UTF-8, `done` batches; reachable from winit 0.31's
surrounding-text request and `Ime::DeleteSurrounding`, not from 0.30, whose seat object winit owns).
So most of this design belongs below the platforms.

**teksilo-core**, public API (CONTRIBUTING rule 4: the maintainer decides each item):

- the read half, `TextInputDocument` (exists), plus `is_available()`, which answers whether the
  document can answer now and changes nothing, and `flush_pending_input()`, which inserts batched
  typing and is called only where a change may be reported. Today every read of the rich editor
  flushes the batch first (`rich_text/input_document.rs`, `with_typing_inserted`) and fires its
  change callbacks, so no read may be the probe;
- the write half, each method with a default body that refuses (a custom implementer keeps working;
  its field reads as read-only to the store, or takes the IMM path): replace a range (with a dry run,
  for `QueryInsert` and `TS_IAS_QUERYONLY`, and a correction flag, for `TS_ST_CORRECTION`), set the
  selection, mark a range of document text as the composition (with styled segments: TSF's clauses,
  macOS marked-text underlines, Android composing spans), end the composition with an origin;
- the batch bracket, `begin_input_batch` and `end_input_batch`: inside it a widget changes only its
  own state, and every effect application code can observe (change callbacks, `on_text_inserted`,
  `on_change`, signal writes) is queued and runs at the end. It is TSF's lock, Android's batch edit,
  iOS's bracket and Wayland's `done`, and the undo grouping point;
- the change journal: a text revision, the changed range since a revision (start, old end, new end),
  selection and layout revisions, and each change's cause (the input system, or anything else). The
  store needs the cause not to echo its own writes; Wayland needs it for `set_text_change_cause`.

**teksilo-platform, pure and tested on Linux**, beside `answers.rs`, `composition.rs`
(`MarkedReplacement`), `dictation.rs` (`DictationSession`), `key_press.rs` (`KeyPress`) and
`winit_ime.rs`, which already are: the lock and batch arbiter (grant, queue coalesced to one
pending grant, upgrade, defer while busy, refuse a synchronous request while changes are
unreported); change coalescing into (start, old end, new end); composition tracking (owner, range,
the write that ended it) feeding `classify`; the key-trace verdict (a key TSF processed, as a
`TakenKey`), the Windows counterpart of `key_press.rs`; a UTF-8 surrounding-window builder for
Wayland and Android later.

**Windows only**: the COM objects (`ITextStoreACP`, `ITfContextOwnerCompositionSink`,
`ITfTextEditSink`, `ITfKeyTraceEventSink`), the thread manager's lifetime, the focus calls, the empty
context, compartments, input scopes, `TS_STATUS` flags, property reading, DPI-to-screen conversion,
`WM_IME_REQUEST` answers, IMM coexistence.

**macOS against this**: it already answers from the read half through `answers.rs` and posts its
commits. It moves its `insertText:` outside a key press onto the write half once the Windows write
half has settled (Q7), which also fixes AppKit asking `selectedRange` right after an insertion that
was only posted. `setMarkedText:` and `keyDown:` stay winit's while winit's key logic depends on its
own marked text. `MarkedReplacement` is macOS's composition over a range, and `DictationSession` its
attribution signal; Windows has no equivalent of the second yet (5).

## 4. Architecture on Windows

### 4.1 Where it lives

`text_input_hook/windows.rs` gains a `windows/` directory: `trace.rs` (today's tap, still behind
`TEKSILO_TEXT_INPUT_TRACE`), `thread.rs` (thread manager, client id, sinks, per-window state),
`store.rs` (the COM objects, `#[implement]` as in `external_dnd/windows.rs`), `imm.rs` (the
`WM_IME_REQUEST` answers and `finish_composition`). teksilo-app: focus switching and per-message
notification next to `settle_ime` (4.6, 4.10), the deferred-grant payload (4.7), the
termination before a press (4.11), the key-trace verdict in the key path (4.3).

### 4.2 One path per focus, never two

winit 0.30.13: at creation it calls its own `set_ime_allowed(false)`
(`ImmAssociateContextEx(hwnd, 0, IACE_CHILDREN)`; that this also removes the top level's context is
measured, scenario 5, not documented); `set_ime_allowed(true)` restores the default context;
`WM_IME_COMPOSITION` is read only while IME is allowed; `set_ime_purpose` does nothing.

Rule, decided in `input_loop::settle_ime`, which already decides `set_ime_allowed`:

- **Store path**: the focused surface has a document and the active input method is a TIP or a
  keyboard layout. winit's IME stays disallowed, so no IMM context and no `Ime` event from winit.
  `WM_IME_ENDCOMPOSITION` is still handled when winit was in preedit, which is safe only because
  `ImmGetContext` then returns null; `finish_composition` runs before any switch.
- **IMM path**: a surface without a document (the terminal; `CodeEditor` has had one since Phase 4), a store that could not
  start, or an active legacy IMM-IME (a keyboard layout whose HKL `ImmIsIME` reports; the trace
  already reads the profile type and HKL), which works only through an IMM context.

Neither production store disassociates the IMM context: Firefox keeps it associated by default
(`WinIMEHandler.cpp`, `NeedsToAssociateIMC`) and reads IMM messages only while an IMM-IME is active,
and Chromium's TSF files never call `ImmAssociateContext*`. The configuration chosen here is the
untrodden one, so M10 tests it; if a TIP misbehaves without a context, the fallback is the
browsers': keep it associated and drop winit's `Ime` events in teksilo-app while the store path is
live. Whether CUAS still serves the IMM path once the thread is activated is M2.

### 4.3 Keys

Recommended for Phase 0: **Chromium's model**. Chromium calls `ITfThreadMgr::Activate`, never
`ITfKeystrokeMgr` and never `ITfMessagePump`, lets TSF process keys itself, and learns which keys a
TIP processed from `ITfKeyTraceEventSink` (`OnKeyTraceDown`/`OnKeyTraceUp`) advised on the thread
manager. It needs no winit hook. Firefox's model (`ITfKeystrokeMgr::TestKeyDown`/`KeyDown` for
`WM_KEYDOWN` and `WM_KEYUP` only, and a message loop through `ITfMessagePump`) works too; with winit
it would be a `with_msg_hook` callback (`PeekMessageW`, then the hook, then `TranslateMessage` and
`DispatchMessageW`; no message pump possible). It stays the variant to try if a TIP misbehaves (M3).

Whichever model: **an eaten key must reach the Alt-tap state.** The bare-Alt menu decides from the
keys winit reports between Alt down and up (`input_routing.rs`, `key_cancels_alt_tap`). Alt+` (the
Japanese IME's toggle, `VK_KANJI` as `WM_SYSKEYDOWN`) eaten without a trace gives winit Alt down, Alt
up and nothing between: round 4's bug (focus jumps to the "Edit" menu) returns. An eaten Alt key-up
leaves `alt_down` set, which drives the mnemonic underline. So the key-trace verdict feeds
`key_taken_by_input_method` (`TakenKey`, macOS only today) on Windows, and the Alt-tap state counts a
taken key as a key. What winit reports for a key TSF processed in Chromium's model (`VK_PROCESSKEY`,
or nothing) is M9, as are a key-down eaten with its key-up not (winit's `KeyEventBuilder` peeks the
queue) and `WM_SYSKEYDOWN`. winit's `ModifiersChanged` corrects itself from `GetKeyState` at the next
key or mouse message; what is lost for good is the release event and state built on it.

### 4.4 Messages delivered during TSF calls

Round 4 measured it: an outgoing TSF call can deliver messages sent to the window meanwhile. Three
cases, three rules:

1. **Before winit has the current message** (a subclass procedure ahead of `DefSubclassProc`): no
   TSF call, ever. Messages delivered there overtake the one in hand (round 4).
2. **Inside winit's handling** (any teksilo-app callback): allowed. winit's runner buffers the events
   of messages delivered while its handler runs (`event_loop/runner.rs`, `send_event`) and hands them
   over after, in order; `finish_composition` calls `ImmNotifyIME` from there, measured in rounds 6
   and 7. Widgets must not hold a borrow across the call.
3. **While no handler runs** (inside a TSF callback reached from the message loop): a delivered
   message reaches the application at once, so a nested dispatch can change the document during a
   lock. The store compares the revision at the end of each lock and notifies then (4.7).

Real outgoing pumping calls are TSF's own (`SetFocus`, `AssociateFocus`, `Push`/`Pop`,
`TerminateComposition`, sink notifications), `ITipInvocation::Toggle` (cross-process,
`soft_keyboard.rs`) and `DoDragDrop`. The clipboard is not one (arboard, Win32 `OpenClipboard`, no
COM), and UIA calls are incoming.

### 4.5 Thread, apartment, lifetime

- The UI thread, an STA (winit's or `external_dnd`'s `OleInitialize`). The store never initialises
  COM; with another apartment it does not start and the IMM path stays.
- First attach: `TF_GetThreadMgr`, else `CoCreateInstance`; `ITfThreadMgrEx::ActivateEx(&client_id,
  0)` (no `TF_TMAE_UIELEMENTENABLEDONLY`: TIPs draw their own candidates; Chromium uses plain
  `Activate`). Activating CUAS's existing manager is ref-counted; what it changes for the thread's
  other windows is M2.
- `Deactivate` explicitly when the event loop ends (teksilo-app knows every way out), not in a
  thread-local destructor: winit's `COM_INITIALIZED` thread-local calls `CoUninitialize` at thread
  exit, and destruction order is unspecified.
- Per window, at close: terminate compositions, re-associate focus away, `Pop`, unadvise, release,
  and `SetInputScope(hwnd, IS_DEFAULT)` if a scope was set (Microsoft requires it before the window
  is destroyed). A store whose widget is gone answers `RequestLock` with `E_FAIL`, as both browsers
  do.
- Every COM method body runs in `catch_unwind` and returns `E_UNEXPECTED` on a panic, as the tap's
  procedure already does: an unwind out of an `extern "system"` method aborts.
- Whether `#[implement]` accepts a type holding an `Rc` (not `Send`) is a one-line compile check in
  Phase 0; if not, the state goes in a thread-local keyed by window, like the tap's.

### 4.6 Documents and focus

- **When**: TSF focus is switched where `settle_ime` already runs, after every dispatched event
  (`app.rs`, four call sites, including the per-frame catch-all), not at the end of the turn.
  Round 7's bursts deliver keys and a click in one loop pass; switching at the end of the turn would
  let a TIP compose the next keys into the field the click just left.
- **What**: one document manager and context per focus session on a text field (a TIP keeps
  per-context state). Chromium instead keeps one per input type and reuses it across fields,
  switching to its empty document in between; reuse is the fallback if M8 finds creation costly.
- **Non-text focus**: an empty, disabled context (`GUID_COMPARTMENT_KEYBOARD_DISABLED` = 1 on the
  context, and `GUID_COMPARTMENT_EMPTYCONTEXT` = 1 where supported), as both browsers do (Chromium
  `InitializeDisabledContext`, Firefox `TSFEmptyTextStore.cpp`), rather than TSF focus null, which
  Chromium calls "TSF's default document which is only there for app compat". Measured (M5): the
  emoji panel opens there and a pick is lost.
- **How**, following Chromium (`tsf_bridge.cc`): each window is associated with its own empty
  document manager (once, while a store or the empty context has the focus: an IMM-path widget needs
  the association dropped, M2) (`AssociateFocus`, which does not AddRef and "calls SetFocus on the document
  manager internally", so that manager lives as long as the window). A text field's document is
  focused with `SetFocus` only, never with both calls for one change (duplicate notifications), and
  only for the window that has the Win32 focus: a switch in a background window (a programmatic
  focus change) is recorded and applied when the window regains focus, so it never takes TSF focus
  from the window the user types in. Measured (M14): the window's `WM_SETFOCUS` comes first, and the
  store's `SetFocus` at the next settle is in time.

### 4.7 The lock model

Microsoft's `RequestLock` remarks: "Applications must never modify the document or send change
notifications using the `ITextStoreACPSink::OnTextChange` method from within the
`ITextStoreACP::RequestLock` method. If the application has pending changes to report, the
application can only respond to the asynchronous lock request." `OnTextChange` adds that the
application "must be able to grant a document lock" when it calls it. The first version broke both.
Four rules:

1. **Nothing changes and nothing is notified inside `RequestLock` before the grant.** The probe is
   `is_available()`. With changes still unreported, a synchronous request answers
   `TS_E_SYNCHRONOUS` and an asynchronous one is queued; the store notifies after `RequestLock`
   returns, then grants.
2. **Inside `OnLockGranted`, only the widget's own state changes** (the batch bracket, 3).
3. **After `OnLockGranted` returns**, the revision is compared with the one at the grant (case 3 of
   4.4) and the store notifies at the tail of `RequestLock`, as both browsers do (Chromium
   `CalculateTextandSelectionDiffAndNotifyIfNeeded`, Firefox `MaybeFlushPendingNotifications`).
4. **No `TerminateComposition`, `SetFocus`, `Push` or `Pop` from inside a lock** (Chromium refuses
   to edit "if there is an on-going document lock"); they are queued to the end of the batch.

Queuing: an asynchronous request while locked is queued (`TS_S_ASYNC`); requests coalesce into one
pending grant, read-write if any asked for it (Microsoft: "the callback should be for write
access"). A synchronous request while locked answers `TS_E_SYNCHRONOUS`. A request while the surface
is busy is queued and a payload posted; teksilo-app grants it between dispatches. With typing flushed
per message (4.10), the busy path should be rare; how often a TIP's synchronous request is refused,
and costs a key, depends on the edit-session flags TIPs use (M12).

### 4.8 Reads

| `ITextStoreACP` | Answered from |
|---|---|
| `GetEndACP`, `GetText` | the UTF-16 index and `text_in`, ranges widened to whole characters then cut back to the units asked; one `TS_RT_PLAIN` run |
| `GetSelection` | `selection()` and `caret()` (`TS_AE_START` or `TS_AE_END`) |
| `GetStatus` | dynamic: `TS_SD_READONLY` unless editable, always `TS_SD_INPUTPANEMANUALDISPLAYENABLE` (the system does not show the touch keyboard by itself; `soft_keyboard.rs` decides, as Chromium does), `TS_SD_TKB*` per field (Q4); static: `TS_SS_NOHIDDENTEXT` |
| `GetTextExt`, `GetScreenExt` | the `first_line_rect` walk of `answers.rs`, and `visible_text_rect`, to screen through `GetDpiForWindow` and `ClientToScreen` |
| `GetACPFromPoint` | `offset_at_point`, `TS_E_INVALIDPOINT` where `None` |
| `GetWnd`, `GetActiveView` | the HWND; one view cookie (`ITextStoreACP2`, which lacks `GetWnd`, is not needed) |
| embedded objects | not supported; an object is already `U+FFFC`, TSF's `TS_CHAR_EMBEDDED` |

- Paragraph breaks stay one `\n` (Chromium's store uses `\n` too); a TIP's `\r\n` is normalised on
  write (4.9).
- Text just written has no layout until the next layout pass. Answer the approximate rectangle
  rather than `TS_E_NOLAYOUT`: Firefox carries that workaround for the Microsoft Japanese IME, ATOK,
  Japanist 10, Free ChangJie and both Microsoft Chinese IMEs (`TSFTextStore.cpp`, the
  `do_not_return_no_layout_error` preferences). M6 only confirms placement. `OnLayoutChange` follows
  the next layout.

### 4.9 Writes

**Decision: a synchronous write half inside the batch bracket** (3). The widget applies a TIP's
`SetText`, `InsertTextAtSelection` or `SetSelection` at once, within the lock, so the TIP's read-back
under the same lock and the next one sees it; its application-visible effects run after
`OnLockGranted` returns. The out-parameters (`pacpStart`/`pacpEnd`, `TS_TEXTCHANGE`) report what
actually landed.

The alternative, posted events with a mirror of the text (the macOS route), loses one of its two
arguments since `b5aa2835` (`ImeComposition` can now replace a range). The other stands: a posted
payload is delivered after TSF has read back, so a mirror is needed, and no event moves the
selection. Chromium's buffer is a third shape: a copy for the duration of one lock, applied to its
client at the tail of `RequestLock`; it is the fallback if the bracket fails in practice.

What the write half must do on the store path:

- **A composition is a range annotation** over text the TIP wrote, not a preedit. The widgets'
  IMM-path machinery (preedit inserted tentatively, withdrawn on cancel, the replaced fragment kept
  aside and put back; `rich_text/keyboard.rs`, `text_input_field/keyboard.rs`) must be bypassed:
  under TSF there is no application-side cancel, and withdrawing would delete text the TIP meant to
  keep. This is a second composition mode in both widgets.
- **Filters** (`max_length`, `char_filter`, single line) apply to writes outside a composition at
  once, and to a composition's final text when it ends: truncating a TIP's intermediate `SetText`
  desynchronises it. A TIP's `\r\n` lands as one break, and the landed length is reported.
- **`TS_ST_CORRECTION`** keeps the replaced run's character format.
- **Undo**: one step per composition, from `OnStartComposition` to `OnEndComposition`, possibly
  across turns (whether text-document's edit blocks can span turns is to be checked), and one per
  write outside a composition. A test: one Ctrl+Z after a TSF composition removes all of it, so round
  6's replay does not carry over.

### 4.10 Notifications from Teksilo to TSF

- **When**: on the store path, typed text is flushed and reported at the end of every dispatched
  input message (in teksilo-app, after the dispatch and `settle_ime`, case 2 of 4.4), so TSF never
  asks for a lock with typing unreported. The frame batch stays for the IMM path. Layout changes are
  reported at the end of the turn, after layout.
- **Never under a lock**: a notification due while a lock is held or `RequestLock` is on the stack
  (a nested dispatch, case 3) waits for the tail of `RequestLock`.
- **What**: `OnTextChange` with the journal's range, `OnSelectionChange`, `OnLayoutChange` after a
  move, a scroll the TIP did not cause, a parent's animation (`SmoothSize`, `Collapse`), a text-scale
  change or a DPI change. Changes caused by the store's own writes are not reported.

### 4.11 Ending a composition

Rounds 6 and 7 settled the IMM path: a click or focus change keeps the text in place, then ends the
composition. TSF has only `ITfContextOwnerCompositionServices::TerminateComposition`, and the TIP may
rewrite the text as it finishes (a Japanese trailing `n` becomes `ん`), so on the store path
teksilo-app terminates **before** it dispatches the press or the focus change, outside any lock and
outside dispatch, lets the TIP write, lays out, and only then dispatches the event, which hit-tests
the text the TIP left (M13). A native dialog opened while composing is the same case (Firefox ends
the composition on `WM_ENTERIDLE`). Round 8: Japanese and Pinyin keep Tab while composing and the
Korean IME commits before Tab reaches the window, so Tab needs nothing.

## 5. Identifying dictation

Plan for **no usable signal** as a likely outcome. Neither browser attributes text by its owner
(Firefox reads no `GUID_PROP_TEXTOWNER`; Chromium only asks whether the active profile is an IME),
and under CUAS three Microsoft IMEs shared one owner GUID that is not a CLSID (1). Candidates, all
M4 and M5:

1. **The composition's owner** (`ITfCompositionView::GetOwnerClsid` in `OnStartComposition`),
   classified with `ITfCategoryMgr::FindClosestCategory` against `GUID_TFCAT_TIP_SPEECH`.
2. **The written range's properties**, read in `ITfTextEditSink::OnEndEdit` with the read-only
   cookie TSF passes, over the ranges `ITfEditRecord::GetTextAndPropertyUpdates` returns (Chromium
   and NVDA advise this sink; it is documented, unlike reading properties with the cookie
   `CreateContext` returns, which Firefox does). Phase 0 enumerates every property there
   (`ITfContext::EnumProperties`), not only `GUID_PROP_TEXTOWNER` (a `TfGuidAtom` of the writer's
   CLSID); `GUID_PROP_TKB_ALTERNATES` (touch keyboard) and the speech lattice
   (`GUID_PROP_LMLATTICE`, not in the `windows` crate) are candidates.
3. **Hints only**: an active speech profile, the speech compartments (empty under CUAS).

Measured for the keyboard IMEs and the emoji panel (M4, M5): candidate 1 fails, every composition
has the same owner (`D7487DBF-804E-41C5-894D-AD96FD4EEA13`) and the same `TEXTOWNER` atom. Whether
voice typing's writes carry a property of their own (candidate 2, the lattice) is M1.

`classify` gains a speech-owner input that yields `Dictation`. Without a signal, text written
outside a composition is `Insertion` and a composition's is `Composition`, as today.

## 6. Secure fields, read-only fields, purposes, accessibility

- **Secure fields** get the empty disabled context with `IS_PASSWORD` (the touch keyboard's password
  layout), so no text and no composition. `GUID_COMPARTMENT_SPEECH_DISABLED` is, per Microsoft,
  specific to the thread manager and a bitmask (`TF_DISABLE_SPEECH`, `TF_DISABLE_DICTATION`,
  `TF_DISABLE_COMMANDING`), not 1 on a context as the first version said: the store sets it on the
  thread while a secure field has focus and clears it on the way out (M7). On the IMM path, Phase -1
  disallows the IME for `ImePurpose::Password` on Windows and X11 (`input_loop::ime_allowed_for`).
- **Read-only fields**: a context with `TS_SD_READONLY`; writes return `TS_E_READONLY`,
  `OnStartComposition` refuses.
- **Purposes**: `Normal` is `IS_DEFAULT`, `Password` `IS_PASSWORD`, `Terminal` `IS_DEFAULT` (it takes
  the IMM path anyway), through `GUID_PROP_INPUTSCOPE` on the store. **Not `SetInputScope` on the
  HWND** (msctf, not bound by the `windows` crate), which Phase -1 tried for the IMM path and
  dropped: measured under CUAS (`tsf-phase-minus-1-windows-3-findings.md`), `IS_PASSWORD` on the
  window closes the Microsoft Japanese and Korean TIPs for the whole window once the password field is
  left; IMM's open status can be set back (`ImmSetOpenStatus`) and then says open while the TIP stays
  off (the next Alt+` "opens" it), so an IMM-level repair does not work. Without the scope, the
  disassociated context alone keeps the field plain and the TIP comes back as it was. The password
  scope therefore belongs to the store's disabled context (Phase 1), and the touch keyboard's
  password layout waits for it.
- **Accessibility**: UIA's TextPattern is AccessKit's and stays separate, but **NVDA reads the
  store**: its injected helper advises `ITfTextEditSink` on the focused context and reads the
  composition through `ITfRange::GetText`. NVDA is one more TSF client of the arbiter, and a reason
  the secure context stays empty (M11). Windows voice access, not Win+H, works through UIA. A write
  from an assistive technology stays `EditSource::Accessibility`.

## 7. Plan, measurements, risks, size, tests

### 7.1 Phases

The store stays behind `TEKSILO_TSF=1` until Phases 2 and 3 pass on the VM: a store that writes but
cannot compose breaks every IME.

- **Phase -1, IMM wins that ship now** (about 500 lines, no TSF). The subclass is installed always and
  answers `WM_IME_REQUEST` (`IMR_QUERYCHARPOSITION`, `IMR_RECONVERTSTRING`,
  `IMR_CONFIRMRECONVERTSTRING`, `IMR_DOCUMENTFEED`) from the read half, through `is_available()`:
  winit has no arm for it, so the tap answers this one message instead of passing it on, a
  documented exception to its rule. Most such requests arrive while winit handles another message
  (round 5), when a widget may be busy and the answer is "none", as today. The reconverted range
  becomes the next preedit's `replacement`, as `take_marked_replacement` does on macOS. The IME is
  disallowed for passwords (no input scope, see below). It also serves `CodeEditor`, the terminal
  and, later, ATOK and Mozc under TSF. Then re-run Japanese reconversion and prediction and Korean
  Hanja on a committed syllable (round 6 saw no request for the latter; to be measured).
  Measured on 9c6641ae (`cross_tasks/tsf-phase-minus-1-windows-findings.md`): candidate placement,
  Japanese reconversion of a selection and at the caret, `IMR_DOCUMENTFEED` and the password refusal
  work. **Hanja on a committed Korean syllable sends no `WM_IME_REQUEST` at all**: the Hanja key
  reaches the window as a plain `WM_KEYDOWN` (`VK_HANJA`), with the caret after the syllable or the
  syllable selected, while Notepad reconverts it. The Korean IME does this through TSF only, so it is
  left for the store (Phase 2 and 3). Leaving a password field closed the Japanese IME (it came back
  `A`, where a WinForms password box gives it back open).
  Rounds 2 and 3 (`tsf-phase-minus-1-windows-2-findings.md`, `-3-findings.md`): the close comes from
  the `IS_PASSWORD` input scope, after the field is left; an `ImmSetOpenStatus` answering it leaves
  IMM saying open while the TIP stays off, and keeping the context with the scope alone is worse (the
  TIP closes as the field takes the focus and Win+. opens nothing). Without the scope the field stays
  plain and the TIP comes back as it was. So Phase -1 sets **no input scope** (section 6). Round 4
  (`-4-findings.md`) confirmed it: the IME comes back after every round trip (Japanese, Korean, by
  click, by Tab, through two password fields), an IME that was off stays off, and the password
  field stays plain. Win+. now opens no emoji panel at all in a password field (with the scope it
  opened and the pick was dropped).
  **Reconversion is not answered** in Phase -1: it opened 6 of 6 times in the first session and
  0 of 24 in later ones (with the text around the selection or the selection alone, in fresh
  processes), while Notepad and a WinForms (Win32 edit) box reconverted at the same moments, with our
  answers the same. A reconversion that does not open stays pending in the IME, whose next
  composition holds the text again (doubled after a caret move). The one marker: every failure
  has 3 `IMR_QUERYCHARPOSITION` between the `VK_CONVERT` key-down and `IMN_OPENCANDIDATE`, the one
  traced success 2, then 3 more and `WM_IME_STARTCOMPOSITION` where the failures get 2 and
  `IMN_CLOSECANDIDATE`, all answered with the same position. **Open question for the store**: under
  TSF a TIP reconverts through the store's own reads (`ITfFnReconversion`, `GetText`, `GetTextExt`),
  not through these requests, so whether it is reliable there is measured in Phase 2 and 3.
- **Phase 0, measure** (about 1,000 lines). Chromium's key model, a store answering reads from the
  document and taking writes into a private mirror (kept afterwards as a debug fallback), the empty
  disabled context, `ITfKeyTraceEventSink`, `ITfTextEditSink` with `EnumProperties`, tracing.
  Answers M1 to M5, M9, M10, M12, M14 (7.2 has what was found; M1 is still open). A design change
  they force comes back here first.
  Built behind `TEKSILO_TSF=1` (`TEKSILO_TSF=associated` keeps winit's IMM context for M10):
  `text_input_hook/windows/tsf.rs`, with the pure `tsf_mirror.rs` and `tsf_lock.rs`. `#[implement]`
  accepts a type holding an `Rc` (4.5's compile check). The store syncs its mirror at every
  `settle_ime` while no lock is held and no composition is in progress, so a text service's writes
  live until the next sync after its composition ends; nothing reaches the field.
- **Phase 0.5, the shared core API** (about 2,000 lines with the two widgets and their headless
  tests). Section 3's teksilo-core items: write half with dry run and correction, batch bracket,
  `is_available`, `flush_pending_input`, journal with cause, composition-as-range mode in both
  widgets, undo across turns, refusing default bodies. Reviewed as public API before any COM write.
  Decided by the maintainer (`claude_reviews/tsf-phase-0.5-api-proposal.md`) and built: the write
  half is a second trait, `TextInputEditor`, reached through `TextSurface::input_editor` (`None`
  for a read-only or secure field); `WriteOptions` is a struct with `Default`. The widgets share
  `common::input_editor` (the order of things), `input_journal` (from the document's change events,
  the cause from a flag set around the input system's writes) and `input_batch`; a written
  composition is `Composition`'s second mode on text-document's transient edits. The batch holds
  back the widgets' own effects and, through text-document's `hold_events` (taken at the outermost
  `begin_input_batch`, dropped at the outermost end), the document's own subscribers; the held
  events are released as the input system's, so the journal, which hears them only then, gives
  them that cause. While the events are held the widgets build their UTF-16 index afresh at each
  read, the cache's invalidation being one of the held subscribers; the layout follows at release.
- **Phase 1, reads** (about 800 lines). Real answers, the arbiter, notifications per message, focus
  in `settle_ime`, the secure, read-only and empty contexts. NVDA with the Japanese IME (M11).
  Built: the store reads the focused field's document (`text_in` through its UTF-16 index, ranges
  widened then cut back to the units asked; the selection; `GetTextExt` as the bounding box of the
  range's own lines, clipped to the visible text with `pfClipped` set when cut), and the mirror is
  now only the text as TSF last heard of it, followed by the journal's ranges (`text_changed_since`,
  the old offsets through the UTF-16 index at the last report) and compared whole only after a gap,
  a text service's writes, or for a document that counts no revisions. In this phase a text
  service's writes still land in the mirror only, and the reads answer from it until the settle
  after the composition puts the document back. The arbiter refuses a synchronous lock and defers
  an asynchronous one while the field is busy or its text or selection revision is ahead of the
  last report, and posts a `TextStorePayload` that `teksilo-app` answers with `settle_ime` between
  dispatches, where batched typing is flushed, the changes reported and the deferred lock granted.
  At the tail of `RequestLock` changes a message made during the grant are reported (no flush,
  no write put back). While a `RequestLock` is on the stack, `set_text_store_focus` moves nothing
  and asks for another settle. A secure field (or a password) gets the empty context and turns
  speech off on the thread (`GUID_COMPARTMENT_SPEECH_DISABLED`, the three bits, the earlier value
  given back on the way out); its `IS_PASSWORD` scope, which needs a store answering
  `GUID_PROP_INPUTSCOPE`, waits for Phase 4 with the touch keyboard's flags. A read-only field that
  describes its document gets a read-only store: `TS_SD_READONLY`, writes refused, compositions
  refused, `OnStatusChange` when the field turns read-only or back.
  Measured (round 1, `tsf-phase-1-windows-findings.md`): everything passed but one stall. A
  change of the field's selection while a composition was open (an IME passing Down to the field
  after the first Space, or Shift+Right after a reconversion opened an empty composition) left the
  revision ahead of the last report for good, since nothing is reported during a composition, and
  every later lock was deferred: the keys were lost until focus left. Fixed by holding a lock back
  only for what the next report can clear (typing not inserted, which a settle now inserts even
  during a composition, or changes reportable now; nothing while a composition is open or the
  mirror holds a text service's writes), and by granting a deferred request at a settle after its
  report whatever is left, traced. A text service's `SetSelection` now counts as a write, so its
  own selection is what it reads back (the reconversion's `SetSelection 140..144` was answered
  `140..140` from the document).
  Round 2 (`tsf-phase-1-windows-2-findings.md`) passed: no lock queued, refused or deferred. It
  also found what Phase 2 must match: in Notepad the Microsoft Japanese IME passes Down after the
  first Space too, and that Down ends the composition there (the underline goes, the next Enter
  adds a line), where the store's stayed open until Enter.
- **Phases 2 and 3, writes and compositions, one unit** (about 2,500 lines). The write half wired,
  the composition sink, interim dictation, the Japanese IME through the store, termination before a
  press, display attributes for clauses, the key-trace verdict, `WM_IME_REQUEST` kept under TSF. On
  by default once the VM runs pass: Japanese, Pinyin and Korean, voice typing in two languages, the
  emoji panel, the touch keyboard, NVDA.
  Built: `SetText`, `InsertTextAtSelection` and `SetSelection` go to the field's
  `TextInputEditor` inside the batch each grant opens (writes land at once and are read back under
  the same lock; what the application sees follows the grant), `TS_ST_CORRECTION` as a correction,
  a query as a dry run, the landed range answered in UTF-16. The mirror keeps no writes: it is the
  text as the text services last heard of it, following the journal; a change the text services
  made, and the selection their writes left, are not told back. The composition sink marks the
  range `OnStartComposition` names (empty at the caret, or the reconverted text), the edit sink
  hands over the clauses (`GUID_PROP_ATTRIBUTE` runs, styled by `ITfDisplayAttributeMgr`'s
  `bAttr`: target converted and target not converted thick, input and converted thin, drawn per
  clause in both widgets), and `OnEndComposition` commits it as one undo step through the field's
  rules. `TerminateComposition` runs before a press is dispatched (`teksilo-app` lays the window
  out after it, M13) and whenever a field ends its composition (a focus change, an edit command),
  through the same `finish_composition` call the IMM path uses; the field then finds it ended. The
  store serves its widget by id (`TextSurfaces::surface`), so the last text of a composition a
  focus change ended lands in the field that held it. A key the text service passes on while its
  composition is open ends the composition first, its text kept, then acts (Notepad's behaviour
  above). The key-trace verdict (a key traced with no `WM_KEYDOWN` for it) counts as a key of an
  Alt chord. `WM_IME_REQUEST` answers stay, unchanged. The emoji panel's one-shot composition goes
  through the same sink. Reports are no longer held back during a composition: the text is the
  field's now, so a change the field makes under one (its caret moved) is reported and lets the
  text service end it.
  Measured (`tsf-phase-2-windows-findings.md`): no hang, crash, lost or doubled key; every
  composition accepted and ended; clicks, focus changes, Down after Space (Notepad's behaviour),
  the emoji panel, Alt+`, bursts, the secure and read-only fields and the IMM path pass. Fixed
  after it: a caret at a soft line break is answered at the start of the next line, where it is
  drawn (the reconversion list opened at the end of the previous line); a selection the IME sets
  is followed by `OnLayoutChange`, since the rectangles it would read moved; a pen press ends the
  composition before it is dispatched, as a mouse press and a contact do. Open: an undo right
  after a commit did nothing three times in about fifteen, until the caret moved (not reproduced
  headlessly; the widgets now trace what an undo did under `TEKSILO_TEXT_INPUT_TRACE`), and the
  Microsoft IME's cancel of a reconversion it started itself first wrote the text before the word
  back over the word (the reads it made are answered from the document and match its
  addressable text; `GetText` now traces what it answered).
  Round 2 (`tsf-phase-2-windows-2-findings.md`) passed: the undo that did nothing was the
  worker's tooling (its key helper sent W's scan code to a QWERTY-based layout); 23 of 23 undos
  right after a commit worked. **Tolerated, as the Microsoft IME's behaviour**: cancelling a
  reconversion the IME started itself (Esc three times) first writes the text from the previous
  break up to the word back over the composition ("Showcase川山海空..."), then the right word;
  the final text is right and UI Automation shows no visible state in between, in Notepad as in
  Teksilo. `GetText` keeps tracing what it answered.
- **Phase 4** (about 500 lines). Attribution (5), touch keyboard flags, a document for `CodeEditor`,
  macOS's `insertText:` onto the write half.
  Built: **attribution** (section 5) takes what M4 and M5 allow: every composition has the same
  owner, but a keyboard input method marks its clauses with display attributes and the emoji
  panel's one-shot pick carries none, so a composition that never carried one ends as
  `ImeOrigin::Insertion` and one that did as `Composition`; both are `EditSource::Ime` in the
  widgets, as before. Voice typing is unmeasured (M1) and is never `Dictation` yet. **Touch
  keyboard**: `TS_SD_INPUTPANEMANUALDISPLAYENABLE` as before, autocorrection and prediction
  (`TS_SD_TKBAUTOCORRECTENABLE`, `TS_SD_TKBPREDICTIONENABLE`) unset (see the measurement below: the
  suggestion strip is the user's setting). **Input scope**: the store answers `GUID_PROP_INPUTSCOPE`
  (`RequestSupportedAttrs`, `RequestAttrsAtPosition`, then `RetrieveRequestedAttrs` with an
  `ITfInputScope` of one scope, Chromium's `TSFInputScope`): `IS_DEFAULT` for a field,
  `IS_PASSWORD` for a secure field, which now gets a context of its own over a store that holds no
  text (the field's document answers nothing while secure), with the keyboard-disabled compartment,
  so no input method composes there and the touch keyboard can show its password layout.
  **`CodeEditor`** describes its document and takes the input system's writes (about 440 lines and
  160 of tests: `code_editor/input_document.rs`): it leaves the IMM path; a write outside a
  composition goes in as a paste does, a multi-caret edit collapses to the primary caret, and a
  written composition ends, its text kept, before a key passed on, a press, an edit command and a
  focus change (the IMM path's preedit is still discarded on focus loss). **`TeksiloAppBuilder::
  windows_text_store(bool)`**, off by default; `TEKSILO_TSF` overrides it (`0` off, `1` on).
  Measured (`tsf-phase-4-windows-findings.md`): no FAIL. The password context answers scope 31 and
  the touch keyboard shows its password layout; attribution, `CodeEditor` through the store and
  `TEKSILO_TSF=0` pass. **The touch keyboard showed suggestions in a field** (a misspelt "the": the, ten, tea,
  tee) without autocorrecting, the store setting neither TKB flag. The flags say a document
  *supports* the keyboard's autocorrection and suggestions (`TS_STATUS`); the keyboard types by
  injecting keys (`WM_KEYDOWN src=keyboard/system`) and shows its strip anyway. Chromium's store
  sets neither flag and answers `IS_DEFAULT` (or `IS_PRIVATE` when it must not learn), and
  Firefox's source does not use them. Round 2 (`tsf-phase-4-windows-2-findings.md`) tried
  `IS_SEARCH_INCREMENTAL` (52, documented for UWP as prediction off): the strip stayed, and a native
  Win32 edit with `SetInputScope` shows it under every non-password scope tried (0, 1, 3, 4, 6, 40,
  50, 52, 65, 68 on 22621) while the layouts change; and that scope switched off the Microsoft
  Japanese IME's own prediction list, in the native box too. **The suggestion strip is the user's
  system setting ("text suggestions"), as in native applications**: Teksilo sets nothing for it,
  there is no per-field opt-in, the TKB flags stay unset, and a field's store answers `IS_DEFAULT`
  (the password context keeps `IS_PASSWORD`, which has no strip). The GUID
  `D5138268-A1BF-4308-BCBF-2E739398E234` a text service asks as a supported attribute (no
  `RetrieveRequestedAttrs` follows) is in no public header; the store does not answer it.
  `CodeEditor` now draws its composition's underline, clause by clause, as the other two widgets do.

Total: **6,000 to 9,000 lines with tests**, plus an open tail of TIP-specific workarounds. For scale:
Chromium's TSF code is about 3,050 lines without tests, with the composition bookkeeping done by
Blink across IPC; Firefox's about 9,900; here, the macOS hook grew from 2,480 to 3,599 lines in the day after the first
version, and the two widgets by about 1,750 lines, with tests, in three composition commits
(`506b5de7`, `df05fb36`, `05e91aa1`).

### 7.2 Measurements

Settled without measuring (sources in 4): the keystroke manager and the message pump are not
required (Chromium); approximate rectangles over `TS_E_NOLAYOUT` (Firefox); `AssociateFocus`
semantics (Chromium); the lock contract and the compartments' levels (Microsoft); NVDA's use of the
store (its source). Still to be measured on the VM, through the `win` worker on the desktop (session
0 over `ssh win` does not load TIPs representatively):

Measured in Phase 0 (`cross_tasks/tsf-phase-0-windows-findings.md`, Windows 11 22621, the
Microsoft Japanese, Pinyin and Korean IMEs, the emoji panel; no hang, no crash). The answers:

- M1: voice typing into the store: **not measured yet** (it needs someone to speak); flyout, interim
  text, commands, the calls it makes and how much it reads; voice access.
- M2: **with our empty document associated with the HWND, the IMM path did not compose**: TSF focused
  that disabled context as winit's IMM context activated (the probe read a context with no flags
  where CUAS's reads `[TRANSITORY]`), Alt+` toggled nothing and keys stayed plain. Fixed by
  associating the HWND back (with what `AssociateFocus` first handed back: nothing) while an IMM-path
  widget has the focus, and with the empty document again when it leaves. **Settled** in round 2
  (`tsf-phase-0-windows-2-findings.md`): the code editor composes Japanese and Pinyin under
  `TEKSILO_TSF=1` after every switch with a store field, by click and by Tab, and the focused
  context reads CUAS's `[TRANSITORY]`. So 4.6's "associated once" becomes "associated while the store or the
  empty context has the focus".
- M3: Chromium's model works with no hook: TSF processes the keys; a key a text service takes
  reaches winit as **its key-up only** (no `WM_KEYDOWN`, neither `VK_PROCESSKEY` nor the plain key,
  and no `WM_CHAR`), between the key trace's down and up. A key it passes on (an arrow ending a Korean
  syllable, letters with the IME off) arrives as usual. No double processing seen. The key-trace
  verdict (4.3) is what tells winit's lone key-ups apart.
- M4: **one owner for everything**: `D7487DBF-804E-41C5-894D-AD96FD4EEA13` for the Japanese, Pinyin
  and Korean compositions and for the emoji picks, not any TIP's CLSID, and `TEXTOWNER` the same atom
  (2) on all of them; so **no attribution from the owner**, as section 5 feared. Properties per IME:
  Japanese `ATTRIBUTE` (30 while typing, 62 once converted), `READING` (half-width katakana),
  `COMPOSING`, `TEXTOWNER`; Pinyin `ATTRIBUTE` 94, `COMPOSING`, `TEXTOWNER`, no `READING`; Korean
  `ATTRIBUTE` 126, `READING` the syllable itself, one composition per syllable, the next one started
  in the same edit session. After a composition ends, `READING` and `TEXTOWNER` stay (Japanese),
  `TEXTOWNER` alone (Pinyin, emoji). Voice typing's are M1's.
- M5: **an emoji pick is a one-shot composition** (`OnStartComposition`, `SetText`,
  `OnEndComposition` in one edit session), not `InsertTextAtSelection`, with the same owner and
  `TEXTOWNER` alone; search works when typed right after Win+. **With a non-text widget focused (the
  empty, disabled context) the panel opens and a pick is lost**, with no call at all, as the IMM path
  loses it without a field.
- M6: the candidate list and the prediction list open under the caret. Phase 0's store answers every
  `GetTextExt` with the caret rectangle (the composition is in the mirror, not the document), so
  per-clause placement waits for the write half.
- M7, M8, M11, M13: not measured yet. For M8, on a 7709-unit document the text service read about
  6000 units at focus, in 63 `GetText` calls of 128 units stepping by 96.
- M9: Alt+` with the Japanese IME toggles it and **the menu bar keeps away**: between Alt's down
  and up winit sees `WM_SYSKEYDOWN`/`WM_SYSKEYUP` for `VK_KANJI` (the key was passed on), so the
  Alt tap is cancelled; Alt's own release came as `WM_KEYUP`, not `WM_SYSKEYUP`.
- M10: **associated and disassociated behave the same for the store**: the same locks, writes,
  owner, properties and keys, and no `WM_IME_*` message during typing in either. Associated adds a
  `WM_IME_SETCONTEXT` pair and two candidate-placement rounds at start-up; disassociated gets
  `IMN_*` notifications with no input context to read (Alt+`, layout switches). The design keeps
  disassociation; association is a working fallback.
- M12: **every lock a text service asked for was asynchronous** (`0x6` read-write, `0x2` read), and
  none was queued or refused. Synchronous read locks (`0x3`) came only with a plain keyboard layout
  active (TSF itself, at a key-up), each followed by one `GetSelection`. So a refused synchronous
  request costing a key (risk 4) did not occur.
- M14: leaving the window gives `WM_KILLFOCUS` and nothing from the store; coming back, the
  window's `WM_SETFOCUS` precedes the store's `SetFocus`, applied from the next settle, and Japanese
  composes at once. Between fields of one window the stores swap with no window focus message.

### 7.3 Risks

1. **Messages delivered during TSF calls** (4.4): measured to lose input once.
2. **The untrodden IMM configuration** (M10, measured: no difference) and **CUAS after activation**
   (M2, settled: the HWND is not associated with the empty document on the IMM path).
3. **Eaten keys**: the Alt tap and `alt_down` (M9).
4. **Reentrancy under a lock**: a nested dispatch changing the document; a refused synchronous lock
   costing a key (M12).
5. **The second composition mode** in both widgets, which carry about 3,600 lines of keyboard code
   between them, and undo across turns.
6. **Other TIPs**: ATOK, Google Japanese Input, Chinese IMEs differ; Firefox carries per-TIP
   workarounds, so expect some.
7. **Large documents** (M8). **Teardown** (TSF calls during `Pop` and `Deactivate`). **A native
   dialog** opened mid-composition (4.11).
8. **Behaviour changes**: passwords refuse IMEs and dictation (6).

### 7.4 Tests

- **Headless, every OS**: the pure modules of section 3 (arbiter, coalescing, composition tracking,
  `classify`, key-trace verdict); the write half, the bracket and the journal on `RichTextEditor`
  and `TextInputField` in `WidgetTree` tests (filters, read-only, secure, undo, origins, deferred
  callbacks, `is_available` changing nothing).
- **Windows, no TIP**: the test process as its own TSF client (a composition and an insertion in an
  edit session on our context) drives real msctf against the real store. On the `win` worker's
  desktop; whether a CI runner activates TSF at all is unknown.
- **Not coverable headlessly**: any TIP, the flyout, placement on screen, key timing. VM runs.

## 8. Decisions on the first version's open questions

1. **Writes**: the synchronous write half inside the batch bracket (4.9).
2. **Path per focus** in `settle_ime` (4.2, 4.6). Not waiting for `CodeEditor`: the IMM path is
   needed anyway for legacy IMM-IMEs.
3. **Passwords refuse IMEs and dictation**: Phase -1 on the IMM path, the disabled context and the
   thread-level speech compartment on the store path. It changes what users get; the maintainer
   decided it applies on Windows and X11 only: on Wayland, turning text-input-v3 off would take the
   on-screen keyboard away (GTK keeps it on with the password purpose), and macOS composes dead keys
   through the input system, as a secure text field does.
4. **Touch keyboard autocorrect and prediction**: decided at first as off by default with a
   per-field opt-in through the dynamic `TS_SD_TKB*` flags; measured in Phase 4, the suggestion
   strip follows the user's own setting whatever a field says (native edits included), so Teksilo
   sets nothing and there is no opt-in. The flags stay unset, so the keyboard does not autocorrect.
5. **A voice command that deletes or replaces** inside a speech-owned session is a `Dictation` edit
   (an empty-text commit with a replacement range, as macOS posts deletions); a selection-only
   command is not an edit and reports nothing.
6. **`\n` stays one unit**; a TIP's `\r\n` is normalised and the landed range reported.
7. **macOS**: its `insertText:` outside a key press moves to the write half after Windows (3).
   Built: text AppKit inserts outside a key press with no marked text (Emoji & Symbols, the accent
   popup, AutoFill, and Dictation or Writing Tools when they insert without marking) is written into
   the focused field at once (`text_input_hook/write_half.rs`), inside a batch: an `Insertion` as the
   input system's write outside a composition, any other origin as a composition written and ended
   at once, so it keeps its origin (`end_composition`) and is one undo step. A text with a line break
   and an origin other than `Insertion` is still posted, because a composition's text lands as
   written and a break would not become a paragraph; so is a commit that ends winit's marked text
   (that composition is winit's, section 3), and anything the field cannot take now. A doc-hidden
   `ImeWrittenPayload` then tells `teksilo-app` to do a commit's bookkeeping (the user's activation,
   the preedit dedup, a settle and a redraw). An insertion is now its own undo step, where the event
   batched it with the typing. `CodeEditor`, which describes its document since Phase 4, honours the
   ranges the macOS input system names (a commit over a range, Korean Hanja's composition over a
   syllable).
8. **Focus lost mid-composition**: terminate, before the event is dispatched (4.11).
9. **The switch**: `TEKSILO_TSF=1` through Phases 2 and 3, then on by default with a
   `TeksiloAppBuilder` option forcing IMM; the variable stays as a kill switch; IMM is taken
   automatically when activation fails or a legacy IMM-IME is active.

Still for the maintainer: the teksilo-core API of section 3 item by item (`is_available` exists,
with a default of `true`); the builder option's name; the touch keyboard default.

## 9. Where this design departs from the review

- **Calls during winit's handling.** The review says never to make a TSF call before winit has
  handled the current message. Refined in 4.4: a call inside winit's handling is safe, because winit
  buffers the events of nested messages in order (`runner.rs`, `send_event`) and `finish_composition`
  relies on it, measured in rounds 6 and 7. The danger is a call before winit has the message, and a
  nested message reaching the application while no handler runs.
- **`#[implement]` and `Send`.** The review calls it settled by `external_dnd/windows.rs`, which it
  says has `#[implement]` types holding a `RefCell`. The `RefCell` there is in `WindowsDndGuard`, not
  a COM type; `DropTarget` holds only an `Arc`. It stays a Phase 0 compile check.
- **"Both browsers chose a mirror."** Firefox's is a mirror; Chromium's buffer lives for one lock
  and is applied to its client synchronously at the tail of `RequestLock`
  (`tsf_text_store.cc`), which is closer to the write half with a bracket than to posted events. It
  is recorded as the fallback (4.9).
- **Focus calls.** The review proposes `SetFocus` for the focused window and `AssociateFocus` alone
  for background ones. This design follows Chromium's arrangement instead (4.6): each window is
  associated once with its own empty document, text documents are only ever `SetFocus`ed, and a
  background window's switch waits for its focus. It meets the same three concerns (stealing focus,
  the missing AddRef, duplicate notifications) without re-associating on every switch.
- **No batching on the store path.** Adopted as "flush and report at the end of every dispatched
  input message" (4.10), which is the same for typing and keeps the frame batch on the IMM path.
