// Pace-aware colors for the tray icon and the popover rings, see
// docs/coloring.md. Same parameters as color.rs and popover.ts.

const green = 0xff22a06b;
const amber = 0xffd97706;
const red = 0xffdc2626;
const gray = 0xff888888;

/// Window total length in seconds.
const session5hSecs = 5 * 3600;
const weekly7dSecs = 168 * 3600;

class ColorParams {
  const ColorParams(this.aStart, this.aEnd, this.rStart, this.rEnd, [this.redFloor]);

  final double aStart;
  final double aEnd;
  final double rStart;
  final double rEnd;

  /// At or above this, red regardless of pace. Session window only.
  final double? redFloor;
}

const paramsSession = ColorParams(25, 3, 30, 5, 95);
const paramsWeekly = ColorParams(20, 2, 28, 3);

/// Linear interpolation between `start` (pace=0) and `end` (pace=100).
double threshold(double pace, double start, double end) => start + (end - start) * (pace / 100);

/// Static fallback used when `resetsAt` is missing.
int colorForPctStatic(double? usedPct) {
  if (usedPct == null) return gray;
  if (usedPct >= 95) return red;
  if (usedPct >= 80) return amber;
  return green;
}

/// `nowSecs` and `resetsAtSecs` are UNIX timestamps in seconds.
int colorForWindow(double? usedPct, int? resetsAtSecs, int nowSecs, int totalWindowSecs, ColorParams params) {
  if (usedPct == null) return gray;
  final floor = params.redFloor;
  if (floor != null && usedPct >= floor) return red;
  if (resetsAtSecs == null || totalWindowSecs <= 0) return colorForPctStatic(usedPct);

  final remaining = (resetsAtSecs - nowSecs).clamp(0, totalWindowSecs);
  final elapsed = totalWindowSecs - remaining;
  final pace = elapsed / totalWindowSecs * 100;
  final delta = usedPct - pace;

  if (delta >= threshold(pace, params.rStart, params.rEnd)) return red;
  if (delta >= threshold(pace, params.aStart, params.aEnd)) return amber;
  return green;
}

int? parseIsoSecs(String? iso) {
  final d = iso == null ? null : DateTime.tryParse(iso);
  return d == null ? null : d.millisecondsSinceEpoch ~/ 1000;
}

int nowSecs() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

/// Countdown text under a ring: `1d4h`, `4h47m`, `12m`.
String fmtDuration(int ms) {
  if (ms <= 0) return '0m';
  final d = ms ~/ 86400000;
  final h = (ms % 86400000) ~/ 3600000;
  final m = (ms % 3600000) ~/ 60000;
  if (d > 0) return '${d}d${h}h';
  if (h > 0) return '${h}h${m}m';
  return '${m}m';
}
