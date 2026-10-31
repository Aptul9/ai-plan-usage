import 'package:ai_plan_usage/color.dart';
import 'package:flutter_test/flutter_test.dart';

int run(double used, double pacePct, ColorParams params, int total) {
  const now = 1700000000;
  final elapsed = (pacePct / 100 * total).toInt();
  return colorForWindow(used, now + (total - elapsed), now, total, params);
}

void main() {
  test('threshold endpoints', () {
    expect(threshold(0, 25, 3), closeTo(25, 1e-9));
    expect(threshold(100, 25, 3), closeTo(3, 1e-9));
    expect(threshold(50, 25, 3), closeTo(14, 1e-9));
  });

  test('session walkthrough', () {
    const p = paramsSession, t = session5hSecs;
    expect(run(25, 20, p, t), green);
    expect(run(50, 20, p, t), red);
    expect(run(90, 80, p, t), red);
    expect(run(80, 90, p, t), green);
    expect(run(88, 80, p, t), amber);
    expect(run(95, 90, p, t), red);
    expect(run(98, 90, p, t), red);
  });

  test('session red floor applies early in the window', () {
    expect(run(95, 10, paramsSession, session5hSecs), red);
    expect(run(99, 10, paramsSession, session5hSecs), red);
  });

  test('weekly walkthrough, no floor', () {
    const p = paramsWeekly, t = weekly7dSecs;
    expect(run(95, 86, p, t), red);
    expect(run(30, 14, p, t), green);
    expect(run(45, 14, p, t), red);
    expect(run(70, 57, p, t), amber);
    expect(run(92, 86, p, t), amber);
    expect(run(96, 86, p, t), red);
  });

  test('missing resets falls back to static, missing pct is gray', () {
    expect(colorForWindow(96, null, 0, session5hSecs, paramsSession), red);
    expect(colorForWindow(85, null, 0, session5hSecs, paramsSession), amber);
    expect(colorForWindow(40, null, 0, session5hSecs, paramsSession), green);
    expect(colorForWindow(null, 0, 0, session5hSecs, paramsSession), gray);
  });

  test('pace clamps to 100 and to 0', () {
    const now = 1000000;
    expect(colorForWindow(80, now - 10000, now, session5hSecs, paramsSession), green);
    expect(colorForWindow(110, now - 10000, now, session5hSecs, paramsSession), red);
    expect(colorForWindow(40, now + session5hSecs * 5, now, session5hSecs, paramsSession), red);
    expect(colorForWindow(20, now + session5hSecs * 5, now, session5hSecs, paramsSession), green);
  });

  test('ISO parse', () {
    expect(parseIsoSecs('2026-01-01T00:00:00+00:00'), 1767225600);
    expect(parseIsoSecs('2026-05-29T21:30:00.176309+02:00'), 1780083000);
    expect(parseIsoSecs('not-a-date'), isNull);
    expect(parseIsoSecs(null), isNull);
  });

  test('countdown text', () {
    expect(fmtDuration(0), '0m');
    expect(fmtDuration(-5), '0m');
    expect(fmtDuration(12 * 60000 + 59000), '12m');
    expect(fmtDuration(4 * 3600000 + 47 * 60000), '4h47m');
    expect(fmtDuration(6 * 86400000 + 2 * 3600000 + 5 * 60000), '6d2h');
  });
}
