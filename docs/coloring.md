# Indicator coloring

The tray icon number, the popover ring, and the popover number are colored based on usage pace, not raw percentage. The rule answers one question: is current usage on track to comfortably reach the next reset, or burning faster than expected?

## What each color means

| Color | Meaning |
|-------|---------|
| Green (`#22a06b`) | Usage tracks pace, or is below pace. No action needed. |
| Amber (`#d97706`) | Usage is ahead of pace by a moderate margin. Slow down to avoid hitting the cap. |
| Red (`#dc2626`)   | Usage is ahead of pace by a large margin, or near depletion with little time left. |
| Gray (`#888888`)  | No data yet, or provider returned no value. |

## How the rule works

At each refresh, for each window (session + weekly), the app computes:

- `pace` = how far into the current window time has progressed, as a percentage.
- `delta` = `used_pct - pace`. Positive means burning faster than pace.

The amber and red thresholds for `delta` are not fixed. They taper linearly with `pace`: at the start of a window, the thresholds are wide (more rope to spike early without flagging). At the end of a window, the thresholds are tight (every extra point near reset counts).

Formula:

```
threshold(pace, start, end) = start + (end - start) * (pace / 100)

amber_thr = threshold(pace, A_start, A_end)
red_thr   = threshold(pace, R_start, R_end)

color = red    if delta >= red_thr
        amber  if delta >= amber_thr
        green  otherwise
```

## Parameters

Two profiles. Session uses tighter tapering (a 5h window allows little recovery) and an absolute red floor at 95% (any session reading at or above 95% is always red regardless of pace). Weekly uses very loose start and very tight end (high volatility early, signal becomes reliable late) and no floor.

| Window     | A_start | A_end | R_start | R_end | Red floor |
|------------|---------|-------|---------|-------|-----------|
| Session 5h | 25      | 3     | 30      | 5     | 95%       |
| Weekly 7d  | 20      | 2     | 28      | 3     | none      |

## Examples

Session 5h:

- Hour 1, used 25%: pace 20, delta +5. Green.
- Hour 1, used 50%: delta +30 >= red_thr 25. Red.
- Hour 4, used 90%: pace 80, delta +10, red_thr 10. Red.
- Hour 4.5, used 80%: pace 90, delta -10. Green. High absolute usage near reset is normal.
- Hour 4.5, used 96%: delta +6 >= amber_thr 5.2. Amber.
- Hour 4.5, used 98%: delta +8 >= red_thr 7.5. Red.

Weekly 7d:

- Day 1, used 30%: pace 14, delta +16, amber_thr 17.48. Green.
- Day 1, used 45%: delta +31 >= red_thr 24.5. Red.
- Day 4, used 70%: pace 57, delta +13, amber_thr 9.74, red_thr 13.75. Amber.
- Day 6, used 92%: pace 86, delta +6, amber_thr 4.53, red_thr 6.08. Amber.
- Day 6, used 96%: delta +10 >= red_thr 6.08. Red.

## Where each color goes

- Tray icon ring (default `ring+number` style) reflects the weekly window color.
- Tray icon number reflects the session window color.
  - When the session color is red, the number renders as white digits on a filled red disc. A bare red number is too low-luminance to read on a dark taskbar; the filled disc keeps it legible on dark and light. Green and amber render as plain colored digits (unchanged).
- Popover rings: each row shows session + weekly with the same per-window colors.
- Solid tray icon style: shows the more severe of session and weekly.

## Provider window lengths

Hardcoded constants:

- Claude: session = 5h (Claude `five_hour` window), weekly = 7 days (Claude `seven_day` window).
- Codex: session = 5h, weekly = 7 days. Assumed identical to Claude; the Codex API does not expose window length.
- Copilot: monthly entitlement. Coloring stays on the existing overage signal (red when overage > 0, neutral otherwise). The pace logic above does not apply.

## Fallback

If `resets_at` is missing for a window, the indicator falls back to the previous static rule: `used >= 95% -> red`, `used >= 80% -> amber`, else green.

## Tuning

Parameters live in code, in three places that must be kept in sync (Rust and TypeScript). To adjust:

- `apps/tauri/src-tauri/src/color.rs` -> `PARAMS_SESSION`, `PARAMS_WEEKLY`.
- `apps/ui/src/popover.ts` -> `PARAMS_SESSION`, `PARAMS_WEEKLY`.
- `apps/electron/src/main.ts` -> `PARAMS_SESSION`, `PARAMS_WEEKLY`.

Window total lengths are also constants in the same files (`SESSION_5H_*`, `WEEKLY_7D_*`).

## Design reference

For derivation, walkthrough tables, and rejected alternatives, see `docs/superpowers/specs/2026-05-28-dynamic-color-schedule-design.md`.
