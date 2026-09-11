<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch verification sign-off

> # ⚠ NOTHING BELOW HAS BEEN EXECUTED
>
> **This file is an empty template. Not one check in it has been run, on any
> device, by anyone. Every result cell is blank because no result exists.**
>
> The touch programme is **not signed off**. It is signed off when this file is
> filled in on real hardware at a release SHA, across the whole device matrix,
> with no unexplained failures — and not before, however green the automated
> suite is.
>
> Do not read a blank cell as a pass. Do not fill a cell in from reasoning, from
> another platform's result, or from what the code says it does: the whole reason
> these checks are here rather than in the suite is that the code cannot answer
> them. A cell you did not observe is `n/a` with a reason, or it stays blank.
>
> The procedure is [`touch-verification.md`](touch-verification.md). Read it
> first; this file records only outcomes.

---

## How to fill this in

1. Copy this file per session, or edit it in place and commit once per platform
   row — either is fine as long as the SHA in the header is the build you ran.
2. Fill in one **Session** block per matrix row, at each of its two scales.
3. In the check tables, put one of:
   - `pass` — observed, behaved as the procedure says;
   - `FAIL` — observed, did not. Add a numbered note below the table with what
     you saw **and the trace excerpt around it**; the excerpt is the part a
     maintainer cannot reconstruct;
   - `n/a` — inapplicable, with the reason (no such hardware, the device reports
     no such axis, the platform has no such feature);
   - blank — not run. Leave it blank rather than guessing.
4. Sign the **Session verdict** only when every row in that session is one of
   those four, and say plainly whether the session passed.
5. When every session block is complete, fill in the **Programme sign-off** at
   the bottom. That block is what makes the programme signed off.

A `FAIL` does not block the sign-off by itself — a recorded, understood failure
with an owner is a better outcome than a blank. An *unexplained* failure does.

---

## Build under test

| | |
|---|---|
| Release SHA (`git rev-parse HEAD`) | |
| Branch / tag | |
| Build profile (`debug` / `release`) | |
| Date of first session | |
| Date of last session | |
| CI suite green at this SHA (`cargo test --workspace`) | |
| CI X11 job (the `#[ignore]`d winit-loop test) has run — see N7 | |

---

## Session blocks

One per matrix row. Duplicate a block if you run a row on more than one machine.

### Session 1 — Windows 2-in-1, touchscreen and stylus

| | |
|---|---|
| Tester | |
| Date | |
| OS and version | |
| Machine make / model | |
| Touch digitizer | |
| Stylus make / model | |
| Display scale(s) tested | |
| Build profile | |
| Trace level used | |

| Group | Checks | Result per check | Notes |
|---|---|---|---|
| A — samples and identity | A1 A2 A3 A4 A5 A6 A7 A8 | | |
| B — hover, primacy, cursor | B1 B2 B3 B4 B5 B6 | | |
| C — arbitration | C1 C2 C3 C4 C5 C6 C7 C8 C9 | | |
| D — density and targets | D1 D2 D3 D4 D5 | | |
| E — kinetic feel | E1 E2 E3 E4 E5 E6 | | |
| F — touch text | F1 F2 F3 F4 F5 F6 F7 F8 F9 | | |
| G — pen and stylus | G1 G2 G3 G4 G5 G6 G7 G8 G9 | | |
| H — trackpad gestures | H1 H2 H3 H4 H5 | | |
| I — overlays and dismissal | I1 I2 I3 I4 I5 I6 | | |
| J — on-screen keyboard | J1 J2 J3 J4 | | |
| K — OS drag and drop | K1 K2 K3 K4 K5 K6 K7 | | |
| L — cancellation | L1 L2 L3 L4 L5 | | |
| M — with a screen reader | M1 M2 M3 M4 | | |
| N — reviewed call sites | N1 N2 N3 N4 N5 N6 N7 | | |
| O — HiDPI | O1 O2 O3 | | |

Failure notes (numbered, each with its trace excerpt):

Session verdict:

### Session 2 — Linux / Wayland, graphics tablet with a stylus

| | |
|---|---|
| Tester | |
| Date | |
| Distribution and kernel | |
| Compositor and version | |
| `zwp_tablet_manager_v2` advertised | |
| Tablet make / model | |
| Stylus make / model | |
| Scale(s) tested (incl. fractional) | |
| Build profile | |
| Trace level used | |

| Group | Checks | Result per check | Notes |
|---|---|---|---|
| A — samples and identity | A1 A2 A3 A4 A5 A6 A7 A8 | | |
| B — hover, primacy, cursor | B1 B2 B3 B4 B5 B6 | | |
| C — arbitration | C1 C2 C3 C4 C5 C6 C7 C8 C9 | | |
| D — density and targets | D1 D2 D3 D4 D5 | | |
| E — kinetic feel | E1 E2 E3 E4 E5 E6 | | |
| F — touch text | F1 F2 F3 F4 F5 F6 F7 F8 F9 | | |
| G — pen and stylus | G1 G2 G3 G4 G5 G6 G7 G8 G9 | | |
| H — trackpad gestures | H1 H2 H3 H4 H5 | | |
| I — overlays and dismissal | I1 I2 I3 I4 I5 I6 | | |
| J — on-screen keyboard | J1 J2 J3 J4 | | |
| K — OS drag and drop | K1 K2 K3 K4 K5 K6 K7 | | |
| L — cancellation | L1 L2 L3 L4 L5 | | |
| M — with a screen reader | M1 M2 M3 M4 | | |
| N — reviewed call sites | N1 N2 N3 N4 N5 N6 N7 | | |
| O — HiDPI | O1 O2 O3 | | |

Failure notes:

Session verdict:

### Session 3 — Linux / Wayland, touchscreen

| | |
|---|---|
| Tester | |
| Date | |
| Distribution and kernel | |
| Compositor and version | |
| Touch digitizer | |
| Scale(s) tested | |
| Build profile | |
| Trace level used | |

| Group | Checks | Result per check | Notes |
|---|---|---|---|
| A — samples and identity | A1 A2 A3 A4 A5 A6 A7 A8 | | |
| B — hover, primacy, cursor | B1 B2 B3 B4 B5 B6 | | |
| C — arbitration | C1 C2 C3 C4 C5 C6 C7 C8 C9 | | |
| D — density and targets | D1 D2 D3 D4 D5 | | |
| E — kinetic feel | E1 E2 E3 E4 E5 E6 | | |
| F — touch text | F1 F2 F3 F4 F5 F6 F7 F8 F9 | | |
| G — pen and stylus | G1 G2 G3 G4 G5 G6 G7 G8 G9 | | |
| H — trackpad gestures | H1 H2 H3 H4 H5 | | |
| I — overlays and dismissal | I1 I2 I3 I4 I5 I6 | | |
| J — on-screen keyboard | J1 J2 J3 J4 | | |
| K — OS drag and drop | K1 K2 K3 K4 K5 K6 K7 | | |
| L — cancellation | L1 L2 L3 L4 L5 | | |
| M — with a screen reader | M1 M2 M3 M4 | | |
| N — reviewed call sites | N1 N2 N3 N4 N5 N6 N7 | | |
| O — HiDPI | O1 O2 O3 | | |

Failure notes:

Session verdict:

### Session 4 — Linux / X11, touchscreen

| | |
|---|---|
| Tester | |
| Date | |
| Distribution and kernel | |
| Window manager and version | |
| `_NET_WM_MOVERESIZE` supported | |
| Touch digitizer | |
| Scale(s) tested | |
| Build profile | |
| Trace level used | |

| Group | Checks | Result per check | Notes |
|---|---|---|---|
| A — samples and identity | A1 A2 A3 A4 A5 A6 A7 A8 | | |
| B — hover, primacy, cursor | B1 B2 B3 B4 B5 B6 | | |
| C — arbitration | C1 C2 C3 C4 C5 C6 C7 C8 C9 | | |
| D — density and targets | D1 D2 D3 D4 D5 | | |
| E — kinetic feel | E1 E2 E3 E4 E5 E6 | | |
| F — touch text | F1 F2 F3 F4 F5 F6 F7 F8 F9 | | |
| G — pen and stylus | G1 G2 G3 G4 G5 G6 G7 G8 G9 | | |
| H — trackpad gestures | H1 H2 H3 H4 H5 | | |
| I — overlays and dismissal | I1 I2 I3 I4 I5 I6 | | |
| J — on-screen keyboard | J1 J2 J3 J4 | | |
| K — OS drag and drop | K1 K2 K3 K4 K5 K6 K7 | | |
| L — cancellation | L1 L2 L3 L4 L5 | | |
| M — with a screen reader | M1 M2 M3 M4 | | |
| N — reviewed call sites | N1 N2 N3 N4 N5 N6 N7 | | |
| O — HiDPI | O1 O2 O3 | | |

Failure notes:

Session verdict:

### Session 5 — macOS, trackpad

| | |
|---|---|
| Tester | |
| Date | |
| macOS version | |
| Machine make / model | |
| Trackpad (built-in / Magic Trackpad) | |
| Display (Retina scale) | |
| Build profile | |
| Trace level used | |

| Group | Checks | Result per check | Notes |
|---|---|---|---|
| A — samples and identity | A1 A2 A3 A4 A5 A6 A7 A8 | | |
| B — hover, primacy, cursor | B1 B2 B3 B4 B5 B6 | | |
| C — arbitration | C1 C2 C3 C4 C5 C6 C7 C8 C9 | | |
| D — density and targets | D1 D2 D3 D4 D5 | | |
| E — kinetic feel | E1 E2 E3 E4 E5 E6 | | |
| F — touch text | F1 F2 F3 F4 F5 F6 F7 F8 F9 | | |
| G — pen and stylus | G1 G2 G3 G4 G5 G6 G7 G8 G9 | | |
| H — trackpad gestures | H1 H2 H3 **H4** H5 | | |
| I — overlays and dismissal | I1 I2 I3 I4 I5 I6 | | |
| J — on-screen keyboard | J1 J2 J3 J4 | | |
| K — OS drag and drop | K1 K2 K3 K4 K5 K6 K7 | | |
| L — cancellation | L1 L2 L3 L4 L5 | | |
| M — with a screen reader | M1 M2 M3 M4 | | |
| N — reviewed call sites | N1 N2 N3 N4 N5 N6 N7 | | |
| O — HiDPI | O1 O2 O3 | | |

Failure notes:

Session verdict:

---

## The one open question — H4, the trackpad rotation sign

Called out separately because it is the only check on the whole matrix whose
answer the source tree **cannot** supply, and because either answer closes it.
Read [`touch-verification.md`](touch-verification.md) §12 H4 before filling this
in.

| | |
|---|---|
| Tester | |
| Date | |
| Machine and macOS version | |
| Twisting two fingers **clockwise** turns the content | (clockwise / counter-clockwise) |
| Verdict | (correct / inverted) |
| If inverted: fix applied at the seam, with the pinning test | |

Until this table is filled in, [Touch & pen](touch-and-pen.md) §10 continues to
carry the trackpad rotation sign as an open finding.

---

## Programme sign-off

Fill this in only when every session block above is complete.

| | |
|---|---|
| Release SHA | |
| Every matrix row covered (1–5, each at both scales) | |
| Failures recorded, each with an owner | |
| Unexplained failures remaining | |
| H4 answered | |
| Signed by | |
| Date | |

**Statement.** Written out rather than ticked, so that it cannot be signed by
accident:

> At the SHA above, the touch, pen and trackpad behaviour described in
> `docs/touch-verification.md` was executed on every row of the device matrix.
> Every check is recorded as pass, a documented failure with an owner, or `n/a`
> with a reason. No check was inferred, and no cell was filled in from another
> platform's result.

Signature:
