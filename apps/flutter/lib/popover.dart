// Popover: the runner's own window, frameless and hidden until the tray icon
// is clicked, anchored above the tray and sized to its content (popover.rs,
// popover.ts, popover.css).
import 'dart:async';
import 'dart:ffi' hide Size;
import 'dart:io';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:intl/intl.dart';
import 'package:screen_retriever/screen_retriever.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'color.dart';
import 'model.dart';

const providerNames = {Provider.claude: 'Claude', Provider.codex: 'Codex', Provider.copilot: 'Copilot'};
const logos = {
  Provider.claude: 'assets/claude-symbol.svg',
  Provider.codex: 'assets/openai-symbol.svg',
  Provider.copilot: 'assets/copilot-symbol.svg',
};

final _user32 = DynamicLibrary.open('user32.dll');
final _foregroundWindow = _user32.lookupFunction<IntPtr Function(), int Function()>('GetForegroundWindow');
final _windowProcessId =
    _user32.lookupFunction<Uint32 Function(IntPtr, Pointer<Uint32>), int Function(int, Pointer<Uint32>)>('GetWindowThreadProcessId');
final _keybdEvent = _user32.lookupFunction<Void Function(Uint8, Uint8, Uint32, UintPtr), void Function(int, int, int, int)>('keybd_event');

/// Windows refuses the foreground to a background process unless input just
/// arrived; an Alt press and release counts. tao does this in Tauri's
/// set_focus, which is why a second launch focuses its popover.
void _allowForeground() {
  const vkMenu = 0x12, extended = 0x1, keyUp = 0x2;
  _keybdEvent(vkMenu, 0, extended, 0);
  _keybdEvent(vkMenu, 0, extended | keyUp, 0);
}

/// True when the focused window belongs to this process, as the settings window does.
bool foregroundIsOurs() {
  final hwnd = _foregroundWindow();
  if (hwnd == 0) return false;
  return using((arena) {
    final owner = arena<Uint32>();
    _windowProcessId(hwnd, owner);
    return owner.value == pid;
  });
}

class PopoverWindow with WindowListener {
  Size _size = const Size(280, 180);

  Future<void> init() async {
    await windowManager.ensureInitialized();
    // Also creates the taskbar interface setSkipTaskbar needs (null before: crash).
    await windowManager.waitUntilReadyToShow();
    await windowManager.setAsFrameless();
    // Win11 then draws the 1 px border, rounded corners and shadow Tauri's window has.
    await windowManager.setHasShadow(true);
    await windowManager.setSkipTaskbar(true);
    await windowManager.setAlwaysOnTop(true);
    await windowManager.setResizable(false);
    await windowManager.setPreventClose(true);
    windowManager.addListener(this);
  }

  Future<void> toggle() async {
    if (await windowManager.isVisible()) {
      await windowManager.hide();
      return;
    }
    await _position();
    if (!foregroundIsOurs()) _allowForeground();
    await windowManager.show();
    await windowManager.focus();
  }

  Future<void> hide() => windowManager.hide();

  Future<void> _position() async {
    final tray = await trayManager.getBounds();
    if (tray == null) return;
    // Tray icons only live on the primary display, which sits at the origin.
    final mon = Offset.zero & (await screenRetriever.getPrimaryDisplay()).size;
    final x = math.min(math.max(tray.center.dx - _size.width / 2, mon.left + 8), mon.right - _size.width - 8);
    final taskbarOnBottom = tray.top > mon.top + mon.height / 2;
    // Tauri's 8 px gap sits outside an 8 px invisible resize border; this
    // window has none, so the border is added to keep the same visible gap.
    final gap = 8 + 8 / windowManager.getDevicePixelRatio();
    final y = taskbarOnBottom ? tray.top - _size.height - gap : tray.bottom + gap;
    await windowManager.setPosition(Offset(x, y));
  }

  Future<void> resize(Size content) async {
    final s = Size(content.width.ceilToDouble().clamp(180, 640), content.height.ceilToDouble().clamp(60, 600));
    if (s == _size) return;
    _size = s;
    // window_manager truncates to physical pixels; round up instead, as WebView2 does.
    final dpr = windowManager.getDevicePixelRatio();
    double up(double v) => (v * dpr).ceil() / dpr + 0.001;
    await windowManager.setSize(Size(up(s.width), up(s.height)));
    if (await windowManager.isVisible()) await _position();
  }

  Future<bool> _cursorOverTray() async {
    final tray = await trayManager.getBounds();
    if (tray == null) return false;
    return tray.inflate(4).contains(await screenRetriever.getCursorScreenPoint());
  }

  @override
  void onWindowBlur() async {
    // A click on the tray icon: its click handler owns visibility.
    if (await _cursorOverTray()) return;
    if (foregroundIsOurs()) return;
    await windowManager.hide();
  }

  @override
  void onWindowClose() => unawaited(windowManager.hide());
}

// ---- Widgets ----

const _ink = Color(0xff222222);
// Windows ships no Segoe UI 500: WebView2 draws CSS 500 with Semibold, as 600.
const _w500 = FontWeight.w600;
const _w600 = FontWeight.w600;

TextStyle seg(double size, {FontWeight weight = FontWeight.w400, Color color = _ink, double? height, double spacing = 0, bool tabular = false}) =>
    TextStyle(
      fontFamily: 'Segoe UI',
      fontSize: size,
      fontWeight: weight,
      color: color,
      height: height,
      letterSpacing: spacing,
      leadingDistribution: TextLeadingDistribution.even,
      fontFeatures: tabular ? const [FontFeature.tabularFigures()] : null,
    );

/// One text line in the box WebView2 gives it: Segoe UI's ascent and descent
/// (2210 and 514 of 2048) each rounded to device pixels. Flutter's own line
/// boxes are taller, and the popover rows would not add up to the same height.
Widget cssLine(BuildContext context, double size, Widget text) {
  final dpr = MediaQuery.devicePixelRatioOf(context);
  final h = ((size * dpr * 2210 / 2048).round() + (size * dpr * 514 / 2048).round()) / dpr;
  return SizedBox(height: h, child: OverflowBox(maxHeight: double.infinity, fit: OverflowBoxFit.deferToChild, child: text));
}

String accountName(Account a) => '${providerNames[a.provider]} · ${a.label}';

String? fmtStamp(String? iso) {
  if (iso == null) return null;
  final d = DateTime.tryParse(iso);
  if (d == null) return iso;
  final local = d.toLocal();
  return '${DateFormat.MMMd().format(local)}, ${DateFormat.jm().format(local)}';
}

int _msUntil(String iso) => (DateTime.tryParse(iso)?.millisecondsSinceEpoch ?? 0) - DateTime.now().millisecondsSinceEpoch;

class PopoverApp extends StatelessWidget {
  const PopoverApp({super.key, required this.model, required this.window, required this.onOpenSettings});

  final AppModel model;
  final PopoverWindow window;
  final VoidCallback onOpenSettings;

  @override
  Widget build(BuildContext context) => MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: ThemeData(
          fontFamily: 'Segoe UI',
          tooltipTheme: TooltipThemeData(
            waitDuration: const Duration(milliseconds: 500),
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
            textStyle: seg(11),
            decoration: BoxDecoration(
              color: const Color(0xfff9f9f9),
              border: Border.all(color: const Color(0xffd0d0d0)),
              borderRadius: BorderRadius.circular(3),
            ),
          ),
        ),
        home: Material(
          color: const Color(0xfffafafa),
          child: PopoverView(model: model, onSize: window.resize, onOpenSettings: onOpenSettings),
        ),
      );
}

class PopoverView extends StatefulWidget {
  const PopoverView({super.key, required this.model, required this.onSize, required this.onOpenSettings});

  final AppModel model;
  final ValueChanged<Size> onSize;
  final VoidCallback onOpenSettings;

  @override
  State<PopoverView> createState() => _PopoverViewState();
}

typedef _ErrorView = ({String id, String label, SnapshotError error});

class _PopoverViewState extends State<PopoverView> {
  bool _panelOpen = false;
  String? _selected;
  late final Timer _tick;

  @override
  void initState() {
    super.initState();
    // Countdowns and the stale chip move every second.
    _tick = Timer.periodic(const Duration(seconds: 1), (_) => setState(() {}));
  }

  @override
  void dispose() {
    _tick.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(listenable: widget.model, builder: (context, _) => _build());

  Widget _build() {
    final s = widget.model.state;
    final shown = s.accounts.where((a) => a.enabled).toList();
    final List<_ErrorView> errors = [
      for (final a in shown)
        if (s.snapshots[a.id]?.error case final e?) (id: a.id, label: accountName(a), error: e),
    ];
    if (errors.isEmpty) {
      _panelOpen = false;
      _selected = null;
    }
    final open = _panelOpen && errors.isNotEmpty;
    if (open && !errors.any((e) => e.id == _selected)) _selected = errors.first.id;

    final rows = <Widget>[
      if (shown.isEmpty)
        ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 220),
          child: Text(
            s.accounts.isEmpty
                ? 'No accounts yet. Right-click the tray icon, then Settings, to add one.'
                : 'All accounts are turned off. Turn one on in Settings.',
            style: seg(11, color: const Color(0xff666666), height: 1.35),
          ),
        ),
      for (final a in shown) _row(a, s.snapshots[a.id]),
    ];

    final content = Padding(
      padding: const EdgeInsets.fromLTRB(22, 14, 22, 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ConstrainedBox(
            // The window is clamped at 600 px; more rows scroll.
            constraints: const BoxConstraints(maxHeight: 560),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [for (final (i, r) in rows.indexed) Padding(padding: EdgeInsets.only(top: i == 0 ? 0 : 4), child: r)],
              ),
            ),
          ),
          if (open) _panel(errors),
        ],
      ),
    );

    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): () {
          if (_panelOpen) setState(() => _panelOpen = false);
        },
      },
      child: Focus(
        autofocus: true,
        child: OverflowBox(
          alignment: Alignment.topLeft,
          minWidth: 0,
          maxWidth: double.infinity,
          minHeight: 0,
          maxHeight: double.infinity,
          child: SizeReporter(
            onSize: widget.onSize,
            child: ConstrainedBox(
              constraints: const BoxConstraints(minWidth: 180),
              child: Stack(children: [content, Positioned(top: 2, right: 4, child: _chip(s, errors))]),
            ),
          ),
        ),
      ),
    );
  }

  Widget _chip(AppState s, List<_ErrorView> errors) {
    final ageMs = DateTime.now().millisecondsSinceEpoch - (DateTime.tryParse(s.fetchedAt)?.millisecondsSinceEpoch ?? 0);
    if (errors.isNotEmpty) {
      return Tooltip(
        message: errors.first.error.message,
        child: Pressable(
          onPressed: () => setState(() {
            _panelOpen = !_panelOpen;
            if (_panelOpen) _selected ??= errors.first.id;
          }),
          outline: const Color(0xff991b1b),
          child: _chipBox(errors.length > 1 ? '${errors.length} errors' : 'error', const Color(0xfffee2e2), const Color(0xff991b1b)),
        ),
      );
    }
    if (ageMs > 15 * 60000) return _chipBox('stale ${fmtDuration(ageMs)}', const Color(0xfffed7aa), const Color(0xff7c2d12));
    return const SizedBox.shrink();
  }

  Widget _chipBox(String text, Color bg, Color fg) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
        decoration: BoxDecoration(color: bg, borderRadius: BorderRadius.circular(3)),
        child: cssLine(context, 9, Text(text, style: seg(9, weight: _w600, color: fg))),
      );

  Widget _row(Account a, ProviderSnapshot? snap) {
    final error = snap?.error;
    final slots = a.provider == Provider.copilot
        ? [_ringSlot('MONTH', snap?.weekly, error != null, weekly: true), _overageSlot(snap?.weekly ?? const UsageBlock(), error != null)]
        : [_ringSlot('SESSION', snap?.session, error != null), _ringSlot('WEEK', snap?.weekly, error != null, weekly: true)];
    final row = Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Tooltip(
            message: accountName(a),
            child: SizedBox(
              width: 56,
              child: Column(
                children: [
                  SvgPicture.asset(logos[a.provider]!, width: 32, height: 32),
                  const SizedBox(height: 2),
                  cssLine(
                    context,
                    9,
                    Text(a.label,
                        maxLines: 1,
                        softWrap: false,
                        overflow: TextOverflow.ellipsis,
                        style: seg(9, weight: _w600, color: const Color(0xff555555))),
                  ),
                ],
              ),
            ),
          ),
          for (final slot in slots) ...[const SizedBox(width: 12), slot],
        ],
      ),
    );
    return error == null ? row : Tooltip(message: error.message, child: row);
  }

  Widget _slot(String label, Widget main, String reset) => ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 50),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            cssLine(context, 8, Text(label, style: seg(8, weight: _w600, color: const Color(0xff888888), spacing: 0.5))),
            const SizedBox(height: 1),
            main,
            const SizedBox(height: 1),
            if (reset.isNotEmpty) cssLine(context, 9, Text(reset, style: seg(9, weight: _w500, color: const Color(0xff555555), tabular: true))),
          ],
        ),
      );

  Widget _ringSlot(String label, UsageBlock? block, bool errored, {bool weekly = false}) {
    final pct = block?.usedPct;
    if (errored || pct == null) {
      return _slot(label, Ring(fraction: 0, color: const Color(0xffbbbbbb), text: errored ? '!' : '--', textColor: const Color(0xff888888)), '');
    }
    final clamped = pct.clamp(0, 100).toDouble();
    final resets = block!.resetsAt;
    final color = Color(colorForWindow(clamped, parseIsoSecs(resets), nowSecs(), weekly ? weekly7dSecs : session5hSecs,
        weekly ? paramsWeekly : paramsSession));
    return _slot(label, Ring(fraction: clamped / 100, color: color, text: '${clamped.round()}', textColor: color),
        resets == null ? '' : fmtDuration(_msUntil(resets)));
  }

  Widget _overageSlot(UsageBlock block, bool errored) {
    final known = !errored && block.usedAbs != null && block.entitlement != null;
    final over = (block.overage ?? 0) > 0;
    final used = errored ? '!' : (known ? '${block.usedAbs}' : '--');
    final of = known ? 'of ${block.entitlement}' : '';
    // Without data the browser keeps the stylesheet's red on the big number.
    final usedColor = known && !over ? _ink : const Color(0xffdc2626);
    final reset = known && block.resetsAt != null ? fmtDuration(_msUntil(block.resetsAt!)) : '';
    return _slot(
      'OVERAGE',
      SizedBox(
        width: 56,
        height: 36,
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(used, style: seg(14, weight: FontWeight.w700, color: usedColor, height: 1.05, tabular: true)),
            const SizedBox(height: 2),
            if (of.isNotEmpty)
              Text(of, style: seg(9, weight: _w500, color: over ? const Color(0xffdc2626) : const Color(0xff888888), height: 1.05, tabular: true)),
          ],
        ),
      ),
      reset,
    );
  }

  Widget _panel(List<_ErrorView> errors) {
    final ordered = [...errors]..sort((a, b) => a.id == _selected ? -1 : (b.id == _selected ? 1 : 0));
    return Container(
      width: 260,
      margin: const EdgeInsets.only(top: 10),
      padding: const EdgeInsets.only(top: 9),
      decoration: const BoxDecoration(border: Border(top: BorderSide(color: Color(0xffe5e5e5)))),
      child: SelectionArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [for (final (i, e) in ordered.indexed) _errorItem(e, first: i == 0)],
        ),
      ),
    );
  }

  Widget _errorItem(_ErrorView e, {required bool first}) {
    final meta = [fmtStamp(e.error.occurredAt), if (e.error.retryAt != null) 'Retry at ${fmtStamp(e.error.retryAt)}'].nonNulls.join(' · ');
    return Container(
      margin: EdgeInsets.only(top: first ? 0 : 9),
      padding: EdgeInsets.only(top: first ? 0 : 9),
      decoration: first ? null : const BoxDecoration(border: Border(top: BorderSide(color: Color(0xffeeeeee)))),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              Expanded(child: cssLine(context, 11, Text(e.label, style: seg(11, weight: FontWeight.w700)))),
              const SizedBox(width: 8),
              cssLine(context, 9, Text(e.error.kind, style: seg(9, weight: _w600, color: const Color(0xff991b1b)))),
            ],
          ),
          const SizedBox(height: 4),
          Text(e.error.message, style: seg(11, height: 1.3)),
          if (e.error.detail case final detail?) ...[
            const SizedBox(height: 5),
            Text(detail,
                style: const TextStyle(
                    fontFamily: 'Consolas', fontSize: 10, height: 1.3, color: Color(0xff444444), leadingDistribution: TextLeadingDistribution.even)),
          ],
          if (meta.isNotEmpty) ...[const SizedBox(height: 4), cssLine(context, 9, Text(meta, style: seg(9, color: const Color(0xff666666))))],
          if (e.error.kind == 'signed-out') ...[
            const SizedBox(height: 6),
            Pressable(
              onPressed: widget.onOpenSettings,
              outline: const Color(0xff991b1b),
              builder: (hover) => Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: hover ? const Color(0xfff0f0f0) : Colors.white,
                  border: Border.all(color: const Color(0xffe5e5e5)),
                  borderRadius: BorderRadius.circular(3),
                ),
                child: cssLine(context, 10, Text('Open Settings', style: seg(10, weight: _w600))),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// One usage ring: 36 px, stroke 12 of 100, from 12 o'clock, 300 ms transition.
class Ring extends StatelessWidget {
  const Ring({super.key, required this.fraction, required this.color, required this.text, required this.textColor});

  final double fraction;
  final Color color;
  final String text;
  final Color textColor;

  @override
  Widget build(BuildContext context) => SizedBox(
        width: 36,
        height: 36,
        child: Stack(
          alignment: Alignment.center,
          children: [
            TweenAnimationBuilder<double>(
              tween: Tween(end: fraction),
              duration: const Duration(milliseconds: 300),
              curve: Curves.ease,
              builder: (_, f, _) => TweenAnimationBuilder<Color?>(
                tween: ColorTween(end: color),
                duration: const Duration(milliseconds: 300),
                curve: Curves.ease,
                builder: (_, c, _) => CustomPaint(size: const Size(36, 36), painter: _RingPainter(f, c ?? color)),
              ),
            ),
            Text(text, style: seg(11, weight: FontWeight.w700, color: textColor, tabular: true)),
          ],
        ),
      );
}

class _RingPainter extends CustomPainter {
  _RingPainter(this.fraction, this.color);

  final double fraction;
  final Color color;

  static const _r = 42 * 0.36;
  static const _w = 12 * 0.36;

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    canvas.drawCircle(c, _r, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = _w
      ..color = const Color(0xffe5e5e5));
    final fg = Paint()..color = color;
    if (fraction <= 0) {
      // A zero-length dash with round caps: the browser draws a dot.
      canvas.drawCircle(c.translate(0, -_r), _w / 2, fg);
      return;
    }
    canvas.drawArc(Rect.fromCircle(center: c, radius: _r), -math.pi / 2, math.min(fraction, 1) * 2 * math.pi, false,
        fg
          ..style = PaintingStyle.stroke
          ..strokeWidth = _w
          ..strokeCap = StrokeCap.round);
  }

  @override
  bool shouldRepaint(_RingPainter old) => old.fraction != fraction || old.color != color;
}

/// A clickable box: pointer cursor, hover state, Enter/Space, focus outline
/// for keyboard users only.
class Pressable extends StatefulWidget {
  const Pressable({super.key, required this.onPressed, this.child, this.builder, this.outline, this.radius = 3, this.enabled = true});

  final VoidCallback onPressed;
  final Widget? child;
  final Widget Function(bool hover)? builder;
  final Color? outline;
  final double radius;
  final bool enabled;

  @override
  State<Pressable> createState() => _PressableState();
}

class _PressableState extends State<Pressable> {
  bool _hover = false;
  bool _focus = false;

  @override
  Widget build(BuildContext context) {
    final child = widget.builder?.call(_hover) ?? widget.child!;
    final keyboard = FocusManager.instance.highlightMode == FocusHighlightMode.traditional;
    return FocusableActionDetector(
      enabled: widget.enabled,
      mouseCursor: widget.enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
      onShowHoverHighlight: (v) => setState(() => _hover = v),
      onShowFocusHighlight: (v) => setState(() => _focus = v),
      actions: {ActivateIntent: CallbackAction<ActivateIntent>(onInvoke: (_) => widget.onPressed())},
      child: GestureDetector(
        onTap: widget.enabled ? widget.onPressed : null,
        child: DecoratedBox(
          position: DecorationPosition.foreground,
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(widget.radius + 2),
            border: _focus && keyboard && widget.outline != null
                ? Border.all(color: widget.outline!, width: 2, strokeAlign: BorderSide.strokeAlignOutside + 1)
                : null,
          ),
          child: child,
        ),
      ),
    );
  }
}

/// Reports its child's laid-out size after each change.
class SizeReporter extends SingleChildRenderObjectWidget {
  const SizeReporter({super.key, required this.onSize, super.child});

  final ValueChanged<Size> onSize;

  @override
  RenderObject createRenderObject(BuildContext context) => RenderSizeReporter(onSize);

  @override
  void updateRenderObject(BuildContext context, RenderSizeReporter renderObject) => renderObject.onSize = onSize;
}

class RenderSizeReporter extends RenderProxyBox {
  RenderSizeReporter(this.onSize);

  ValueChanged<Size> onSize;
  Size? _last;

  @override
  void performLayout() {
    super.performLayout();
    if (size == _last) return;
    _last = size;
    final s = size;
    WidgetsBinding.instance.addPostFrameCallback((_) => onSize(s));
  }
}
