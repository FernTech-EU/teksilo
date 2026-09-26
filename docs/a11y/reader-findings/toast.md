<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->
<!-- Written from the sweep's results of 25 and 26 September 2026: a record of what was measured then, not regenerated. See ../reader-findings.md. -->

# Toasts and the notification log

Examples: `toast-demo`.
19 findings: 1 critical, 10 high, 5 medium, 3 low.
How to read an entry, and what the words mean, is in
[Screen-reader findings](../reader-findings.md).

| id | example | finding | severity | platform | status |
|---|---|---|---|---|---|
| [toast-01](#toast-01) | toast-demo | Every change to the toast queue announces every toast still on screen again, in arbitrary order | high | Linux | fixed |
| [toast-02](#toast-02) | toast-demo | Background job: the title 'Background job' is announced at each of 20 steps, and the percentage is never spoken | high | Linux | partly fixed |
| [toast-03](#toast-03) | toast-demo | A toast is announced by its title only: the body is never spoken, and neither is the severity of a polite toast | high | Linux | open |
| [toast-04](#toast-04) | toast-demo | Keyboard focus inside a toast is thrown onto the oldest toast at every queue change, so the background job's Cancel cannot be pressed from the keyboard | critical | Linux | fixed |
| [toast-05](#toast-05) | toast-demo | A toast that arrives while focus is inside a toast or on the bell is cut by Orca re-reading the re-focused node | high | Linux | open |
| [toast-06](#toast-06) | toast-demo | A toast shown while another is up loses the idle time and expires with the older one (e.g. an error toast gone after about 1 s) | high | all | fixed |
| [toast-07](#toast-07) | toast-demo | A timed toast expires with keyboard focus on its action, and focus falls to the bare frame | high | Linux | partly fixed |
| [toast-08](#toast-08) | toast-demo | Dismissing a toast (close button or Escape) leaves focus on the unnamed frame | medium | Linux | open |
| [toast-09](#toast-09) | toast-demo | The toast's close button is named 'Clear' | medium | all | open |
| [toast-10](#toast-10) | toast-demo | The bell does not tell a reader how many notifications are unread | high | all | open |
| [toast-11](#toast-11) | toast-demo | The bell's popover is an unnamed dialog, which Orca names 'Today' from its first label | medium | Linux | open |
| [toast-12](#toast-12) | toast-demo | The notification log's entries cannot be reached by keyboard, and they are not list items | high | all | open |
| [toast-13](#toast-13) | toast-demo | A focused toast reads its body twice | medium | Linux | open |
| [toast-14](#toast-14) | toast-demo | Orca reads a focused toast with an earlier toast's contents (a Cancel button that no longer exists) | medium | Linux | upstream |
| [toast-15](#toast-15) | toast-demo | A loading toast's spinner is an unnamed progress bar with no value | low | all | open |
| [toast-16](#toast-16) | toast-demo | Names repeated as descriptions: bell, toast close button, log rows without a body | low | all | open |
| [toast-v1](#toast-v1) | toast-demo | Any toast shown or updated while the bell's popover is open closes the popover and marks the new notification read unseen; during a background job the popover stays open about 0.1 s | high | Linux | fixed |
| [toast-v2](#toast-v2) | toast-demo | The notification log dialog rebuilds its content on every archive change: while a job runs, focus is thrown back to 'Mark all read' every 160 ms and 'Clear all' cannot be pressed | high | Linux | fixed |
| [toast-v3](#toast-v3) | toast-demo | A reader tabbing forward at a listening pace reaches a toast just as it times out; Shift+Tab from the top is the only quick way in | low | Linux | open |

### toast-01 {#toast-01}

Every change to the toast queue announces every toast still on screen again, in arbitrary order

- **Example:** toast-demo
- **Scenario:** toast-severities, toast-persistent
- **Act:** toast-severities: Space on Info, Success, Warning, Error in turn (each earlier toast still up). toast-persistent: Space on Info while a persistent error is up, then let the Info toast expire.
- **The reader should get:** Each act announces only the toast that just appeared. A toast that expires or is dismissed leaves quietly. Toasts already on screen are not announced again.
- **The reader gets:** Each new toast brings one announcement for every live toast. The Error act carries 4 announcements, and the new 'Build #4 failed' comes third of four (the order changes from run to run). When the Info toast expires, the persistent 'Sticky error #1' is announced again as if new. Orca speaks all of it: 'Info notice #1', 'Warning #3', 'Build #4 failed', 'Saved #2'.
- **Platform:** Linux AT-SPI/Orca measured. By adapter source, the same on Windows (accesskit\_windows adapter.rs:256-262 raises UIA LiveRegionChanged for every added named live node) and on macOS (accesskit\_macos event.rs:233-240 queues an announcement for every added live node with a label).
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: a queue change (show, update, dismiss, expiry) announces only the toast that just appeared. Toasts already on screen are never read again..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:252-256, 273-320; crates/teksilo-widgets/src/toast/surface.rs:426-446
- **Evidence:**
  - `toast-severities (run 20260925-130506), Space on Error: '+44.4 ms object:announcement [status bar] 'Info notice #1' text='Info notice #1'', '+49.7 ms object:announcement [status bar] 'Warning #3' text='Warning #3'', '+50.7 ms object:announcement [notification] 'Build #4 failed' text='Build #4 failed'', '+51.7 ms object:announcement [status bar] 'Saved #2' text='Saved #2''`
  - `orca-debug.out (same run): '13:05:22.919951 - SPEECH OUTPUT: 'Info notice #1'', '13:05:22.927434 - SPEECH OUTPUT: 'Warning #3'', '13:05:22.933556 - SPEECH OUTPUT: 'Build #4 failed'', '13:05:22.941670 - SPEECH OUTPUT: 'Saved #2''`
  - `toast-severities (run 20260925-130112), Space on Success: '+32.3 ms object:announcement [status bar] 'Info notice #1' text='Info notice #1'', '+34.4 ms object:announcement [status bar] 'Saved #2' text='Saved #2'', '+36.5 ms object:children-changed:add [frame] '' -> [status bar] 'Info notice #1'', '+36.8 ms object:children-changed:remove [frame] '' -> [status bar] 'Info notice #1''`
  - `toast-persistent (run 20260925-130310), the info toast times out: '+7144.6 ms object:announcement [notification] 'Sticky error #1' text='Sticky error #1'', '+7147.9 ms object:children-changed:add [frame] '' -> [notification] 'Sticky error #1'', '+7148.3 ms object:children-changed:remove [frame] '' -> [notification] 'Sticky error #1'', '+7172.5 ms ORCA SAYS: 'Sticky error #1''`
  - `crates/teksilo-widgets/src/toast/host.rs:252-256 binds the registry version at BindingLevel::Rebuild; host.rs:273-319 builds a fresh ToastSurface (a new WidgetId, so a new AccessKit node) for every live entry on every rebuild`
  - `crates/teksilo-widgets/src/toast/surface.rs:426-446: every surface is a live node (Status polite / Alert assertive) named by its title`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:72-77 emits object:announcement for a live node added with a name; accesskit_consumer-0.39.0/src/tree.rs:80 keeps added_node_ids in a HashSet, which is why several re-added toasts are announced in hash order`
  - `toast-severities-20260925-132559-2986621, Space on Error: '+41.5 ms object:announcement [status bar] 'Info notice #1'', '+45.4 ms object:announcement [notification] 'Build #4 failed'', '+46.4 ms object:announcement [status bar] 'Saved #2'', '+47.5 ms object:announcement [status bar] 'Warning #3''; Orca speaks all four: +83.3 'Info notice #1', +88.5 'Build #4 failed', +94.5 'Saved #2', +99.7 'Warning #3'`
  - `toast-persistent-20260925-132640-3017264, the info toast times out: '+7139.9 ms object:announcement [notification] 'Sticky error #1' text='Sticky error #1'', '+7141.3 ms object:children-changed:remove [frame] '' -> [status bar] 'Info notice #2'', '+7150.4 ms ORCA SAYS: 'Sticky error #1''`
  - `verify-toast-dismiss-one-of-two-20260925-133046-3232072, Space on the second toast's close button: '+29.1 ms object:announcement [notification] 'Sticky error #1'', '+30.7 ms object:state-changed:focused 1 [notification] 'Sticky error #1'', '+39.1 ms ORCA SAYS (CUT): 'Sticky error #1''`
  - `run dirs: target/reader-verify/toast/toast-severities-20260925-1325*, -132840-*, -133255-*; toast-persistent-20260925-132640-*, -132921-*, -133336-*; toast-job-with-info-20260925-132638-*, -132914-*, -133414-*`
- **Reproduced:** toast-severities 3 of 3 runs (re-announcement in every act that had a toast already up); toast-persistent 3 of 3 runs (on the Info toast's arrival and on its expiry)
- **Verification:** confirmed. Reproduced: toast-severities 3 of 3: each act re-announced every toast already up (Success: 2 announcements, Warning: 3, Error: 4). The new title came 1st, 2nd or 3rd, depending on the run. toast-persistent 3 of 3: the sticky error was re-announced when the Info toast arrived and again when it expired. toast-job-with-info 3 of 3: 'Info notice #1' was re-announced 21 times. verify-toast-dismiss-one-of-two 2 of 2: closing one toast re-announces the one left.
- **Fix idea:** Keep each toast's surface (and its node) across host rebuilds, keyed by entry\_id: reconcile the child list instead of rebuilding every surface, or preserve children and push changed data into the existing surface through signals. Then a node enters the tree only when its toast appears, and its name changes only when its title does. None of these messages go through ctx.announce, so the K2 announcer fix does not cover them.

### toast-02 {#toast-02}

Background job: the title 'Background job' is announced at each of 20 steps, and the percentage is never spoken

- **Example:** toast-demo
- **Scenario:** toast-job, toast-job-with-info
- **Act:** toast-job: Space on Start background job; the worker updates the loading toast in place 20 times, 160 ms apart, then replaces it with a success toast. toast-job-with-info: the same with an Info toast already on screen.
- **The reader should get:** The reader hears the job start, some sense of its progress (a percentage now and then), and the completion with its detail, without the title repeated at every step. An unrelated toast already on screen is not re-read.
- **The reader gets:** 20 object:announcement events with the exact text 'Background job'. Orca speaks all 20 and queues them, because Orca 46's speech-dispatcher backend ignores interrupt=True, so the reader hears 'Background job' about 20 times back to back. Then 'Background job complete'. The percentage ('35% · Fetching item 7 of 20') and the detail 'All 20 items fetched' are never spoken, because they are only in the description. When an Info toast is up, 'Info notice #1' is re-read at every step (22 utterances). In 1 of 3 keyboard-cancel runs Orca lagged 9 to 13 s and ignored all 21 announcements as defunct: the reader heard 'Background job' once and never heard the completion.
- **Platform:** Linux AT-SPI/Orca measured. Windows and macOS emit one live-region event or announcement per re-added node (see toast-01). NVDA drops a repeat of the same text within 0.5 s, so on Windows a share of the 'Background job' repeats would be dropped, but the percentage is not spoken there either, since the announced string is the name.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: partial): 'Background job' is announced and spoken once instead of 20 times, and 'Background job complete' is heard. An unrelated Info toast is no longer re-read at each step. Orca speaks each new percentage only while focus is on the progress toast..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:273-320; crates/teksilo-widgets/src/toast/surface.rs:436-445; examples/toast\_demo/src/main.rs:360-370 (progress carried only in .body())
- **Evidence:**
  - `toast-job (run 20260925-130854): '20 announcement(s) of exactly 'Background job''; 'Orca said exactly 'Background job' 20 time(s)'`
  - `orca-debug.out (same run): '13:09:02.593028 - SPEECH OUTPUT: 'Background job'' … '13:09:05.644350 - SPEECH OUTPUT: 'Background job'' (20 lines), then '13:09:05.805930 - SPEECH OUTPUT: 'Background job complete''; every one logged as 'NULL SPEECH: speak 'Background job' interrupt=True' with no stop between`
  - `toast-job report: 'FAIL  Orca says any of ('%', 'percent', 'Fetching item')'; 'FAIL  Orca says 'All 20 items fetched'' (Orca unheard)`
  - `toast-job-with-info (run 20260925-130446): '+58.6 ms ORCA SAYS: 'Background job'', '+63.9 ms ORCA SAYS: 'Info notice #1'', '+209.3 ms ORCA SAYS: 'Background job'', '+213.5 ms ORCA SAYS: 'Info notice #1'' … 'Info notice #1' 22 times`
  - `toast-job-cancel-keys (run 20260925-130912, Orca lagging): '13:09:36.053057 - EVENT MANAGER: object:announcement for [DEAD] in [application: 'toast-demo'] (1, 0, Background job complete)', '13:09:40.276239 - EVENT MANAGER: Ignoring defunct object: [DEAD]'; 21 'Ignoring defunct' lines in that log`
  - `/usr/lib/python3/dist-packages/orca/speechdispatcherfactory.py:463-464: the cancel on interrupt is commented out, so announcements queue`
  - `examples/toast_demo/src/main.rs job_progress_toast: progress is in .body(), which surface.rs:442-444 turns into the description; toast/registry.rs:339-360 updates the entry in place, which bumps the version and rebuilds every surface (host.rs:252-256, 273-319)`
  - `toast-job-20260925-132620-2998517: '20 announcement(s) of exactly 'Background job''; orca-debug.out has 20 lines 'NULL SPEECH: speak 'Background job' interrupt=True' with no stop between them; '+3239.7 ms object:announcement [status bar] 'Background job complete'', '+3252.5 ms ORCA SAYS: 'Background job complete''; 'FAIL  Orca says any of ('%', 'percent', 'Fetching item')'; 'FAIL  Orca says 'All 20 items fetched''`
  - `toast-job-with-info-20260925-133414-3449813: the job act has anns {'Info notice #1': 21, 'Background job': 20, 'Background job complete': 1}, and Orca said each of them`
  - `/usr/lib/python3/dist-packages/orca/speechdispatcherfactory.py:158 'client.set_priority(speechd.Priority.MESSAGE)'`
- **Reproduced:** toast-job 3 of 3 runs (20 announcements and 20 utterances each time, no percentage); toast-job-with-info 3 of 3 runs (22 'Info notice #1' utterances); the lag-dependent total loss 1 of 3 runs
- **Verification:** confirmed. Reproduced: toast-job 3 of 3: exactly 20 announcements of 'Background job' and 20 utterances, none cut, then 'Background job complete'. No percentage and no 'All 20 items fetched' in any run. toast-job-with-info 3 of 3: 'Info notice #1' was re-announced and spoken 21 times in the act. The sweep's sub-claim of total loss under Orca lag was not reproduced (0 of 3 cancel-keys runs; Orca lag 1.6 to 3.4 ms).
- **Fix idea:** Update the job toast in place on the same node (see toast-01). Give a loading toast a real progress value: a determinate ProgressIndicator with a numeric value and name. Announce progress sparingly, for example on milestones or at most every few seconds, instead of re-announcing the title at every step.

### toast-03 {#toast-03}

A toast is announced by its title only: the body is never spoken, and neither is the severity of a polite toast

- **Example:** toast-demo
- **Scenario:** toast-severities, toast-persistent
- **Act:** toast-severities: Space on Warning and on Error. toast-persistent: Space on Persistent error. toast-job: completion.
- **The reader should get:** The reader hears what the toast says, for example 'Build #4 failed. Three errors in src/main.rs, two warnings.', and can tell a warning from an info.
- **The reader gets:** Only the title: 'Warning #3', 'Build #4 failed', 'Sticky error #1', 'Background job complete'. The body is only a description, which no adapter announces, so the error detail is heard only if the reader Tabs to the toast before it expires (see toast-05 and toast-06 for why that often fails). Info, Success and Warning are all \[status bar\] with a polite live setting. Orca ignores politeness and speaks only the text, so 'Warning #3' sounds exactly like 'Info notice #1'.
- **Platform:** Linux AT-SPI/Orca measured. Windows: LiveRegionChanged carries no text; the client reads the node, whose name is the title (the description is FullDescription, which NVDA does not speak live). macOS: the announcement carries the label only (accesskit\_macos event.rs:237-239).
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:426-446
- **Evidence:**
  - `toast-severities (run 20260925-130751): 'FAIL  Orca says 'Take a look when you have a moment.''; 'FAIL  Orca says 'Three errors in src/main.rs, two warnings.''`
  - `toast-severities (run 20260925-130112), Space on Error: 'Orca unheard: 'Three errors in src/main.rs, two warnings.'', while the tree holds the text as a description only: 'pass  the tree holds [notification] 'Build #4 failed'' (in_tree with description_contains)`
  - `toast-persistent (run 20260925-131144): 'FAIL  Orca says 'This one persists until you dismiss it.''`
  - `crates/teksilo-widgets/src/toast/surface.rs:436-444: name = announcement override or title; body -> set_description only`
  - `accesskit_atspi_common-0.20.0/src/adapter.rs:72-77: the announcement text is the name; /usr/lib/python3/dist-packages/orca/scripts/default.py:1424-1428 onAnnouncement speaks event.any_data and ignores the politeness`
  - `toast-severities-20260925-132559-2986621, Space on Warning: 'FAIL  Orca says 'Take a look when you have a moment.'' (Orca said 'Warning #3', 'Info notice #1', 'Saved #2'); Space on Error: 'FAIL  Orca says 'Three errors in src/main.rs, two warnings.''`
  - `toast-persistent-20260925-132640-3017264: 'FAIL  Orca says 'This one persists until you dismiss it.'' (Orca said 'Sticky error #1')`
- **Reproduced:** deterministic; seen in every run: toast-severities 3 of 3, toast-persistent 3 of 3, toast-job 3 of 3
- **Verification:** confirmed. Reproduced: deterministic: toast-severities 3 of 3 (Warning and Error bodies never spoken), toast-persistent 3 of 3 (body never spoken on arrival)
- **Fix idea:** When the app gives no .announcement(), default the live name to 'title. body' (Toast::announcement already exists, toast.rs:631-636), and prefix a severity word ('Warning:', 'Error:') where the title does not carry it. The announcement path speaks only a string: the role never reaches the listener.

### toast-04 {#toast-04}

Keyboard focus inside a toast is thrown onto the oldest toast at every queue change, so the background job's Cancel cannot be pressed from the keyboard

- **Example:** toast-demo
- **Scenario:** toast-job-cancel-keys, toast-focus-second, toast-job-cancel-atspi
- **Act:** toast-job-cancel-keys: Space on Start background job, Tab x4 to the job toast's Cancel, listen 0.7 s, Space. toast-focus-second: focus on the second toast ('Warning #2') when a third arrives. toast-job-cancel-atspi: AT-SPI click on Cancel.
- **The reader should get:** Focus stays on the control the reader chose. Space on Cancel cancels the job ('Background job cancelled'). A toast that arrives or updates elsewhere does not move focus.
- **The reader gets:** Focus reaches Cancel. At the next progress step (132 to 163 ms later) the host rebuild destroys it, and the framework puts focus on the first focusable descendant of the ToastHost, the toast surface. Space then lands on the surface and does nothing, and the job completes uncancelled (3 of 3 runs). Each step also re-focuses a fresh 'Background job' node, which Orca reads again every 160 ms. With focus on 'Warning #2', a new toast moves focus to the oldest toast, 'Sticky error #1' (3 of 3). Even AT-SPI activation is unreliable: in 1 of 3 runs the Cancel lookup failed three times, and a fourth click returned done=True but reached a node already replaced, so the job ran on to completion.
- **Platform:** Linux AT-SPI/Orca measured. The focus move is made in the framework's widget tree, so every platform gets the same focus change (UIA focus changed, macOS focused element).
- **Severity:** critical; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: critical): the job toast's Cancel keeps keyboard focus through every progress step and cancels the job. A toast arriving elsewhere no longer moves focus, and focus on 'Warning #2' stays when a third toast arrives. When an update removes the focused action, focus stays in that toast instead of jumping to the oldest one..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:273-320; crates/teksilo-core/src/widget\_tree/layout\_impl.rs:355-373, 405, 862-868
- **Evidence:**
  - `toast-job-cancel-keys (run 20260925-130250): '+1355.4 ms object:state-changed:focused 1 [push button] 'Cancel'', '+1476.4 ms object:announcement [status bar] 'Background job' text='Background job'', '+1479.1 ms object:state-changed:focused 1 [status bar] 'Background job'', '+1479.2 ms object:state-changed:focused 0 [push button] 'Cancel'', '+1479.5 ms object:state-changed:defunct 1 [push button] 'Cancel''`
  - `same run, each later step: '+1630/1634.0 ms object:state-changed:focused 1 [status bar] 'Background job'', '+1634.0 ms object:state-changed:focused 0 [status bar] 'Background job'' (a fresh node every 160 ms); 'FAIL  Orca says 'Background job cancelled''`
  - `run 20260925-130635 focus sequence: '13:06:44.899455 ('push button', 'Cancel')' then '13:06:45.031329 ('status bar', 'Background job')' then '13:06:46.806177 ('status bar', 'Background job complete')'`
  - `run 20260925-130912: '13:09:24.410092 ('push button', 'Cancel')' then '13:09:24.573453 ('status bar', 'Background job')'`
  - `toast-focus-second (run 20260925-130441): '+28.2 ms object:state-changed:focused 1 [notification] 'Sticky error #1'', '+28.3 ms object:state-changed:focused 0 [status bar] 'Warning #2''`
  - `toast-job-cancel-atspi (run 20260925-131353) steps: 'attempt 1: no node matches {...'name': 'Cancel'}', 'attempt 2: no node matches', 'attempt 3: no node matches', 'attempt 4: do_action returned True', then '+3318.7 ms object:announcement [status bar] 'Background job complete' text='Background job complete''`
  - `crates/teksilo-core/src/widget_tree/layout_impl.rs:355-373 (focus_owner = outermost rebuild root holding focus, here the ToastHost), :405 (pending_focus_restore = Some(root)), :862-868 (focus_ops(first_focusable_descendant(root)), the oldest toast surface)`
  - `crates/teksilo-widgets/src/toast/host.rs:273-319: every surface, with its buttons, is rebuilt on every queue change`
  - `toast-job-cancel-keys-20260925-132657-3027627: '+1357.6 ms object:state-changed:focused 1 [push button] 'Cancel'', '+1490.0 ms object:announcement [status bar] 'Background job'', '+1498.0 ms object:state-changed:focused 1 [status bar] 'Background job'', '+1498.0 ms object:state-changed:focused 0 [push button] 'Cancel'', '+1498.1 ms object:state-changed:defunct 1 [push button] 'Cancel''; focus then lands on a new 'Background job' node at every step up to +3082.1 ms; 'FAIL  Orca says 'Background job cancelled''`
  - `toast-job-cancel-keys-20260925-132933-3166981 and -133112-3261187: the same FAIL; each run has anns {'Background job': 20, 'Background job complete': 1}`
  - `toast-focus-second-20260925-132819-3098376 / -133217-3315388 / -133415-3450046: 'FAIL  focus stays, untouched, on [status bar] 'Warning #2''; focus goes to [notification] 'Sticky error #1'`
  - `toast-job-cancel-atspi-20260925-132714-3045264 / -132951-3187947 / -133241-3340151: 'attempt 1: do_action returned True', then 'Background job cancelled' announced`
- **Reproduced:** keyboard cancel: 3 of 3 runs never cancelled; focus-second focus jump 3 of 3 runs; AT-SPI click cancelled in 2 of 3 runs, silently lost in 1 of 3
- **Verification:** confirmed. Reproduced: keyboard cancel: 3 of 3 runs never cancelled. Focus was pulled off Cancel 132 to 141 ms after it arrived. toast-focus-second 3 of 3: focus jumped to 'Sticky error #1'. AT-SPI click on Cancel: cancelled at the first attempt in 3 of 3 of my runs; the sweep's 1-of-3 silent loss was not reproduced.
- **Fix idea:** Primarily fix toast-01: keep surfaces and their children alive across host rebuilds, keyed by entry\_id, so the focused Cancel survives a progress update. In the core, a restore could target the rebuilt counterpart of the focused widget (same key or path) rather than the first focusable descendant of the rebuild root, which for any container of peers means 'the first peer'.

### toast-05 {#toast-05}

A toast that arrives while focus is inside a toast or on the bell is cut by Orca re-reading the re-focused node

- **Example:** toast-demo
- **Scenario:** toast-focus-survives, toast-focus-second
- **Act:** toast-focus-survives: focus on 'Sticky error #1', then AT-SPI click on Info; then focus on the bell, AT-SPI click on Success. toast-focus-second: focus on 'Warning #2', AT-SPI click on Success.
- **The reader should get:** The new toast ('Info notice #2', 'Saved #3') is heard in full, and focus stays where the reader left it.
- **The reader gets:** The rebuild replaces the focused node (the toast, or the bell, which NotificationCenterButton rebuilds on every archive change) and focus is set on the fresh node. That focus event comes a few ms after the announcements, so Orca stops speech to present the 'new' focus: the new toast's title is cut about 40 ms in, and the reader hears 'notification Sticky error #1. This one persists…' or 'Notifications push button.' instead. In the bell case, 'Saved #3' was heard whole only about 2.2 s later, when the Info toast's expiry re-announced every toast (toast-01).
- **Platform:** Linux AT-SPI/Orca measured (the cut is Orca's stop on a focus change, default.py ~698-705). The focus move itself happens on every platform; NVDA reads the new focus after a live region, so on Windows the extra re-read is heard rather than the new toast cut.
- **Severity:** high; **layer:** framework
- **Status:** Open. In the announce-focus fix topic, not fixed there: Toast surfaces are their own live regions (nodes added), not announcer messages. The cut comes from rebuilding and re-focusing the focused bell or toast (center\_button.rs:248-252 binds at Rebuild). Fix in the widgets: do not rebuild the focused control.
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:248-252; crates/teksilo-widgets/src/toast/host.rs:273-320; crates/teksilo-core/src/widget\_tree/layout\_impl.rs:862-868
- **Evidence:**
  - `toast-focus-survives (run 20260925-130412), AT-SPI click on Info: '+26.0 ms object:announcement [status bar] 'Info notice #2' text='Info notice #2'', '+29.2 ms object:announcement [notification] 'Sticky error #1' text='Sticky error #1'', '+32.7 ms object:state-changed:focused 1 [notification] 'Sticky error #1'', '+32.7 ms object:state-changed:focused 0 [notification] 'Sticky error #1'', '+57.5 ms ORCA SAYS (CUT): 'Info notice #2'', '+161.9 ms ORCA SAYS: 'notification Sticky error #1.''`
  - `toast-focus-survives (run 20260925-131250), bell act, orca-debug.out: '13:13:13.793470 - SPEECH OUTPUT: 'Saved #3'', '13:13:13.832359 - NULL SPEECH: stop', '13:13:13.832427 - SPEECH OUTPUT: 'Notifications push button.'', '13:13:16.051132 - SPEECH OUTPUT: 'Saved #3'' (the later one comes from Info's expiry)`
  - `toast-focus-survives (run 20260925-130412), bell act: '+51.2 ms object:state-changed:focused 1 [push button] 'Notifications'', '+51.3 ms object:state-changed:focused 0 [push button] 'Notifications'', '+51.4 ms object:state-changed:defunct 1 [push button] 'Notifications''`
  - `toast-focus-second (run 20260925-131319), orca-debug.out: '13:13:40.660994 - SPEECH OUTPUT: 'Saved #3'', '13:13:40.698351 - NULL SPEECH: stop', '13:13:40.698417 - SPEECH OUTPUT: 'notification Sticky error #1.''`
  - `crates/teksilo-widgets/src/notification/center_button.rs:248-252 binds the archive version at BindingLevel::Rebuild (the bell is rebuilt on every toast, and at each of the job's 20 steps: toast-job-cancel-keys run 20260925-130250 has 'Notifications push button.' at +821.6, +907.6 and +1055.2 ms)`
  - `crates/teksilo-core/src/widget_tree/layout_impl.rs:862-868 re-focuses the fresh node`
  - `toast-focus-survives-20260925-132750-3077511: '+18.8 ms object:announcement [status bar] 'Info notice #2'', '+21.3 ms object:announcement [notification] 'Sticky error #1'', '+23.8 ms object:state-changed:focused 1 [notification] 'Sticky error #1'', '+43.8 ms ORCA SAYS (CUT): 'Info notice #2'', '+115.7 ms ORCA SAYS: 'notification Sticky error #1.''`
  - `same run, bell act: '+28.2 ms object:state-changed:focused 1 [push button] 'Notifications'', '+66.7 ms ORCA SAYS (CUT): 'Saved #3'', '+123.0 ms ORCA SAYS: 'Notifications push button.''; 'Saved #3' is heard whole only at +2286.5 ms, when Info's expiry re-announces the toasts (toast-01)`
  - `verify-toast-popover-toast-20260925-132919-3155812 orca-debug.out: '13:29:34.480224 SPEECH OUTPUT: 'Warning #2'', '13:29:34.600077 NULL SPEECH: stop', '13:29:34.600227 SPEECH OUTPUT: 'Notifications push button.''`
- **Reproduced:** focus-survives 'Info notice #2' cut 3 of 3 runs; bell 'Saved #3' first utterance cut 3 of 3 runs; focus-second 'Saved #3' cut 3 of 3 runs
- **Verification:** confirmed. Reproduced: toast-focus-survives 3 of 3 (the first 'Info notice #2' was cut about 70 ms in; on the bell, the first 'Saved #3' was cut and the reader heard 'Notifications push button.'). toast-focus-second 3 of 3 (the first 'Saved #3' was cut, then 'notification Sticky error #1.' was read).
- **Fix idea:** Do not rebuild the focused control. The bell only needs its badge (and its accessible count, toast-10) updated: bind the count reactively rather than rebuilding the PopoverIconButton. Keep toast surfaces across host rebuilds (toast-01).

### toast-06 {#toast-06}

A toast shown while another is up loses the idle time and expires with the older one (e.g. an error toast gone after about 1 s)

- **Example:** toast-demo
- **Scenario:** toast-lifetime, toast-severities
- **Act:** toast-lifetime: Space on Info, then 5 s later an AT-SPI click on Success; the bus is watched until both are gone. Also seen in toast-severities.
- **The reader should get:** Each toast stays its own 10 s, so a reader has as long to reach the second toast (to Tab to it and hear its body or its action) as the first.
- **The reader gets:** 'Saved #2' is removed together with 'Info notice #1' at the 10 s mark of the first toast: on the bus for 4.80 to 4.81 s instead of 10. In one toast-severities run, the Error toast 'Build #4 failed', shown about 9 s after the Info toast, was removed about 1 s after it appeared, together with all four toasts.
- **Platform:** all platforms (framework timer logic); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: each toast gets its own time. 'Saved #2' shown 5 s after another now stays about 10 s..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:228-231, 341-358; crates/teksilo-widgets/src/toast/registry.rs:710-722; crates/teksilo-core/src/widget\_tree.rs:1380-1395
- **Evidence:**
  - `toast-lifetime (run 20260925-130555): ''Saved #2' first added +5221.2 ms, last removed +10031.2 ms: 4.81 s on the bus'; (run 20260925-130832): ''Saved #2' first added +5230.6 ms, last removed +10028.3 ms: 4.80 s on the bus'; (run 20260925-131806): ''Saved #2' first added +5222.6 ms, last removed +10029.1 ms: 4.81 s on the bus'`
  - `toast-lifetime (run 20260925-130249): '+5216.8 ms object:children-changed:add [frame] '' -> [status bar] 'Saved #2'', '+10029.0 ms object:children-changed:remove [frame] '' -> [status bar] 'Info notice #1'', '+10029.1 ms object:children-changed:remove [frame] '' -> [status bar] 'Saved #2''`
  - `toast-severities (run 20260925-130112), Space on Error: '+121.8 ms object:children-changed:add [frame] '' -> [notification] 'Build #4 failed'', then '+1072.7 ms object:children-changed:remove [frame] '' -> [notification] 'Build #4 failed'' (with the other three toasts, removed within 3 ms of each other)`
  - `crates/teksilo-widgets/src/toast/host.rs:336-358: last_tick_at is stamped only when None; the frame-tick effect computes dt = now - last tick, i.e. the whole idle stretch since the last frame, and passes it to tick_timers`
  - `crates/teksilo-widgets/src/toast/registry.rs:710-722: tick_timers subtracts the same dt from every live entry, including one enqueued on this very frame with its full 10 s`
  - `toast-lifetime-20260925-132718-3051262 / -132959-3197164 / -133433-3469591: ''Saved #2': added 5.17-5.19 s, removed 9.99 s, on the bus 4.80-4.82 s'`
  - `verify-toast-lifetime-keys-20260925-133020-3208452 / -133455-3500003 / -133521-3534831: ''Saved #2': added 5.62 s, removed 15.26 s, on the bus 9.64 s'`
  - `toast-severities-20260925-133255-3359228: 'Info notice #1' 10.00 s, 'Saved #2' 7.33 s, 'Warning #3' 4.65 s, 'Build #4 failed' 1.99 s, all removed at 10.00 s`
  - `toast-job-with-info-20260925-133414-3449813: ''Background job complete': added 5.76 s, removed 10.00 s, on the bus 4.24 s' (auto_dismiss_after 5 s, examples/toast_demo/src/main.rs:398)`
  - `crates/teksilo-core/src/widget_tree.rs:1380-1395: advance_frame_tick returns early unless take_frame_tick_request()`
- **Reproduced:** toast-lifetime 4 of 4 runs (4.80 to 4.81 s); the early loss of a new toast is also visible in toast-severities run 20260925-130112
- **Verification:** corrected by the verifier. Reproduced: Second toast raised by an AT-SPI click: 3 of 3 ('Saved #2' on the bus 4.80, 4.81 and 4.82 s). toast-severities: 3 of 3 (all four toasts removed in one rebuild at 9.99 to 10.00 s after the first; 'Build #4 failed' lived 1.92, 1.97 and 1.99 s). A background toast, the 5 s 'Background job complete' shown with Info up: 3 of 3 (4.24 to 4.26 s). Not reproduced when the second toast was raised by Tab then Space: 3 of 3 lived 9.64 s (verify-toast-lifetime-keys). Real, but narrower and more precise than stated. The frame-tick signal the host's timer effect listens to advances only on frames that requested a tick (widget\_tree.rs:1380-1395, advance\_frame\_tick). A rebuild that adds a toast does not run the effect. The wake deadline is merged to the earliest one (host.rs:228-231), and last\_tick\_at is stamped only at the first arm (host.rs:341-343). The first tick then charges every live entry for the whole interval since the last tick (host.rs:346-358, registry.rs:710-722), including entries added after that tick. So a toast is cut short by however long nothing requested a frame tick before it arrived. That covers toasts raised by background events (the job's completion toast, 3 of 3), by an AT-SPI activation, and by a Space on a button focused through AT-SPI (toast-severities). In each of these cases, every default-length toast shown while an older timed toast is up leaves at the older toast's deadline. With Tab then Space, something ticked in between and only about 0.36 s was lost (3 of 3). Notifications usually come from background events, so high stands.
- **Fix idea:** Tick each entry from its own start: store an arrival or deadline Instant per entry, or bring the elapsed time up to date (tick with dt up to now) before enqueueing a new entry, so a new toast never pays for idle time before it existed.

### toast-07 {#toast-07}

A timed toast expires with keyboard focus on its action, and focus falls to the bare frame

- **Example:** toast-demo
- **Scenario:** toast-error-action
- **Act:** toast-error-action: Tab x9 to the Error toast's 'Show errors', then stay there reading and deciding for 10 s.
- **The reader should get:** While keyboard focus is inside the toast its timer is paused, as it is on hover (WCAG 2.2.1). If the toast does leave, focus moves somewhere meaningful.
- **The reader gets:** About 4.5 s after the reader reached 'Show errors', the toast expires under focus. The button goes defunct and focus goes to the unnamed frame, so Orca says 'frame.'. The reader never got to press the action.
- **Platform:** Linux AT-SPI/Orca measured; the timer and focus logic are the framework's, the same on every platform
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: main part): a toast no longer expires while keyboard or screen-reader focus is on it or inside it. 'Show errors' stays for 10 s and more..
- **Where:** crates/teksilo-widgets/src/toast/host.rs:212-233, 346-371; crates/teksilo-widgets/src/toast/registry.rs:700-722
- **Evidence:**
  - `toast-error-action (run 20260925-130506): '+4500.5 ms object:state-changed:focusable 1 [frame] ''', '+4501.2 ms object:state-changed:focused 1 [frame] ''', '+4501.3 ms object:state-changed:focused 0 [push button] 'Show errors'', '+4501.4 ms object:state-changed:defunct 1 [push button] 'Show errors'', '+4604.3 ms ORCA SAYS: 'frame.''`
  - `orca-debug.out (run 20260925-131344): '13:13:56.739792 - SPEECH OUTPUT: 'Show errors push button.'', then '13:14:03.223470 - SPEECH OUTPUT: 'frame.''`
  - `crates/teksilo-widgets/src/toast/host.rs:354-358 and registry.rs:710-713: timers pause only for hover (hover_count) or a held press, never for focus inside a toast; layout_impl.rs:862-868 finds no focusable descendant once the last toast is gone, so focus stays None and lands on the frame`
  - `toast-error-action-20260925-132844-3120707: '+4498.3 ms object:state-changed:focused 1 [frame] ''', '+4498.3 ms object:state-changed:focused 0 [push button] 'Show errors''; orca-debug.out '13:28:56.557053 SPEECH OUTPUT: 'Show errors push button.'' then '13:29:02.296183 SPEECH OUTPUT: 'frame.''`
  - `toast-error-action-20260925-133208-3309823 and -133548-3567817: 'FAIL  focus stays, untouched, on [push button] 'Show errors''`
  - `toast-job-cancel-keys (3 runs), events.jsonl: focus -> [status bar] 'Background job complete' at completion, then focus -> [frame] '' +4.99/+5.00/+5.00 s later`
- **Reproduced:** 3 of 3 runs
- **Verification:** confirmed. Reproduced: 3 of 3. Also 3 of 3 in toast-job-cancel-keys: when the job completes, focus sits on 'Background job complete' (the rebuilds had put it on the job toast) and falls to the frame 4.99 to 5.00 s later, when that toast times out.
- **Fix idea:** Treat focus within a toast (a focus\_within signal on the surface) as a pause, like hover. Consider not auto-dismissing a toast that carries actions, or at least not while focus is inside it.

### toast-08 {#toast-08}

Dismissing a toast (close button or Escape) leaves focus on the unnamed frame

- **Example:** toast-demo
- **Scenario:** toast-dismiss
- **Act:** toast-dismiss: Tab into a persistent error toast, Tab to its close button, Space; later Escape on a focused toast; then Tab.
- **The reader should get:** Focus returns to where the reader was before entering the toast (or to the bell, or to the next toast), and the reader hears it.
- **The reader gets:** Focus goes to the frame: Orca says 'frame.' (with K1, the frame has no name either). The next Tab restarts at the top of the window ('Toolbar tool bar', 'Info push button.').
- **Platform:** Linux AT-SPI/Orca measured; the framework focus logic is the same on every platform
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/registry.rs:621-641 (dismiss\_entry); crates/teksilo-core/src/widget\_tree/layout\_impl.rs:862-868
- **Evidence:**
  - `toast-dismiss (run 20260925-130322), Space on the close button: '+27.5 ms object:state-changed:focusable 1 [frame] ''', '+27.8 ms object:state-changed:focused 1 [frame] ''', '+27.9 ms object:state-changed:focused 0 [push button] 'Clear'', '+70.8 ms ORCA SAYS: 'frame.''`
  - `same run, Escape on the focused toast: '+7.4 ms object:state-changed:focused 1 [frame] ''', '+7.5 ms object:state-changed:focused 0 [notification] 'Sticky error #2'', '+84.9 ms ORCA SAYS: 'frame.''`
  - `orca-debug.out (run 20260925-131421): '13:14:41.888336 - SPEECH OUTPUT: 'frame.'', '13:14:41.896051 - EVENT MANAGER: Ignoring defunct object: [push button: 'Clear']'`
  - `Tab after the dismissal: '+5.7 ms object:state-changed:focused 1 [push button] 'Info'', 'ORCA SAYS: 'Toolbar tool bar'', 'ORCA SAYS: 'Info push button.''`
  - `crates/teksilo-core/src/widget_tree/layout_impl.rs:862-868: the restore needs a focusable descendant of the rebuilt ToastHost; with the last toast gone there is none and focus stays None. With other toasts left, the same code would put focus on the oldest toast (from source, not measured)`
  - `toast-dismiss-20260925-132701-3030915: '+26.8 ms object:state-changed:focused 1 [frame] ''', '+76.0 ms ORCA SAYS: 'frame.''; Escape: '+7.1 ms object:state-changed:focused 1 [frame] ''', '+52.4 ms ORCA SAYS: 'frame.''`
  - `verify-toast-show-errors-20260925-133119-3266998 / -133954-3767592: '+28.5 ms object:state-changed:focused 1 [frame] ''', '+90.9 ms ORCA SAYS: 'frame.''; app.log '[demo] Show errors clicked'`
  - `verify-toast-dismiss-one-of-two-20260925-133921-3743192: '+29.7 ms object:announcement [notification] 'Sticky error #1'', '+33.2 ms object:state-changed:focused 1 [notification] 'Sticky error #1'', '+48.4 ms ORCA SAYS (CUT): 'Sticky error #1'', '+88.4 ms ORCA SAYS: 'notification Sticky error #1.''`
- **Reproduced:** 3 of 3 runs, both the close button and Escape
- **Verification:** corrected by the verifier. Reproduced: toast-dismiss 3 of 3, both the close button and Escape. Also verify-toast-show-errors 2 of 2: pressing the toast's own action 'Show errors', which closes the toast, sends focus to the frame the same way. Real, with one correction to scope: focus goes to the frame only when the dismissed toast was the last one. With another toast still up, the restore at layout\_impl.rs:862-868 puts focus on the oldest toast. That toast is re-announced, the announcement is cut, and Orca then reads it as a focus (verify-toast-dismiss-one-of-two, 2 of 2). The unnamed frame is K1 and only changes the word Orca says ('frame.'); the focus loss is separate from K1.
- **Fix idea:** When a toast holding focus is dismissed, move focus deliberately: to the next toast if there is one, otherwise to the control focused before the reader entered the toast region (remember it on focus-in), or to the bell.

### toast-09 {#toast-09}

The toast's close button is named 'Clear'

- **Example:** toast-demo
- **Scenario:** toast-dismiss
- **Act:** toast-dismiss: Tab from a focused toast to its close button.
- **The reader should get:** The button says what it does: 'Close' or 'Dismiss notification'.
- **The reader gets:** 'Clear push button.', the label of the text-field clear icon. It suggests clearing text or content, not closing the notification.
- **Platform:** all platforms (the accessible name); measured on Linux
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:265-278
- **Evidence:**
  - `toast-dismiss (run 20260925-130322): '+6.0 ms object:state-changed:focused 1 [push button] 'Clear'', '+54.1 ms ORCA SAYS: 'Clear push button.''; 'FAIL  Orca says any of ('Close', 'Dismiss')'`
  - `tree: '[push button] 'Clear' desc='Clear' {focusable}' under every toast`
  - `crates/teksilo-widgets/src/toast/surface.rs:270 uses IconButton::clear(); crates/teksilo-widgets/src/icon_button.rs:505-508 names it tr_widget!(a11y_builtin_clear()) = 'Clear'`
  - `toast-dismiss-20260925-132701-3030915: '+5.3 ms object:state-changed:focused 1 [push button] 'Clear'', '+54.4 ms ORCA SAYS: 'Clear push button.''; 'FAIL  Orca says any of ('Close', 'Dismiss')'`
- **Reproduced:** 3 of 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: 3 of 3 (deterministic)
- **Fix idea:** Give the toast close button its own localized label (for example 'Dismiss notification', or 'Close' plus the toast title) through .access\_label or a dedicated IconButton::close().

### toast-10 {#toast-10}

The bell does not tell a reader how many notifications are unread

- **Example:** toast-demo
- **Scenario:** toast-bell
- **Act:** toast-bell: two toasts shown, then Tab to the bell.
- **The reader should get:** 'Notifications, 2 unread, push button' (the unread count is the bell's state).
- **The reader gets:** 'Notifications push button.' The count sits in a separate \[label\] '2' beside the button in the status bar, reachable only by flat review and meaningless alone.
- **Platform:** all platforms (the tree); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:19-21, 255, 318-327
- **Evidence:**
  - `toast-bell (run 20260925-131212): '+5.7 ms object:state-changed:focused 1 [push button] 'Notifications'', orca-debug.out '13:12:25.559461 - SPEECH OUTPUT: 'Notifications push button.''; 'FAIL  Orca says any of ('2 unread', '2 new', '2 notifications', 'Notifications 2', 'Notifications, 2', '2')'`
  - `tree after the act: '[status bar] 'Status'' > '[push button] 'Open log dialog' {focusable}', '[push button] 'Notifications' desc='Notifications' {focusable,focused}', '[label] '2''`
  - `crates/teksilo-widgets/src/notification/center_button.rs:255 (bell named by its tooltip only), :337 (Badge::new(lit!(label)) added as a separate sibling); the module doc at center_button.rs:19-21 says the count is not announced`
  - `toast-bell-20260925-133334-3406209: '+6.9 ms object:state-changed:focused 1 [push button] 'Notifications'', '+54.2 ms ORCA SAYS: 'Notifications push button.''; tree: '[push button] 'Notifications' desc='Notifications' {focusable}', '[label] '2''`
- **Reproduced:** 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: 2 of 2 toast-bell runs (deterministic)
- **Fix idea:** Put the count in the button's accessible name or description ('Notifications, 2 unread'), bound reactively, and hide the badge label from AT.

### toast-11 {#toast-11}

The bell's popover is an unnamed dialog, which Orca names 'Today' from its first label

- **Example:** toast-demo
- **Scenario:** toast-bell, toast-bell-escape
- **Act:** toast-bell / toast-bell-escape: Space on the bell.
- **The reader should get:** 'Notifications dialog'.
- **The reader gets:** 'dialog Today' then 'Notifications.' 'list.' 'Mark all read push button.' The dialog node has no name, so Orca guesses one from the day-bucket header.
- **Platform:** Linux AT-SPI/Orca measured; the missing name reaches every platform
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:288-296
- **Evidence:**
  - `toast-bell (run 20260925-131212), orca-debug.out: '13:12:29.644675 - SPEECH OUTPUT: 'dialog Today'', '13:12:29.644719 - SPEECH OUTPUT: 'Notifications.'', '13:12:29.644735 - SPEECH OUTPUT: 'list.'', '13:12:29.644753 - SPEECH OUTPUT: 'Mark all read push button.''`
  - `tree: '[dialog] '' {active}' > '[list] 'Notifications'' > ... '[label] 'Today''; 'FAIL  the tree holds [dialog] 'Notifications''`
  - `crates/teksilo-widgets/src/notification/center_button.rs:288-293 builds the PopoverIconButton without .surface_name(...); crates/teksilo-widgets/src/popover_widget.rs:505-512 provides it (empty by default)`
  - `toast-bell-20260925-133334-3406209: '+48.5 ms object:children-changed:add [status bar] 'Status' -> [dialog] ''', '+142.3 ms ORCA SAYS: 'dialog Today''`
- **Reproduced:** 4 of 4 openings in 3 runs (deterministic)
- **Verification:** confirmed. Reproduced: Every opening in my runs: toast-bell 2 of 2, toast-bell-escape 2 of 2, verify-toast-popover-toast 3 of 3, verify-toast-popover-mark-read 2 of 2
- **Fix idea:** .surface\_name(tr\_widget!(notifications\_title()).resolve\_now()) on the PopoverIconButton, or label the dialog by the log (labelled\_by).

### toast-12 {#toast-12}

The notification log's entries cannot be reached by keyboard, and they are not list items

- **Example:** toast-demo
- **Scenario:** toast-log-dialog, toast-bell
- **Act:** toast-bell: Space on the bell, then Tab x3 in the popover. toast-log-dialog: Space on Open log dialog, then Tab x4.
- **The reader should get:** The rows are list items of the 'Notifications' list, reachable by Tab or arrow keys, each read with its title and body.
- **The reader gets:** The \[list\] 'Notifications' holds the two toolbar buttons and an unnamed \[panel\] of a header label and rows of role \[unknown\], none focusable. In the dialog, Tab only cycles 'Mark all read' and 'Clear all'. In the popover, the second Tab leaves it, the popover is destroyed, and focus lands on the toolbar's 'Info'. A reader hears the entries only through flat review. An archived action reads as a bare 'Show errors (no longer available)', and an entry with no body carries its title again as description.
- **Platform:** all platforms (tree and focus order); measured on Linux
- **Severity:** high; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/notification/log.rs:262-330, 440-470, 554-557
- **Evidence:**
  - `toast-log-dialog tree (run 20260925-130640): '[dialog] 'Notifications' {active,modal}' > '[list] 'Notifications'' > '[push button] 'Mark all read' {focusable,focused}', '[push button] 'Clear all' {focusable}', '[panel] ''' > '[label] 'Today'', '[unknown] 'Build #2 failed' desc='Three errors in src/main.rs, two warnings.'' > '[label] 'Show errors (no longer available)'', '[unknown] 'Info notice #1' desc='Info notice #1''`
  - `toast-log-dialog (run 20260925-131515), orca-debug.out: '13:15:36.434937 - SPEECH OUTPUT: 'Clear all push button.'', '13:15:37.379696 - SPEECH OUTPUT: 'Mark all read push button.'', '13:15:38.043262 - SPEECH OUTPUT: 'Clear all push button.''; 'FAIL  Orca says any of ('Build #2 failed',)'`
  - `toast-bell (run 20260925-130542), Tab x3 in the popover: '+847.9 ms object:state-changed:focused 1 [push button] 'Info'', '+848.1 ms object:state-changed:defunct 1 [list] 'Notifications'', '+848.3 ms object:state-changed:defunct 1 [unknown] 'Warning #2'', '+848.6 ms object:state-changed:defunct 1 [dialog] ''', '+958.4 ms ORCA SAYS (CUT): 'Toolbar tool bar''`
  - `crates/teksilo-widgets/src/notification/log.rs:268 and :464 place StandardListItem rows straight into a VStack inside a ScrollArea; crates/teksilo-widgets/src/standard_item.rs:916-930 sets only name and description and leaves role, position and focus to a ListView wrapper that is absent here; log.rs:555-558 names the root a List whose children are not items`
  - `toast-log-dialog-20260925-133432-3466428 tree: '[list] 'Notifications'' > '[push button] 'Mark all read' {focusable,focused}', '[push button] 'Clear all' {focusable}', '[panel] ''' > '[label] 'Today'', '[unknown] 'Build #2 failed' desc='Three errors in src/main.rs, two warnings.'' > '[label] 'Show errors (no longer available)'', '[unknown] 'Info notice #1' desc='Info notice #1''; Tab x4 only alternates 'Clear all' and 'Mark all read'`
  - `toast-bell-20260925-133334-3406209, second Tab in the popover: '+848.5 ms object:state-changed:focused 1 [push button] 'Info'', '+848.8 ms object:state-changed:defunct 1 [dialog] '''`
- **Reproduced:** toast-log-dialog 3 of 3 runs; toast-bell 2 of 2 runs (deterministic)
- **Verification:** confirmed. Reproduced: toast-log-dialog 2 of 2, toast-bell 2 of 2 (deterministic)
- **Fix idea:** Render the rows through a ListView over the archive (Role::List &gt; Role::ListItem, posinset/setsize, arrow-key navigation, one tab stop), or give each row Role::ListItem and focusability. Move the toolbar buttons out of the List node. Name archived actions in context ('Show errors, not available from the log').

### toast-13 {#toast-13}

A focused toast reads its body twice

- **Example:** toast-demo
- **Scenario:** toast-dismiss, toast-focus-survives, toast-error-action
- **Act:** toast-dismiss / toast-focus-survives / toast-error-action: Tab onto a toast.
- **The reader should get:** 'notification Sticky error #1. This one persists until you dismiss it.'
- **The reader gets:** 'notification Sticky error #1.' 'This one persists until you dismiss it.' 'This one persists until you dismiss it.' (and for a status toast: 'Warning #2 statusbar.' 'Take a look when you have a moment.' 'Clear push button.' 'Take a look when you have a moment.').
- **Platform:** Linux AT-SPI/Orca measured (Orca speaks the body label child as an unrelated label, then the node's description). Other readers that read both description and content would repeat it too; not verified.
- **Severity:** medium; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:443-445 with the CollapsibleBody label at surface.rs:211-225
- **Evidence:**
  - `toast-dismiss (run 20260925-130322): '+1200.4 ms ORCA SAYS: 'notification Sticky error #1.'', '+1200.5 ms ORCA SAYS: 'This one persists until you dismiss it.'', '+1200.5 ms ORCA SAYS: 'This one persists until you dismiss it.''; 'FAIL  Orca says the body once'`
  - `Orca generator trace (run 20260925-131041): 'unrelatedLabelsOrDescription=[This one persists until you dismiss it.]' then 'description=[This one persists until you dismiss it.]'`
  - `tree: '[notification] 'Build #2 failed' desc='Three errors in src/main.rs, two warnings.' {focusable}' > '[label] 'Three errors in src/main.rs, two warnings.''`
  - `crates/teksilo-widgets/src/toast/surface.rs:442-444 copies the body into the description while the CollapsibleBody label (surface.rs:218-229) stays visible to AT; /usr/lib/python3/dist-packages/orca/formatting.py:381-382 (notification format)`
  - `toast-dismiss-20260925-132701-3030915 orca-debug.out: 'unrelatedLabelsOrDescription=[This one persists until you dismiss it.]' then 'description=[This one persists until you dismiss it.]'; '+1167.7 ms ORCA SAYS: 'notification Sticky error #1.'', '+1167.7 ms ORCA SAYS: 'This one persists until you dismiss it.'', '+1167.8 ms ORCA SAYS: 'This one persists until you dismiss it.''`
  - `toast-focus-second-20260925-132819-3098376: 'Warning #2 statusbar.' 'Take a look when you have a moment.' 'Clear push button.' 'Take a look when you have a moment.'`
- **Reproduced:** every focus on a toast with a body, in 9 runs (deterministic)
- **Verification:** confirmed. Reproduced: Every focus on a toast with a body: toast-dismiss 3 of 3, focus-survives 3 of 3, focus-second 3 of 3, dismiss-one-of-two 2 of 2
- **Fix idea:** Carry the body once: keep the visible label and drop the description (or point described\_by at the label), or keep the description and hide the label from AT, as is already done for the title.

### toast-14 {#toast-14}

Orca reads a focused toast with an earlier toast's contents (a Cancel button that no longer exists)

- **Example:** toast-demo
- **Scenario:** toast-job-cancel-keys
- **Act:** toast-job-cancel-keys: with focus pulled onto the job toast at each step, the job completes and focus lands on 'Background job complete'.
- **The reader should get:** 'Background job complete, status bar. All 20 items fetched. Clear button.'
- **The reader gets:** 'Background job complete statusbar.' 'progress bar.' '30% · Fetching item 6 of 20.' 'Cancel push button.' 'Clear push button.' 'All 20 items fetched.' These are the children of the first toast whose items Orca listed. The tree on the bus is right: the completed toast holds only \[label\] 'All 20 items fetched' and \[push button\] 'Clear'. Orca 46 caches one list of status-bar items per window (cleared only on window activation), and every toast except an error is exposed as a status bar.
- **Platform:** Linux, Orca 46.1 (upstream behaviour triggered by toasts exposed as AT-SPI status bars); not applicable to Windows or macOS
- **Severity:** medium; **layer:** upstream
- **Status:** Upstream, outside Teksilo: not fixed here.
- **Where:** crates/teksilo-widgets/src/toast/surface.rs:110-135
- **Evidence:**
  - `toast-job-cancel-keys (run 20260925-130635), orca-debug.out: '13:06:46.880226 - SPEECH OUTPUT: 'Background job complete statusbar.'', '13:06:46.880247 - SPEECH OUTPUT: 'progress bar.'', '13:06:46.880264 - SPEECH OUTPUT: '30% · Fetching item 6 of 20.'', '13:06:46.880283 - SPEECH OUTPUT: 'Cancel push button.''`
  - `run 20260925-130250, orca-debug.out: 'GENERATION TIME: 0.0048 ----> statusBar=[progress bar PAUSE 30% · Fetching item 6 of 20 PAUSE Cancel push button PAUSE Clear push button PAUSE]', '13:03:02.167808 - AXObject: [push button: 'Clear'] has index -1 ; parent [status bar: 'Background job'] has -1 children'`
  - `toast-job (run 20260925-130617), tree after completion: '[status bar] 'Background job complete' desc='All 20 items fetched' {focusable}' > '[label] 'All 20 items fetched'', '[push button] 'Clear' desc='Clear' {focusable}'; 'pass  the tree holds no [push button] 'Cancel''`
  - `/usr/lib/python3/dist-packages/orca/script_utilities.py:1642-1660 (statusBarItems cached in pointOfReference['statusBarItems']); scripts/default.py:1885, 1929 (reset only on window activation)`
  - `crates/teksilo-widgets/src/toast/surface.rs:109-119 maps non-error toasts to Role::Status (AT-SPI STATUS_BAR)`
  - `toast-job-cancel-keys-20260925-132657-3027627: '+3321.5 ms ORCA SAYS: 'Background job complete statusbar.'', 'progress bar.', '30% · Fetching item 6 of 20.', 'Cancel push button.', 'Clear push button.', 'All 20 items fetched.'; orca-debug.out 'statusBar=[progress bar PAUSE 30% · Fetching item 6 of 20 PAUSE Cancel push button PAUSE Clear push button PAUSE]'`
- **Reproduced:** 2 of 3 runs of toast-job-cancel-keys (in the third, Orca lagged and dropped the events); deterministic once a status-bar toast has been read in the window
- **Verification:** confirmed. Reproduced: 3 of 3 toast-job-cancel-keys runs (Orca never lagged in mine)
- **Fix idea:** Report upstream to Orca (statusBarItems assumes one status bar per window). In Teksilo, the main trigger here is re-focusing rebuilt toasts (toast-04). Using a single role for every toast (Role::Alert, AT-SPI 'notification', whose Orca format does not use the cache) would avoid it; weigh that against ARIA status semantics.

### toast-15 {#toast-15}

A loading toast's spinner is an unnamed progress bar with no value

- **Example:** toast-demo
- **Scenario:** toast-job-cancel-keys
- **Act:** toast-job-cancel-keys: focus lands on the job toast.
- **The reader should get:** A named progress bar carrying the job's percentage, or no progress node at all.
- **The reader gets:** 'progress bar.' with nothing after it. The percentage is only in the toast's description.
- **Platform:** all platforms (tree); measured on Linux
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast.rs:487-491; crates/teksilo-widgets/src/spinner.rs:191-198
- **Evidence:**
  - `toast-job-cancel-keys (run 20260925-130250): '+1121.8 ms ORCA SAYS (CUT): 'Background job statusbar.'', '+1121.8 ms ORCA SAYS (CUT): 'progress bar.'', '+1121.8 ms ORCA SAYS (CUT): '30% · Fetching item 6 of 20.''`
  - `toast-job-cancel-atspi (run 20260925-130308): '+1204.0 ms object:state-changed:defunct 1 [progress bar] '''`
  - `crates/teksilo-widgets/src/toast.rs:487-491 Toast::loading adds Spinner::new(16.0) with no label; crates/teksilo-widgets/src/spinner.rs:191-198 sets a name only when a label is given`
  - `toast-job-cancel-keys-20260925-132657-3027627: '+1124.4 ms ORCA SAYS (CUT): 'Background job statusbar.'', '+1124.4 ms ORCA SAYS (CUT): 'progress bar.''`
  - `crates/teksilo-widgets/src/spinner.rs:195 'builder.set_live(teksilo_core::accesskit::Live::Polite)'`
- **Reproduced:** every job run that focused the toast (3 of 3 keyboard-cancel runs); deterministic
- **Verification:** confirmed. Reproduced: 3 of 3 keyboard-cancel runs (deterministic)
- **Fix idea:** Hide the decorative spinner from AT, or label it ('Loading') and let Toast carry an optional numeric progress exposed as the indicator's value.

### toast-16 {#toast-16}

Names repeated as descriptions: bell, toast close button, log rows without a body

- **Example:** toast-demo
- **Scenario:** tree, toast-log-dialog
- **Act:** tree at launch and after toasts / log dialog
- **The reader should get:** No description when it only repeats the name.
- **The reader gets:** '\[push button\] 'Notifications' desc='Notifications'', '\[push button\] 'Clear' desc='Clear'', '\[unknown\] 'Info notice #1' desc='Info notice #1''. Orca drops a description equal to the name, so this was silent here; on Windows it becomes a FullDescription equal to the name (what NVDA or JAWS do with that was not verified).
- **Platform:** tree on all platforms; silent on Linux/Orca; Windows effect unverified
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-core/src/widget\_tree/accessibility\_emit\_impl.rs:641-650
- **Evidence:**
  - `tree-toast-demo launch tree: '[push button] 'Notifications' desc='Notifications' {focusable}'`
  - `toast-log-dialog tree: '[unknown] 'Info notice #1' desc='Info notice #1''`
  - `crates/teksilo-widgets/src/icon_button.rs:505-508 and 519-522 (clear/bell name themselves through a tooltip, which is also exposed as the description); crates/teksilo-widgets/src/notification/log.rs:298-304 (the row's rich tooltip text is the title when there is no body)`
  - `verify-toast-log-clear-all-20260925-133005-3200522 tree: '[push button] 'Notifications' desc='Notifications' {focusable}'`
  - `toast-log-dialog-20260925-133432-3466428 tree: '[unknown] 'Info notice #1' desc='Info notice #1''`
- **Reproduced:** deterministic, every run
- **Verification:** corrected by the verifier. Reproduced: deterministic, in every tree Real, but in the wrong place. The IconButton constructors only set a tooltip, and icon\_button.rs:868-874 names the button from it. The duplicate description is written by the core walker: accessibility\_emit\_impl.rs:641-650 copies an unshown tooltip's text into set\_description without comparing it with the name. The same code path gives a log row without a body its title as description.
- **Fix idea:** Skip the tooltip-derived description when it equals the accessible name.

### toast-v1 {#toast-v1}

Any toast shown or updated while the bell's popover is open closes the popover and marks the new notification read unseen; during a background job the popover stays open about 0.1 s

- **Example:** toast-demo
- **Scenario:** verify-toast-popover-toast, verify-toast-popover-during-job, verify-toast-popover-mark-read (tools/reader/scenarios/verify\_toast.py)
- **Act:** verify-toast-popover-toast: Space on the bell (the popover opens, focus on Mark all read), then an AT-SPI click on Warning. verify-toast-popover-during-job: Space on Start background job, Tab x2 to the bell, Space, listen 2 s. verify-toast-popover-mark-read: Space on Mark all read inside the popover.
- **The reader should get:** The popover stays open, with focus where the reader left it. The new toast is heard and shows up in the popover as an unread row. Mark all read does its job and leaves the reader in the popover.
- **The reader gets:** Every archive change (a new toast, or each in-place progress step) rebuilds NotificationCenterButton, which destroys the PopoverIconButton together with its open popover. The dialog leaves the tree 3 ms after the toast's announcements and focus goes to a new bell node. Orca cuts both toast titles and says 'Notifications push button.'. The popover's on\_close then marks everything read, the notification that just arrived included: the badge '2' leaves the tree in the same act, so the reader never sees 'Warning #2' as unread. During a job the popover is removed 105 to 111 ms after it opens. After that the bell is re-focused every 160 ms, and Orca says 'Notifications push button.' 16 times, mixed with 20 'Background job'. Mark all read also closes the popover and lands on the bell, and nothing tells the reader what happened.
- **Platform:** Linux AT-SPI/Orca measured. The rebuild, the popover teardown and the focus move are the framework's own logic, so they happen on every platform.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: the bell popover stays open, with focus where the reader left it, when a toast arrives, during a job, and after Mark all read..
- **Where:** crates/teksilo-widgets/src/notification/center\_button.rs:248-252, 262-296; crates/teksilo-widgets/src/toast/registry.rs:401; crates/teksilo-widgets/src/notification/archive.rs:340-341
- **Evidence:**
  - `verify-toast-popover-toast-20260925-132919-3155812, AT-SPI click on Warning: '+23.8 ms object:announcement [status bar] 'Info notice #1'', '+26.0 ms object:announcement [status bar] 'Warning #2'', '+28.1 ms object:children-changed:remove [status bar] 'Status' -> [dialog] ''', '+28.4 ms object:state-changed:focused 1 [push button] 'Notifications'', '+28.5 ms object:state-changed:focused 0 [push button] 'Mark all read'', '+31.7 ms object:children-changed:remove [status bar] 'Status' -> [label] '2''; 'FAIL  a dialog is in the tree after the act'`
  - `same run, orca-debug.out: '13:29:34.452397 SPEECH OUTPUT: 'Info notice #1'', '13:29:34.480224 SPEECH OUTPUT: 'Warning #2'', '13:29:34.600077 NULL SPEECH: stop', '13:29:34.600227 SPEECH OUTPUT: 'Notifications push button.''`
  - `verify-toast-popover-during-job (3 runs): dialog added at +1162/+1176/+1177 ms, removed at +1273/+1283/+1287 ms, focus on a new 'Notifications' 13 more times; utterances: ('Background job', 20), ('Notifications push button.', 16)`
  - `verify-toast-popover-mark-read-20260925-133517-3528908 / -133539-3555899: Space on Mark all read -> '+32.1 ms object:state-changed:focused 1 [push button] 'Notifications'', '+136.5 ms ORCA SAYS: 'Notifications push button.''; 'FAIL  a dialog is in the tree after the act'`
  - `crates/teksilo-widgets/src/notification/center_button.rs:248-252 binds archive.version_signal() at BindingLevel::Rebuild. The comment at :262-270 already says a rebuild would tear down the PopoverIconButton and its overlay; :292-294 on_close calls mark_all_read. toast/registry.rs:401 pushes to the archive on every in-place update, and notification/archive.rs:340-341 bumps the version on every push.`
- **Reproduced:** verify-toast-popover-toast 3 of 3; verify-toast-popover-during-job 3 of 3; verify-toast-popover-mark-read 2 of 2
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Stop rebuilding NotificationCenterButton when the archive changes: bind only the badge text and the accessible count reactively, so an archive change never destroys the PopoverIconButton. Update the log's rows in place (for example through a ListView over the archive). On close, mark read only the entries that were shown while the popover was open.

### toast-v2 {#toast-v2}

The notification log dialog rebuilds its content on every archive change: while a job runs, focus is thrown back to 'Mark all read' every 160 ms and 'Clear all' cannot be pressed

- **Example:** toast-demo
- **Scenario:** verify-toast-log-during-job, verify-toast-log-clear-all (tools/reader/scenarios/verify\_toast.py)
- **Act:** verify-toast-log-during-job: Space on Start background job, Tab to Open log dialog, Space, Tab to Clear all, listen 2.5 s. verify-toast-log-clear-all (no job): Space on Clear all.
- **The reader should get:** Focus stays on Clear all while notifications are archived behind it. After Clear all, the reader hears that the log was cleared.
- **The reader gets:** 'Clear all' is replaced 31 to 49 ms after focus reaches it, and focus lands on a new 'Mark all read'. In 2.5 s focus lands on 'Mark all read' 15 times and Orca says 'Mark all read push button.' 14 or 15 times, mixed with 20 'Background job'. Any single toast shown while the log is open does the same once, since every archive push rebuilds the log. Space on Clear all, with no job running, moves focus to 'Mark all read' and Orca says only 'Mark all read push button.'. The \[label\] 'No notifications' that replaces the rows is never spoken.
- **Platform:** Linux AT-SPI/Orca measured. The rebuild and the focus restore are framework logic, so they happen on every platform.
- **Severity:** high; **layer:** framework
- **Status:** Fixed by `0e2b6377` (toast). Fixed part: the log keeps focus on Clear all, Mark all read and unchanged rows while a job archives its steps, and Clear all says 'No notifications'..
- **Where:** crates/teksilo-widgets/src/notification/log.rs:356-360; crates/teksilo-widgets/src/toast/registry.rs:401
- **Evidence:**
  - `verify-toast-log-during-job-20260925-132951-3189779: '+1776.8 ms object:state-changed:focused 1 [push button] 'Clear all'', '+1801.7 ms object:announcement [status bar] 'Background job'', '+1808.2 ms object:state-changed:focused 1 [push button] 'Mark all read'', '+1808.3 ms object:state-changed:focused 0 [push button] 'Clear all'', '+1812.1 ms object:state-changed:defunct 1 [push button] 'Clear all''; 'FAIL  once on [push button] 'Clear all', focus stays there'`
  - `3 runs: 'Clear all' at +1743/+1723/+1732 ms, 'Mark all read' again at +1774/+1772/+1774 ms; utterances ('Background job', 20), ('Mark all read push button.', 15/15/14)`
  - `verify-toast-log-clear-all-20260925-133005-3200522, Space on Clear all: '+33.9 ms object:children-changed:add [list] 'Notifications' -> [label] 'No notifications'', '+34.6 ms object:state-changed:focused 1 [push button] 'Mark all read'', '+96.2 ms ORCA SAYS: 'Mark all read push button.''`
  - `crates/teksilo-widgets/src/notification/log.rs:356-360 binds archive.version_signal() at BindingLevel::Rebuild; crates/teksilo-core/src/widget_tree/layout_impl.rs:862-868 restores focus to the log's first focusable descendant`
- **Reproduced:** verify-toast-log-during-job 3 of 3; Clear all feedback 1 of 1 (deterministic)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Keep the log's toolbar out of the rebuild, and update the rows in place (a ListView over the archive) rather than rebuilding the whole NotificationLog. Consider archiving an in-place progress update at most once, not at every step. Announce 'Notifications cleared' after Clear all.

### toast-v3 {#toast-v3}

A reader tabbing forward at a listening pace reaches a toast just as it times out; Shift+Tab from the top is the only quick way in

- **Example:** toast-demo
- **Scenario:** verify-toast-reach-listening (tools/reader/scenarios/verify\_toast.py)
- **Act:** verify-toast-reach-listening: an Error toast appears, then Tab x9 from the Error button with 1 s per stop. Then, with a new Error toast up, Shift+Tab once from the first toolbar button.
- **The reader should get:** A reader who listens to each Tab stop reaches the toast's action before the toast leaves, or can jump straight to the notifications.
- **The reader gets:** Toasts are the last Tab stops (after the bell), and there is no key to jump to them. At 1 s per stop, focus lands on the toast at +7.27 s (the 8th stop), 0.37 s before the toast times out, 10 s after it appeared. Focus then falls to the frame, and the 9th Tab goes to 'Info'. Shift+Tab from the window's first control does reach the last toast's close button in one press: 'notification Build #2 failed.' 'Three errors in src/main.rs, two warnings.' 'Clear push button.'.
- **Platform:** Linux AT-SPI/Orca measured. The Tab order and the timer are framework logic, so they are the same on every platform.
- **Severity:** low; **layer:** framework
- **Status:** Open.
- **Where:** crates/teksilo-widgets/src/toast/host.rs:488-497
- **Evidence:**
  - `verify-toast-reach-listening-20260925-133634-3613536: '+7266.7 ms object:state-changed:focused 1 [notification] 'Build #1 failed'', '+7639.7 ms object:children-changed:remove [frame] '' -> [notification] 'Build #1 failed'', '+7639.9 ms object:state-changed:focused 1 [frame] '''; 'FAIL  focus lands on [push button] 'Show errors''`
  - `same run, Shift+Tab from Info: '+20.6 ms object:state-changed:focused 1 [push button] 'Clear'', '+80.7 ms ORCA SAYS: 'notification Build #2 failed.''`
  - `crates/teksilo-widgets/src/toast/host.rs:488-497: the host is a bare GenericContainer, with no region and no jump key`
- **Reproduced:** 3 of 3 (toast reached at +7.27 s and gone at +7.64 s in each run; Shift+Tab reached 'Clear' in each run)
- **Verification:** found by the verifier, which the sweep missed.
- **Fix idea:** Offer a shortcut that moves focus into the toast region (for example F6 or F8, as common toast libraries do), and pause every timer while focus is in the region (toast-07). The root cause is the fixed timeout; this item only shows the timeout is too short to reach a toast at a listening pace.
