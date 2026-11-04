import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:ai_plan_usage/color.dart';
import 'package:ai_plan_usage/icon.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

Future<(int, int)> countWhiteRed(ui.Image image) async {
  final px = (await image.toByteData(format: ui.ImageByteFormat.rawRgba))!.buffer.asUint8List();
  var white = 0, redPx = 0;
  for (var i = 0; i < px.length; i += 4) {
    if (px[i + 3] != 255) continue; // anti-aliased edges
    final (r, g, b) = (px[i], px[i + 1], px[i + 2]);
    if (r > 230 && g > 230 && b > 230) white++;
    if (r > 170 && g < 80 && b < 80) redPx++;
  }
  return (white, redPx);
}

IconPayload payload(int sessionColor) =>
    IconPayload(sessionPct: 88, sessionColor: sessionColor, weeklyPct: 70, weeklyColor: green);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    final f = File(r'C:\Windows\Fonts\segoeuib.ttf');
    if (!f.existsSync()) return;
    await (FontLoader('Segoe UI')..addFont(Future.value(ByteData.sublistView(f.readAsBytesSync())))).load();
  });

  test('ring+number with a red session draws white digits on a red disc', () async {
    final (white, redPx) = await countWhiteRed(renderIconImage('ring+number', payload(red)));
    expect(redPx, greaterThan(200));
    expect(white, greaterThan(50));
  });

  test('ring+number with a green session paints no white', () async {
    final (white, _) = await countWhiteRed(renderIconImage('ring+number', payload(green)));
    expect(white, 0);
  });

  test('critical color wins in solid', () {
    expect(pickCritical(green, amber), amber);
    expect(pickCritical(red, amber), red);
    expect(pickCritical(gray, green), green);
  });

  test('ICO wraps the PNG with a one-entry directory', () async {
    final png = await renderIconPng('solid', payload(green));
    final ico = pngToIco(png, 64);
    final h = ByteData.sublistView(ico);
    expect(ico.sublist(0, 6), [0, 0, 1, 0, 1, 0]);
    expect(h.getUint8(6), 64);
    expect(h.getUint32(14, Endian.little), png.length);
    expect(h.getUint32(18, Endian.little), 22);
    expect(ico.sublist(22, 30), png.sublist(0, 8));
  });
}
