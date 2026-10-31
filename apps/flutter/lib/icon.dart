// Tray icon: 64x64, drawn per style from the primary account. tray_manager
// 0.5.3 loads Windows icons from an .ico file only, so the PNG is wrapped in
// a one-entry ICO container.
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/painting.dart';

import 'color.dart';
import 'model.dart';

const _size = 64.0;

class IconPayload {
  const IconPayload({this.sessionPct, this.sessionColor = gray, this.weeklyPct, this.weeklyColor = gray, this.error = false});

  final double? sessionPct;
  final int sessionColor;
  final double? weeklyPct;
  final int weeklyColor;
  final bool error;
}

int pickCritical(int a, int b) {
  int rank(int c) => switch (c) { red => 3, amber => 2, green => 1, _ => 0 };
  return rank(a) >= rank(b) ? a : b;
}

IconPayload payloadFromState(AppState state) {
  var sessPct = state.session.usedPct;
  final weekPct = state.weekly.usedPct;
  // Copilot has no session window: its monthly value stands in.
  sessPct ??= weekPct;
  final now = nowSecs();
  // Codex does not expose window lengths; Claude's are assumed for every provider.
  return IconPayload(
    sessionPct: sessPct,
    sessionColor: colorForWindow(sessPct, parseIsoSecs(state.session.resetsAt), now, session5hSecs, paramsSession),
    weeklyPct: weekPct,
    weeklyColor: colorForWindow(weekPct, parseIsoSecs(state.weekly.resetsAt), now, weekly7dSecs, paramsWeekly),
    error: state.error != null,
  );
}

Paint _fill(int color) => Paint()
  ..color = Color(color)
  ..isAntiAlias = true;

Paint _stroke(int color, double width, {StrokeCap cap = StrokeCap.butt}) => Paint()
  ..color = Color(color)
  ..style = PaintingStyle.stroke
  ..strokeWidth = width
  ..strokeCap = cap
  ..isAntiAlias = true;

/// Digits centered on their cap box, as icon.rs does with ab_glyph: `px` is
/// the line height (Segoe UI ascent - descent = 2724 units), the cap box is
/// 0.72 x ascent (2210 units).
void _number(Canvas c, String text, double px, int color) {
  final tp = TextPainter(
    text: TextSpan(
      text: text,
      style: TextStyle(fontFamily: 'Segoe UI', fontWeight: FontWeight.w700, fontSize: px * 2048 / 2724, color: Color(color)),
    ),
    textDirection: TextDirection.ltr,
  )..layout();
  final baseline = _size / 2 + 0.72 * px * 2210 / 2724 / 2;
  tp.paint(c, Offset(_size / 2 - tp.width / 2, baseline - tp.computeDistanceToActualBaseline(TextBaseline.alphabetic)));
}

void _arc(Canvas c, double r, double width, int color, double pct) {
  if (pct <= 0) return;
  final sweep = pct.clamp(0, 100) / 100 * 2 * math.pi;
  c.drawArc(Rect.fromCircle(center: const Offset(_size / 2, _size / 2), radius: r), -math.pi / 2, sweep, false,
      _stroke(color, width, cap: StrokeCap.round));
}

ui.Image renderIconImage(String style, IconPayload p) {
  final recorder = ui.PictureRecorder();
  final c = Canvas(recorder);
  const center = Offset(_size / 2, _size / 2);

  if (p.error) {
    c.drawCircle(center, 28, _fill(0xff666666));
    final x = _stroke(0xffffffff, 4, cap: StrokeCap.round);
    c.drawLine(const Offset(20, 20), const Offset(44, 44), x);
    c.drawLine(const Offset(44, 20), const Offset(20, 44), x);
  } else {
    final sPct = p.sessionPct ?? 0;
    final wPct = p.weeklyPct ?? 0;
    switch (style) {
      case 'solid':
        c.drawCircle(center, 28, _fill(pickCritical(p.sessionColor, p.weeklyColor)));
      case 'number':
        c.drawCircle(center, 30, _fill(p.sessionColor));
        _number(c, '${sPct.round()}', 44, 0xffffffff);
      case 'ring':
        c.drawCircle(center, 24, _stroke(0xffd4d4d4, 9));
        _arc(c, 24, 9, p.weeklyColor, wPct);
      case 'ring+number':
        c.drawCircle(center, 30, _stroke(0xffd4d4d4, 4));
        _arc(c, 30, 4, p.weeklyColor, wPct);
        // Red digits vanish on a dark taskbar: white digits on a red disc instead.
        var numColor = p.sessionColor;
        if (numColor == red) {
          c.drawCircle(center, 27, _fill(red));
          numColor = 0xffffffff;
        }
        _number(c, '${sPct.round()}', 46, numColor);
      case 'bar':
        const padX = 12.0, padY = 6.0;
        const barW = _size - padX * 2, barH = _size - padY * 2;
        final fillH = (sPct / 100 * barH).roundToDouble();
        c.drawRect(const Rect.fromLTWH(padX, padY, barW, barH), _fill(0xffe5e5e5)..isAntiAlias = false);
        c.drawRect(Rect.fromLTWH(padX, padY + (barH - fillH), barW, fillH), _fill(p.sessionColor)..isAntiAlias = false);
        c.drawRect(const Rect.fromLTWH(padX, padY, barW, barH), _stroke(0xff333333, 2));
    }
  }
  return recorder.endRecording().toImageSync(_size.toInt(), _size.toInt());
}

Future<Uint8List> renderIconPng(String style, IconPayload p) async {
  final image = renderIconImage(style, p);
  final data = await image.toByteData(format: ui.ImageByteFormat.png);
  image.dispose();
  return data!.buffer.asUint8List();
}

/// One-entry ICO with a PNG payload (Windows Vista and later read it).
Uint8List pngToIco(Uint8List png, int size) {
  final h = ByteData(22)
    ..setUint16(2, 1, Endian.little) // type: icon
    ..setUint16(4, 1, Endian.little) // one image
    ..setUint8(6, size >= 256 ? 0 : size)
    ..setUint8(7, size >= 256 ? 0 : size)
    ..setUint16(10, 1, Endian.little) // planes
    ..setUint16(12, 32, Endian.little) // bits per pixel
    ..setUint32(14, png.length, Endian.little)
    ..setUint32(18, 22, Endian.little); // payload offset
  return (BytesBuilder(copy: false)
        ..add(h.buffer.asUint8List())
        ..add(png))
      .toBytes();
}
