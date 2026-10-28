<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# A TSF text store on Windows: design

Status: a design, not implemented. Revised on 2026-10-28 after an independent review, against
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
- **IMM path**: a surface without a document (`CodeEditor`, the terminal), a store that could not
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
  Chromium calls "TSF's default document which is only there for app compat". What the emoji panel
  does there (refuses, or still loses the pick of scenario 5) is M5.
- **How**, following Chromium (`tsf_bridge.cc`): each window is associated, once, with its own empty
  document manager (`AssociateFocus`, which does not AddRef and "calls SetFocus on the document
  manager internally", so that manager lives as long as the window). A text field's document is
  focused with `SetFocus` only, never with both calls for one change (duplicate notifications), and
  only for the window that has the Win32 focus: a switch in a background window (a programmatic
  focus change) is recorded and applied when the window regains focus, so it never takes TSF focus
  from the window the user types in. The order of TSF's own `WM_SETFOCUS` handling against winit's
  `Focused(true)` is M14.

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

`classify` gains a speech-owner input that yields `Dictation`. Without a signal, text written
outside a composition is `Insertion` and a composition's is `Composition`, as today.

## 6. Secure fields, read-only fields, purposes, accessibility

- **Secure fields** get the empty disabled context with `IS_PASSWORD` (the touch keyboard's password
  layout), so no text and no composition. `GUID_COMPARTMENT_SPEECH_DISABLED` is, per Microsoft,
  specific to the thread manager and a bitmask (`TF_DISABLE_SPEECH`, `TF_DISABLE_DICTATION`,
  `TF_DISABLE_COMMANDING`), not 1 on a context as the first version said: the store sets it on the
  thread while a secure field has focus and clears it on the way out (M7). On the IMM path, Phase -1
  disallows the IME for `ImePurpose::Password`; today IMEs and voice typing work in passwords.
- **Read-only fields**: a context with `TS_SD_READONLY`; writes return `TS_E_READONLY`,
  `OnStartComposition` refuses.
- **Purposes**: `Normal` is `IS_DEFAULT`, `Password` `IS_PASSWORD`, `Terminal` `IS_DEFAULT` (it takes
  the IMM path anyway), through `GUID_PROP_INPUTSCOPE` on the store. `SetInputScope` on the HWND
  (msctf, not bound by the `windows` crate) is for the IMM path only: Microsoft says that on a window
  "not associated with a Document Manager ... no text service notifications are sent to interested
  clients (such as the touch keyboard)", so it may reach an IME (Firefox's `SetInputScopeForIMM32`)
  and probably not the touch keyboard (M7). `IS_DEFAULT` before the window is destroyed.
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
  disallowed for passwords; `SetInputScope` per purpose. It also serves `CodeEditor`, the terminal
  and, later, ATOK and Mozc under TSF. Then re-run Japanese reconversion and prediction and Korean
  Hanja on a committed syllable (round 6 saw no request for the latter; to be measured).
- **Phase 0, measure** (about 1,000 lines). Chromium's key model, a store answering reads from the
  document and taking writes into a private mirror (kept afterwards as a debug fallback), the empty
  disabled context, `ITfKeyTraceEventSink`, `ITfTextEditSink` with `EnumProperties`, tracing.
  Answers M1 to M5, M9, M10, M12, M14. A design change they force comes back here first.
- **Phase 0.5, the shared core API** (about 2,000 lines with the two widgets and their headless
  tests). Section 3's teksilo-core items: write half with dry run and correction, batch bracket,
  `is_available`, `flush_pending_input`, journal with cause, composition-as-range mode in both
  widgets, undo across turns, refusing default bodies. Reviewed as public API before any COM write.
- **Phase 1, reads** (about 800 lines). Real answers, the arbiter, notifications per message, focus
  in `settle_ime`, the secure, read-only and empty contexts. NVDA with the Japanese IME (M11).
- **Phases 2 and 3, writes and compositions, one unit** (about 2,500 lines). The write half wired,
  the composition sink, interim dictation, the Japanese IME through the store, termination before a
  press, display attributes for clauses, the key-trace verdict, `WM_IME_REQUEST` kept under TSF. On
  by default once the VM runs pass: Japanese, Pinyin and Korean, voice typing in two languages, the
  emoji panel, the touch keyboard, NVDA.
- **Phase 4** (about 500 lines). Attribution (5), touch keyboard flags, a document for `CodeEditor`,
  macOS's `insertText:` onto the write half.

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

- M1: voice typing into the store, French and English: flyout, interim text, commands, the calls it
  makes and how much it reads; voice access.
- M2: CUAS for an IMM-path focus once the thread is activated.
- M3: keys in Chromium's model; the hook variant; double processing.
- M4: owners and every property on written ranges, for voice typing and the three IMEs.
- M5: how the emoji panel writes, its owner; type-to-search; with the empty context.
- M6: placement with approximate rectangles (confirmation).
- M7: secure and read-only contexts, the thread-level speech compartment, input scopes, the touch
  keyboard with `TS_SD_INPUTPANEMANUALDISPLAYENABLE`; hardware-keyboard suggestions.
- M8: context creation, `GetText` volume, a Japanese keystroke in a document of 1 MB or more, the
  UTF-16 index rebuild.
- M9: what winit sees for an eaten key; Alt+` and the menu; eaten key-down with a passed key-up.
- M10: IMM context disassociated against associated with a store focused; a legacy IMM-IME.
- M11: NVDA through the store; nothing read from a secure field.
- M12: which edit-session flags TIPs use (`TF_ES_SYNC` or asynchronous), with round 7's bursts.
- M13: termination before a press: the Japanese trailing `n` lands before the press hit-tests.
- M14: TSF's focus handling against winit's `Focused(true)`; a background window's switch.

### 7.3 Risks

1. **Messages delivered during TSF calls** (4.4): measured to lose input once.
2. **The untrodden IMM configuration** (M10) and **CUAS after activation** (M2).
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
   thread-level speech compartment on the store path. It changes what users get; whether the IMM
   change applies on every platform or Windows only is the maintainer's call.
4. **Touch keyboard autocorrect and prediction**: off by default, a per-field opt-in (the touch
   keyboard rewrites committed words with `SetText`; Chromium had to add revert logic for
   `autocorrect="off"`), through the dynamic `TS_SD_TKB*` flags.
5. **A voice command that deletes or replaces** inside a speech-owned session is a `Dictation` edit
   (an empty-text commit with a replacement range, as macOS posts deletions); a selection-only
   command is not an edit and reports nothing.
6. **`\n` stays one unit**; a TIP's `\r\n` is normalised and the landed range reported.
7. **macOS**: its `insertText:` outside a key press moves to the write half after Windows (3).
8. **Focus lost mid-composition**: terminate, before the event is dispatched (4.11).
9. **The switch**: `TEKSILO_TSF=1` through Phases 2 and 3, then on by default with a
   `TeksiloAppBuilder` option forcing IMM; the variable stays as a kill switch; IMM is taken
   automatically when activation fails or a legacy IMM-IME is active.

Still for the maintainer: the teksilo-core API of section 3 item by item; the scope of the password
change; the builder option's name; the touch keyboard default.

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
