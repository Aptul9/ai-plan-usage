// Settings window: a second engine opened on demand. It talks to the main
// engine over one channel with the Tauri command names (tauri-shim.js);
// values travel as JSON text. The page is settings.html + settings.ts.
import 'dart:async';
import 'dart:convert';
import 'dart:ui' as ui;

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart' show OverflowBoxFit;
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'model.dart';
import 'popover.dart' show Pressable, logos, providerNames;

const settingsArgs = 'settings';
const _channel = WindowMethodChannel('aipu');

/// Main engine side: answers the commands and pushes every change.
class SettingsHost {
  SettingsHost(this.model);

  final AppModel model;

  Future<void> init() async {
    await _channel.setMethodCallHandler((call) async {
      final args = jsonDecode(call.arguments as String? ?? '{}') as Map<String, dynamic>;
      try {
        return jsonEncode(await model.invoke(call.method, args));
      } on AppError catch (e) {
        throw PlatformException(code: 'app', message: e.message);
      }
    });
    model.addListener(_push);
  }

  // No settings window: the channel has no peer and the call fails, ignored.
  void _push() {
    _channel.invokeMethod('snapshot-updated', jsonEncode(model.state.publicJson())).catchError((_) => null);
  }

  Future<void> open() async {
    for (final w in await WindowController.getAll()) {
      if (w.arguments == settingsArgs) {
        await w.invokeMethod('focus');
        return;
      }
    }
    await WindowController.create(const WindowConfiguration(arguments: settingsArgs, hiddenAtLaunch: true));
  }
}

/// Settings engine side.
class Api {
  final _updates = StreamController<void>.broadcast();

  Stream<void> get updates => _updates.stream;

  Future<void> init(WindowController window) async {
    await _channel.setMethodCallHandler((call) async {
      if (call.method == 'snapshot-updated') _updates.add(null);
      return null;
    });
    await window.setWindowMethodHandler((call) async {
      if (call.method == 'focus') {
        await windowManager.show();
        await windowManager.focus();
      }
      return null;
    });
  }

  Future<Object?> call(String method, [Map<String, Object?> args = const {}]) async {
    try {
      return jsonDecode(await _channel.invokeMethod<String>(method, jsonEncode(args)) ?? 'null');
    } on WindowChannelException catch (e) {
      throw AppError(e.message);
    }
  }
}

Future<void> runSettings(WindowController window) async {
  final api = Api();
  await api.init(window);
  await windowManager.ensureInitialized();
  await windowManager.setTitle('ai-plan-usage settings (Flutter)');
  await windowManager.setResizable(false);
  await windowManager.setMinimizable(false);
  await windowManager.setMaximizable(false);
  // Tauri sizes the inner area to 520 x 640; window_manager sizes the frame.
  await windowManager.setSize(const Size(520, 640));
  final view = WidgetsBinding.instance.platformDispatcher.views.first;
  final client = view.physicalSize / view.devicePixelRatio;
  final outer = await windowManager.getSize();
  await windowManager.setSize(Size(520 + outer.width - client.width, 640 + outer.height - client.height));
  await windowManager.center();
  hair = view.devicePixelRatio.floorToDouble().clamp(1, 8) / view.devicePixelRatio;
  runApp(MaterialApp(
    debugShowCheckedModeBanner: false,
    theme: ThemeData(
      fontFamily: 'Inter',
      textSelectionTheme: const TextSelectionThemeData(cursorColor: zinc900, selectionColor: Color(0x663b82f6)),
    ),
    home: SettingsPage(api: api),
  ));
  await windowManager.show();
  await windowManager.focus();
}

// ---- Tailwind zinc palette and type ----

const zinc900 = Color(0xff18181b);
const zinc700 = Color(0xff3f3f46);
const zinc600 = Color(0xff52525b);
const zinc500 = Color(0xff71717a);
const zinc400 = Color(0xffa1a1aa);
const zinc300 = Color(0xffd4d4d8);
const zinc200 = Color(0xffe4e4e7);
const zinc100 = Color(0xfff4f4f5);
const zinc50 = Color(0xfffafafa);
const emerald500 = Color(0xff10b981);
const emerald600 = Color(0xff059669);
const red600 = Color(0xffdc2626);
const star = Color(0xfff59e0b);
const brand = Color(0xff6366f1);

/// Inter at a CSS size, weight and line height (px). The variable font gets
/// its weight on the wght axis.
TextStyle inter(double size, {FontWeight w = FontWeight.w400, Color c = zinc900, double? lh, double ls = 0}) => TextStyle(
      fontFamily: 'Inter',
      fontSize: size,
      fontWeight: w,
      fontVariations: [ui.FontVariation.weight(w.value.toDouble())],
      color: c,
      height: (lh ?? size * 1.5) / size,
      letterSpacing: ls,
      leadingDistribution: TextLeadingDistribution.even,
    );

TextStyle mono(double size, {Color c = zinc900, double? lh}) => TextStyle(
      fontFamily: 'Consolas',
      fontSize: size,
      color: c,
      height: lh == null ? null : lh / size,
      leadingDistribution: TextLeadingDistribution.even,
    );

/// A CSS 1 px border: Chrome draws it as whole device pixels.
double hair = 1;

const _shadowSm = [BoxShadow(color: Color(0x0d000000), offset: Offset(0, 1), blurRadius: 2)];

Widget svgIcon(String svg, double size, Color color) =>
    SvgPicture.string(svg, width: size, height: size, theme: SvgTheme(currentColor: color));

String _lucide(String body) =>
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">$body</svg>';

final _iconAccounts = _lucide('<path d="m12 2 8 4-8 4-8-4 8-4z"/><path d="m4 14 8 4 8-4"/><path d="m4 10 8 4 8-4"/>');
final _iconAdd = _lucide('<circle cx="12" cy="12" r="10"/><path d="M12 8v8"/><path d="M8 12h8"/>');
final _iconMeridian = _lucide('<rect x="2" y="3" width="20" height="8" rx="2"/><rect x="2" y="13" width="20" height="8" rx="2"/><path d="M6 7h.01"/><path d="M6 17h.01"/>');
final _iconBehavior = _lucide(
    '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M3 21v-5h5"/>');
final _iconStartup = _lucide('<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>');
final _iconDev = _lucide('<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>');
final _iconInfo = _lucide('<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>');
final _iconChevron = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>';

const _iconGrip = '<svg viewBox="0 0 16 16" fill="currentColor"><circle cx="6" cy="3.5" r="1.25"/><circle cx="10" cy="3.5" r="1.25"/>'
    '<circle cx="6" cy="8" r="1.25"/><circle cx="10" cy="8" r="1.25"/><circle cx="6" cy="12.5" r="1.25"/><circle cx="10" cy="12.5" r="1.25"/></svg>';
const _starPath = 'M10 2.2l2.4 4.9 5.4.8-3.9 3.8.9 5.4L10 14.6l-4.8 2.5.9-5.4-3.9-3.8 5.4-.8z';
const _iconStar = '<svg viewBox="0 0 20 20"><path d="$_starPath" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>';
const _iconStarFilled = '<svg viewBox="0 0 20 20"><path d="$_starPath" fill="currentColor" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>';
const _iconEdit =
    '<svg viewBox="0 0 16 16"><path d="M11 2.5l2.5 2.5L6 12.5H3.5V10z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>';
const _iconRemove = '<svg viewBox="0 0 16 16"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';

const _sourceTags = {Source.meridian: 'Meridian', Source.oauth: 'Sign-in', Source.token: 'Token'};

const _bodyStrut = StrutStyle(fontFamily: 'Inter', fontSize: 16, height: 1.5, leadingDistribution: TextLeadingDistribution.even);

String _errText(Object e) => e is AppError ? e.message : e.toString();

// ---- Page ----

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.api});

  final Api api;

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

enum _Tone { info, ok, err }

class _SettingsPageState extends State<SettingsPage> {
  Map<String, dynamic>? _cfg;
  bool _dragging = false;
  bool _reloadAfterDrag = false;
  String _accountsStatus = '';
  Provider? _addProvider;
  (String, _Tone) _addStatus = ('', _Tone.info);
  String _meridianError = '';
  bool _meridianDirty = false;
  bool? _autostart;
  int? _interval;
  String? _iconStyle;
  bool _busy = false;
  int _codexRun = 0;
  bool _claudeStep2 = false;
  List<({String id, String label})> _imports = [];

  final _claudeLabel = TextEditingController();
  final _claudeCode = TextEditingController();
  final _claudeUrl = TextEditingController();
  final _codexLabel = TextEditingController();
  final _copilotLabel = TextEditingController();
  final _copilotToken = TextEditingController();
  final _meridianUrl = TextEditingController();
  final _meridianKey = TextEditingController();
  final _meridianUrlFocus = FocusNode();
  final _claudeCodeFocus = FocusNode();

  Api get api => widget.api;

  @override
  void initState() {
    super.initState();
    api.updates.listen((_) => _reload());
    _init();
  }

  Future<void> _init() async {
    await _reload();
    final autostart = await api.call('settings_get_autostart') as bool;
    setState(() => _autostart = autostart);
  }

  Future<void> _reload() async {
    if (_dragging) {
      _reloadAfterDrag = true;
      return;
    }
    final cfg = await api.call('settings_get_all') as Map<String, dynamic>;
    if (!mounted) return;
    setState(() {
      _cfg = cfg;
      _interval ??= cfg['pollIntervalMs'] as int;
      _iconStyle ??= cfg['iconStyle'] as String;
      final m = cfg['meridian'] as Map<String, dynamic>;
      if (!_meridianDirty && !_meridianUrlFocus.hasFocus) _meridianUrl.text = m['url'] as String;
    });
  }

  /// Non-add actions: the error goes to that block's status line, and the page
  /// always reloads to match the main engine.
  Future<void> _runAction(void Function(String) setStatus, Future<void> Function() fn) async {
    setState(() => setStatus(''));
    try {
      await fn();
    } catch (e) {
      setState(() => setStatus(_errText(e)));
    } finally {
      await _reload();
    }
  }

  void _setAccountsStatus(String s) => _accountsStatus = s;
  void _setMeridianError(String s) => _meridianError = s;

  /// Add buttons stay disabled while their call runs.
  Future<void> _busyRun(Future<void> Function() fn) async {
    setState(() => _busy = true);
    try {
      await fn();
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _status(String text, [_Tone tone = _Tone.info]) => setState(() => _addStatus = (text, tone));

  List<Account> get _accounts => [for (final a in (_cfg?['accounts'] as List? ?? [])) Account.fromJson(a as Map<String, dynamic>)];

  @override
  Widget build(BuildContext context) {
    final cfg = _cfg;
    return Scaffold(
      backgroundColor: zinc50,
      body: cfg == null ? const SizedBox.shrink() : _page(cfg),
    );
  }

  Widget _page(Map<String, dynamic> cfg) {
    final content = Padding(
      padding: const EdgeInsets.fromLTRB(24, 20, 24, 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _section(_iconAccounts, 'Accounts',
              description: 'Rows shown in the tray popover, in this order. Drag to reorder, use the pencil to rename. The star drives the tray icon number + ring.',
              children: [
                _card(child: _accountList(cfg)),
                if (_accountsStatus.isNotEmpty) _statusLine(_accountsStatus, red600, indent: true),
              ]),
          const SizedBox(height: 20),
          _section(_iconAdd, 'Add account', children: [_card(padding: const EdgeInsets.all(20), child: _addCard())]),
          const SizedBox(height: 20),
          _section(_iconMeridian, 'Meridian',
              description: 'Claude profiles from a local Meridian.',
              children: [_card(padding: const EdgeInsets.all(20), child: _meridianCard(cfg))]),
          const SizedBox(height: 20),
          _section(_iconBehavior, 'Behavior', children: [_card(padding: const EdgeInsets.all(20), child: _behaviorCard(cfg))]),
          const SizedBox(height: 20),
          _section(_iconStartup, 'Startup', children: [
            _card(
              padding: const EdgeInsets.all(16),
              child: _toggleRow(
                'Start with Windows',
                Text.rich(TextSpan(children: [
                  const TextSpan(text: 'Writes '),
                  _code(r'HKCU\Software\Microsoft\Windows\CurrentVersion\Run', 10),
                ]), style: inter(11, c: zinc500)),
                _autostart ?? false,
                (on) async {
                  final now = await api.call('settings_set_autostart', {'on': on}) as bool;
                  setState(() => _autostart = now);
                },
              ),
            ),
          ]),
          const SizedBox(height: 20),
          _section(_iconDev, 'Developer', children: [
            _card(
              padding: const EdgeInsets.all(16),
              child: _toggleRow(
                'Developer mode',
                Text('Reveals "simulate state" + "icon style" submenus in tray right-click.', style: inter(11, c: zinc500)),
                cfg['devMode'] as bool,
                (on) => _runAction((_) {}, () => api.call('settings_set_dev_mode', {'on': on})),
              ),
            ),
          ]),
          const SizedBox(height: 20),
          _dataSources(),
        ],
      ),
    );
    return LayoutBuilder(builder: (context, box) {
      return Stack(
        children: [
          RawScrollbar(
            thumbVisibility: true,
            thickness: 6,
            radius: const Radius.circular(3),
            thumbColor: zinc300,
            crossAxisMargin: 2,
            mainAxisMargin: 2,
            child: ScrollConfiguration(
              behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
              child: SingleChildScrollView(
                child: Container(
                  constraints: BoxConstraints(minHeight: box.maxHeight),
                  decoration: const BoxDecoration(
                    gradient: LinearGradient(begin: Alignment.topCenter, end: Alignment.bottomCenter, colors: [zinc50, zinc100]),
                  ),
                  padding: EdgeInsets.only(right: _scrollbar, top: 66.5 + hair),
                  child: Align(
                    alignment: Alignment.topCenter,
                    child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 512), child: content),
                  ),
                ),
              ),
            ),
          ),
          Positioned(top: 0, left: 0, right: _scrollbar, child: _header()),
        ],
      );
    });
  }

  /// The page scrollbar: 10 CSS px, whole device pixels.
  double get _scrollbar => (10 * View.of(context).devicePixelRatio).floorToDouble() / View.of(context).devicePixelRatio;

  Widget _header() => ClipRect(
        child: BackdropFilter(
          filter: ui.ImageFilter.blur(sigmaX: 8, sigmaY: 8),
          child: Container(
            decoration: BoxDecoration(
              color: const Color(0xccffffff),
              border: Border(bottom: BorderSide(color: zinc200, width: hair)),
            ),
            alignment: Alignment.center,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 512),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 12),
                child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('Settings', style: inter(16, w: FontWeight.w600, lh: 24, ls: -0.4)),
                          const SizedBox(height: 2),
                          Text('ai-plan-usage', style: inter(11, c: zinc500)),
                        ],
                      ),
                    ),
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                      decoration: BoxDecoration(color: zinc100, borderRadius: BorderRadius.circular(999)),
                      child: Text('LOCAL', style: inter(10, w: FontWeight.w500, c: zinc600, ls: 0.5)),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );

  Widget _section(String icon, String title, {String? description, required List<Widget> children}) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(children: [
            svgIcon(icon, 16, zinc500),
            const SizedBox(width: 8),
            Text(title, style: inter(14, w: FontWeight.w600, lh: 20, ls: -0.35)),
          ]),
          if (description != null)
            Padding(
              padding: const EdgeInsets.only(left: 24, top: 8),
              child: Text(description, style: inter(12, c: zinc500, lh: 16)),
            ),
          for (final c in children) Padding(padding: const EdgeInsets.only(top: 8), child: c),
        ],
      );

  Widget _card({required Widget child, EdgeInsets padding = EdgeInsets.zero}) => ClipRRect(
        borderRadius: BorderRadius.circular(16),
        child: BackdropFilter(
          filter: ui.ImageFilter.blur(sigmaX: 8, sigmaY: 8),
          child: Container(
            padding: padding,
            decoration: BoxDecoration(
              color: const Color(0xb3ffffff),
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: zinc200, width: hair),
              boxShadow: _shadowSm,
            ),
            child: child,
          ),
        ),
      );

  Widget _statusLine(String text, Color color, {bool indent = false}) => Padding(
        padding: EdgeInsets.only(left: indent ? 24 : 0),
        child: Text(text, style: inter(11, c: color)),
      );

  // Inline padding paints outside the line box in CSS: the chip's vertical
  // padding overflows its placeholder instead of growing the line.
  InlineSpan _code(String text, double size) => WidgetSpan(
        alignment: PlaceholderAlignment.middle,
        child: SizedBox(
          height: size * 1.2,
          child: OverflowBox(
            maxHeight: double.infinity,
            fit: OverflowBoxFit.deferToChild,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
              decoration: BoxDecoration(color: zinc100, borderRadius: BorderRadius.circular(4)),
              child: Text(text, style: mono(size, c: zinc500)),
            ),
          ),
        ),
      );

  Widget _toggleRow(String title, Widget subtitle, bool value, ValueChanged<bool> onChanged) => Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: inter(14, w: FontWeight.w500, lh: 20)),
                const SizedBox(height: 2),
                subtitle,
              ],
            ),
          ),
          const SizedBox(width: 16),
          Toggle(value: value, onChanged: onChanged),
        ],
      );

  // ---- Accounts ----

  Widget _accountList(Map<String, dynamic> cfg) {
    final accounts = _accounts;
    if (accounts.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
        child: Text('No accounts yet. Add one below, or start Meridian.', style: inter(12, c: zinc500, lh: 16)),
      );
    }
    return ReorderableListView(
      shrinkWrap: true,
      physics: const NeverScrollableScrollPhysics(),
      buildDefaultDragHandles: false,
      proxyDecorator: (child, _, _) => Opacity(opacity: 0.6, child: ColoredBox(color: zinc50, child: child)),
      onReorderStart: (_) => _dragging = true,
      onReorderEnd: (_) {
        _dragging = false;
        if (_reloadAfterDrag) {
          _reloadAfterDrag = false;
          _reload();
        }
      },
      onReorder: (from, to) {
        if (to > from) to -= 1;
        if (to == from) return;
        final id = accounts[from].id;
        setState(() {
          final list = cfg['accounts'] as List;
          list.insert(to, list.removeAt(from));
        });
        _runAction(_setAccountsStatus, () => api.call('account_move', {'id': id, 'to': to}));
      },
      children: [
        for (final (i, a) in accounts.indexed)
          AccountRow(
            key: ValueKey(a.id),
            index: i,
            account: a,
            primary: cfg['primary'] == a.id,
            first: i == 0,
            onStar: () => _runAction(_setAccountsStatus, () => api.call('settings_set_primary', {'id': a.id})),
            onToggle: (on) => _runAction(_setAccountsStatus, () => api.call('account_set_enabled', {'id': a.id, 'on': on})),
            onRename: (label) => _runAction(_setAccountsStatus, () async {
              if (label.isNotEmpty && label != a.label) await api.call('account_rename', {'id': a.id, 'label': label});
            }),
            onRemove: () => _runAction(_setAccountsStatus, () => api.call('account_remove', {'id': a.id})),
          ),
      ],
    );
  }

  // ---- Add account ----

  void _showAdd(Provider? p) => setState(() {
        _addProvider = p;
        _addStatus = ('', _Tone.info);
      });

  Widget _addCard() {
    final p = _addProvider;
    final status = _addStatus;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(children: [
          for (final (i, prov) in Provider.values.indexed) ...[
            if (i > 0) const SizedBox(width: 8),
            Expanded(
              child: QuietButton(
                pressed: p == prov,
                onPressed: () => _showAdd(p == prov ? null : prov),
                icon: logos[prov]!,
                label: providerNames[prov]!,
              ),
            ),
          ],
        ]),
        if (p == null) ...[const SizedBox(height: 16), Text('Pick where the account signs in.', style: inter(12, c: zinc500, lh: 16))],
        if (p == Provider.claude) ...[const SizedBox(height: 16), _addClaude()],
        if (p == Provider.codex) ...[const SizedBox(height: 16), _addCodex()],
        if (p == Provider.copilot) ...[const SizedBox(height: 16), _addCopilot()],
        if (status.$1.isNotEmpty) ...[
          const SizedBox(height: 16),
          Text(status.$1, style: inter(11, c: switch (status.$2) { _Tone.ok => emerald600, _Tone.err => red600, _Tone.info => zinc500 })),
        ],
      ],
    );
  }

  // An inline <label> in a block div sits on that div's 16 px / 1.5 strut:
  // a 24 px line box, text on the strut's baseline.
  Widget _label(String text) => Padding(
        padding: const EdgeInsets.only(bottom: 6),
        child: Text(text, style: inter(12, w: FontWeight.w500, c: zinc700, lh: 16), strutStyle: _bodyStrut),
      );

  Widget _hint(String text) => Text(text, style: inter(12, c: zinc500, lh: 12 * 1.625));

  Widget _addClaude() => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _hint('Meridian profiles appear on their own. This signs in to Claude directly: the browser opens, and the last page shows a code to paste here.'),
          const SizedBox(height: 12),
          _label('Account name'),
          Row(children: [
            Expanded(child: Field(controller: _claudeLabel, placeholder: 'For example Work')),
            const SizedBox(width: 8),
            DarkButton(
              label: 'Open sign-in page',
              enabled: !_busy,
              onPressed: () => _busyRun(() async {
                try {
                  _claudeUrl.text = await api.call('claude_login_start') as String;
                  setState(() => _claudeStep2 = true);
                  _claudeCodeFocus.requestFocus();
                  _status('Sign in in the browser, then paste the code from the last page.');
                } catch (e) {
                  _status(_errText(e), _Tone.err);
                }
              }),
            ),
          ]),
          if (_claudeStep2) ...[
            const SizedBox(height: 12),
            _label('Code from the last page'),
            Row(children: [
              Expanded(child: Field(controller: _claudeCode, focusNode: _claudeCodeFocus, placeholder: 'Paste the code', monospace: true)),
              const SizedBox(width: 8),
              DarkButton(
                label: 'Add',
                enabled: !_busy,
                onPressed: () => _busyRun(() async {
                  try {
                    await api.call('claude_login_finish', {'code': _claudeCode.text, 'label': _claudeLabel.text});
                    _claudeCode.clear();
                    setState(() => _claudeStep2 = false);
                    _status('Claude account added.', _Tone.ok);
                    await _reload();
                  } catch (e) {
                    _status(_errText(e), _Tone.err);
                  }
                }),
              ),
            ]),
            const SizedBox(height: 6),
            Field(controller: _claudeUrl, readOnly: true, fontSize: 12),
          ],
        ],
      );

  Widget _addCodex() => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _hint('The browser opens on the ChatGPT sign-in page. After signing in, the account shows up here.'),
          const SizedBox(height: 12),
          _label('Account name'),
          Row(children: [
            Expanded(child: Field(controller: _codexLabel, placeholder: 'For example Personal')),
            const SizedBox(width: 8),
            DarkButton(
              label: 'Sign in with browser',
              onPressed: () async {
                // A new click replaces a sign-in still waiting for the browser.
                final run = ++_codexRun;
                _status('Finish signing in in the browser…');
                try {
                  await api.call('codex_login', {'label': _codexLabel.text});
                  if (run != _codexRun) return;
                  _status('Codex account added.', _Tone.ok);
                  await _reload();
                } catch (e) {
                  if (run != _codexRun) return;
                  _status(_errText(e), _Tone.err);
                }
              },
            ),
          ]),
        ],
      );

  Widget _addCopilot() => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              SizedBox(
                width: 128,
                child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                  _label('Account name'),
                  Field(controller: _copilotLabel, placeholder: 'Work'),
                ]),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                  _label('GitHub token'),
                  Field(controller: _copilotToken, placeholder: 'gho_... or ghp_...', monospace: true, obscure: true),
                ]),
              ),
              const SizedBox(width: 8),
              DarkButton(
                label: 'Add',
                enabled: !_busy,
                onPressed: () => _busyRun(() async {
                  try {
                    await api.call('copilot_add_token', {'token': _copilotToken.text, 'label': _copilotLabel.text});
                    _copilotToken.clear();
                    _status('Copilot account added.', _Tone.ok);
                    await _reload();
                  } catch (e) {
                    _status(_errText(e), _Tone.err);
                  }
                }),
              ),
            ],
          ),
          const SizedBox(height: 12),
          Text.rich(
            TextSpan(children: [
              const TextSpan(text: 'Token with Copilot access, for example from '),
              _code('gh auth token', 11),
              const TextSpan(text: '. Stored as plain text in the settings file.'),
            ]),
            style: inter(11, c: zinc500, lh: 11 * 1.625),
          ),
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.only(top: 12),
            decoration: BoxDecoration(border: Border(top: BorderSide(color: zinc100, width: hair))),
            child: Row(children: [
              Expanded(child: Text('Or copy a login from OpenCode', style: inter(12, c: zinc500, lh: 16))),
              const SizedBox(width: 12),
              QuietButton(label: 'Show OpenCode logins', enabled: !_busy, onPressed: _loadImports),
            ]),
          ),
          for (final l in _imports) ...[const SizedBox(height: 8), _importRow(l)],
        ],
      );

  Future<void> _loadImports() => _busyRun(() async {
        setState(() => _imports = []);
        try {
          final logins = [
            for (final l in await api.call('copilot_import_list') as List)
              (id: (l as Map)['id'] as String, label: l['label'] as String),
          ];
          setState(() => _imports = logins);
          _status(logins.isEmpty ? 'No Copilot logins in OpenCode.' : '');
        } catch (e) {
          _status(_errText(e), _Tone.err);
        }
      });

  Widget _importRow(({String id, String label}) l) => Container(
        padding: const EdgeInsets.fromLTRB(12, 6, 6, 6),
        decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(8), border: Border.all(color: zinc200, width: hair)),
        child: Row(children: [
          SvgPicture.asset(logos[Provider.copilot]!, width: 16, height: 16),
          const SizedBox(width: 12),
          Expanded(child: Text(l.label, maxLines: 1, overflow: TextOverflow.ellipsis, style: inter(14, lh: 20))),
          const SizedBox(width: 12),
          QuietButton(
            label: 'Add',
            onPressed: () => _busyRun(() async {
              try {
                await api.call('copilot_import', {'id': l.id, 'label': l.label});
                _status('Copilot · ${l.label} added.', _Tone.ok);
                setState(() => _imports = [for (final x in _imports) if (x.id != l.id) x]);
                await _reload();
              } catch (e) {
                _status(_errText(e), _Tone.err);
              }
            }),
          ),
        ]),
      );

  // ---- Meridian ----

  Widget _meridianCard(Map<String, dynamic> cfg) {
    final m = cfg['meridian'] as Map<String, dynamic>;
    final st = cfg['meridianStatus'] as Map<String, dynamic>;
    final enabled = m['enabled'] as bool;
    final hasKey = m['hasApiKey'] as bool;
    final url = m['url'] as String;
    final checkedAt = DateTime.tryParse(st['checkedAt'] as String? ?? '')?.toLocal();
    final checked = checkedAt == null
        ? ''
        : ', checked ${checkedAt.hour.toString().padLeft(2, '0')}:${checkedAt.minute.toString().padLeft(2, '0')}';
    final reachable = st['reachable'] as bool;
    final profiles = st['profiles'] as int;
    var status = 'Not reachable$checked';
    if (!enabled || url.isEmpty) {
      status = 'Off';
    } else if (checkedAt == null) {
      status = 'Not checked yet';
    } else if (reachable) {
      status = 'Reachable, $profiles Claude profile${profiles == 1 ? '' : 's'}$checked';
    }
    final green = reachable && enabled && url.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _toggleRow(
          'Use Meridian',
          Text('Off: Meridian is not asked, and its accounts are removed. On adds them back.', style: inter(11, c: zinc500)),
          enabled,
          (on) => _runAction(_setMeridianError, () => api.call('meridian_set_enabled', {'on': on})),
        ),
        if (enabled) ...[
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.only(top: 12),
            decoration: BoxDecoration(border: Border(top: BorderSide(color: zinc100, width: hair))),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _label('Address'),
                Field(
                  controller: _meridianUrl,
                  focusNode: _meridianUrlFocus,
                  placeholder: 'http://127.0.0.1:3456',
                  monospace: true,
                  onChanged: (_) => _meridianDirty = true,
                ),
                const SizedBox(height: 12),
                _label('API key'),
                Field(
                  controller: _meridianKey,
                  placeholder: hasKey ? 'API key saved, type to replace' : 'API key, only if MERIDIAN_API_KEY is set',
                  monospace: true,
                  obscure: true,
                ),
                const SizedBox(height: 16),
                Row(children: [
                  Container(
                    width: 6,
                    height: 6,
                    margin: const EdgeInsets.only(right: 6),
                    decoration: BoxDecoration(color: green ? emerald500 : zinc400, shape: BoxShape.circle),
                  ),
                  Expanded(child: Text(status, style: inter(12, c: green ? emerald600 : zinc500, lh: 16))),
                  if (hasKey) ...[
                    _RemoveKeyButton(
                      onPressed: () => _runAction(_setMeridianError, () async {
                        await api.call('meridian_set', {'url': _meridianUrl.text, 'apiKey': ''});
                        _meridianDirty = false;
                      }),
                    ),
                    const SizedBox(width: 8),
                  ],
                  DarkButton(
                    label: 'Save',
                    onPressed: () => _runAction(_setMeridianError, () async {
                      final key = _meridianKey.text.trim();
                      await api.call('meridian_set', {'url': _meridianUrl.text, 'apiKey': key.isEmpty ? null : key});
                      _meridianKey.clear();
                      _meridianDirty = false;
                    }),
                  ),
                ]),
              ],
            ),
          ),
        ],
        if (_meridianError.isNotEmpty) ...[const SizedBox(height: 12), Text(_meridianError, style: inter(11, c: red600))],
      ],
    );
  }

  // ---- Behavior ----

  Widget _behaviorCard(Map<String, dynamic> cfg) {
    final intervals = [for (final i in cfg['intervals'] as List) (label: (i as Map)['label'] as String, ms: i['ms'] as int)];
    final styles = [for (final s in cfg['iconStyles'] as List) s as String];
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            _label('Poll interval'),
            Select<int>(
              value: _interval,
              options: [for (final i in intervals) (i.ms, i.label)],
              onChanged: (ms) async {
                final now = await api.call('settings_set_interval', {'ms': ms}) as int;
                setState(() => _interval = now);
              },
            ),
          ]),
        ),
        const SizedBox(width: 20),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            _label('Icon style'),
            Select<String>(
              value: _iconStyle,
              options: [for (final s in styles) (s, s)],
              onChanged: (style) async {
                final now = await api.call('settings_set_icon_style', {'style': style}) as String;
                setState(() => _iconStyle = now);
              },
            ),
          ]),
        ),
      ],
    );
  }

  Widget _dataSources() => CustomPaint(
        painter: _DashedBorder(color: zinc300, radius: 16),
        child: Container(
          padding: const EdgeInsets.all(20),
          decoration: BoxDecoration(color: const Color(0x66ffffff), borderRadius: BorderRadius.circular(16)),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(padding: const EdgeInsets.only(top: 2), child: svgIcon(_iconInfo, 16, zinc400)),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Data sources', style: inter(11, w: FontWeight.w500, c: zinc700, lh: 11 * 1.625)),
                    const SizedBox(height: 2),
                    Text.rich(
                      TextSpan(children: [
                        const TextSpan(
                            text: 'Claude from Meridian or a direct sign-in, Codex from a browser sign-in, Copilot from a GitHub token. '
                                'Tokens are stored as plain text in '),
                        _code(r'%APPDATA%\ai-plan-usage-flutter\settings.json', 11),
                        const TextSpan(text: '.'),
                      ]),
                      style: inter(11, c: zinc600, lh: 11 * 1.625),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      );
}

// ---- Rows and controls ----

class AccountRow extends StatefulWidget {
  const AccountRow({
    super.key,
    required this.index,
    required this.account,
    required this.primary,
    required this.first,
    required this.onStar,
    required this.onToggle,
    required this.onRename,
    required this.onRemove,
  });

  final int index;
  final Account account;
  final bool primary;
  final bool first;
  final VoidCallback onStar;
  final ValueChanged<bool> onToggle;
  final ValueChanged<String> onRename;
  final VoidCallback onRemove;

  @override
  State<AccountRow> createState() => _AccountRowState();
}

class _AccountRowState extends State<AccountRow> {
  bool _hover = false;
  bool _renaming = false;
  bool _armed = false;
  Timer? _disarm;
  final _name = TextEditingController();
  final _nameFocus = FocusNode();

  @override
  void initState() {
    super.initState();
    _nameFocus.addListener(() {
      if (!_nameFocus.hasFocus && _renaming) _finish(true);
    });
  }

  @override
  void dispose() {
    _disarm?.cancel();
    _name.dispose();
    _nameFocus.dispose();
    super.dispose();
  }

  void _startRename() {
    _name.text = widget.account.label;
    _name.selection = TextSelection(baseOffset: 0, extentOffset: _name.text.length);
    setState(() => _renaming = true);
    _nameFocus.requestFocus();
  }

  void _finish(bool save) {
    if (!_renaming) return;
    setState(() => _renaming = false);
    widget.onRename(save ? _name.text.trim() : widget.account.label);
  }

  void _remove() {
    if (!_armed) {
      setState(() => _armed = true);
      _disarm = Timer(const Duration(seconds: 3), () {
        if (mounted) setState(() => _armed = false);
      });
      return;
    }
    widget.onRemove();
  }

  @override
  Widget build(BuildContext context) {
    final a = widget.account;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        constraints: const BoxConstraints(minHeight: 56),
        padding: const EdgeInsets.fromLTRB(8, 8, 16, 8),
        decoration: BoxDecoration(
          color: _hover ? const Color(0x80fafafa) : const Color(0x00fafafa),
          border: widget.first ? null : Border(top: BorderSide(color: zinc100, width: hair)),
        ),
        child: Row(
          children: [
            ReorderableDragStartListener(
              index: widget.index,
              child: MouseRegion(
                cursor: SystemMouseCursors.grab,
                child: Tooltip(
                  message: 'Drag to reorder',
                  child: SizedBox(width: 24, height: 32, child: Center(child: _HoverIcon(svg: _iconGrip, size: 16))),
                ),
              ),
            ),
            const SizedBox(width: 8),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Opacity(opacity: a.enabled ? 1 : 0.4, child: SvgPicture.asset(logos[a.provider]!, width: 20, height: 20)),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    if (_renaming)
                      Expanded(
                        child: SizedBox(
                          height: 32,
                          child: CallbackShortcuts(
                            bindings: {const SingleActivator(LogicalKeyboardKey.escape): () => _finish(false)},
                            child: TextField(
                              controller: _name,
                              focusNode: _nameFocus,
                              onSubmitted: (_) => _finish(true),
                              style: inter(14, lh: 20),
                              decoration: InputDecoration(
                                isDense: true,
                                contentPadding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                                enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(6), borderSide: const BorderSide(color: zinc300)),
                                focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(6), borderSide: const BorderSide(color: brand, width: 2)),
                              ),
                            ),
                          ),
                        ),
                      )
                    else ...[
                      Flexible(
                        child: GestureDetector(
                          onTap: _startRename,
                          child: MouseRegion(
                            cursor: SystemMouseCursors.click,
                            child: Tooltip(
                              message: 'Rename',
                              child: SizedBox(
                                height: 24,
                                child: Align(
                                  alignment: Alignment.centerLeft,
                                  widthFactor: 1,
                                  child: Text(a.label,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: inter(14, w: FontWeight.w500, lh: 20, c: a.enabled ? zinc900 : zinc400)),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(width: 4),
                      IconBox(svg: _iconEdit, iconSize: 14, box: 24, radius: 4, tooltip: 'Rename', onPressed: _startRename),
                    ],
                  ]),
                  Text(
                    '${providerNames[a.provider]} · ${_sourceTags[a.source]}${a.enabled ? '' : ' · off'}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: inter(12, c: zinc500, lh: 16),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 8),
            IconBox(
              svg: widget.primary ? _iconStarFilled : _iconStar,
              iconSize: 18,
              box: 32,
              radius: 6,
              color: widget.primary ? star : null,
              enabled: a.enabled,
              tooltip: widget.primary ? 'Behind the tray icon' : 'Use for the tray icon',
              onPressed: widget.onStar,
            ),
            const SizedBox(width: 8),
            Toggle(value: a.enabled, onChanged: widget.onToggle),
            const SizedBox(width: 8),
            _armed
                ? Pressable(
                    onPressed: _remove,
                    child: Container(
                      height: 32,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                      alignment: Alignment.center,
                      decoration: BoxDecoration(color: const Color(0x1adc2626), borderRadius: BorderRadius.circular(6)),
                      child: Text('Remove?', style: inter(12, w: FontWeight.w500, c: red600, lh: 16)),
                    ),
                  )
                : IconBox(svg: _iconRemove, iconSize: 14, box: 32, radius: 6, hoverColor: red600, hoverBg: const Color(0x1adc2626), onPressed: _remove),
          ],
        ),
      ),
    );
  }
}

class _HoverIcon extends StatefulWidget {
  const _HoverIcon({required this.svg, required this.size});

  final String svg;
  final double size;

  @override
  State<_HoverIcon> createState() => _HoverIconState();
}

class _HoverIconState extends State<_HoverIcon> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) => MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: svgIcon(widget.svg, widget.size, _hover ? zinc900 : zinc400),
      );
}

/// Square icon button: zinc-400 icon, darker with a light background on hover.
class IconBox extends StatelessWidget {
  const IconBox({
    super.key,
    required this.svg,
    required this.iconSize,
    required this.box,
    required this.radius,
    required this.onPressed,
    this.color,
    this.hoverColor = zinc900,
    this.hoverBg = zinc100,
    this.enabled = true,
    this.tooltip,
  });

  final String svg;
  final double iconSize;
  final double box;
  final double radius;
  final VoidCallback onPressed;
  final Color? color;
  final Color hoverColor;
  final Color hoverBg;
  final bool enabled;
  final String? tooltip;

  @override
  Widget build(BuildContext context) {
    final button = Opacity(
      opacity: enabled ? 1 : 0.3,
      child: Pressable(
        onPressed: onPressed,
        enabled: enabled,
        outline: brand,
        radius: radius,
        builder: (hover) => Container(
          width: box,
          height: box,
          alignment: Alignment.center,
          decoration: BoxDecoration(color: hover && enabled ? hoverBg : null, borderRadius: BorderRadius.circular(radius)),
          child: svgIcon(svg, iconSize, color ?? (hover && enabled ? hoverColor : zinc400)),
        ),
      ),
    );
    return tooltip == null ? button : Tooltip(message: tooltip!, child: button);
  }
}

/// `.switch`: 44 x 24 track, emerald when on, 20 px knob.
class Toggle extends StatelessWidget {
  const Toggle({super.key, required this.value, required this.onChanged});

  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) => Pressable(
        onPressed: () => onChanged(!value),
        outline: brand,
        radius: 12,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          width: 44,
          height: 24,
          decoration: BoxDecoration(color: value ? emerald500 : zinc200, borderRadius: BorderRadius.circular(999)),
          child: AnimatedAlign(
            duration: const Duration(milliseconds: 150),
            alignment: value ? Alignment.centerRight : Alignment.centerLeft,
            child: Container(
              width: 20,
              height: 20,
              margin: const EdgeInsets.all(2),
              decoration: const BoxDecoration(
                color: Colors.white,
                shape: BoxShape.circle,
                boxShadow: [
                  BoxShadow(color: Color(0x1a000000), offset: Offset(0, 1), blurRadius: 3),
                  BoxShadow(color: Color(0x0f000000), offset: Offset(0, 1), blurRadius: 2),
                  BoxShadow(color: zinc200, spreadRadius: 1),
                ],
              ),
            ),
          ),
        ),
      );
}

/// `.btn`: 40 px, zinc-900, white text.
class DarkButton extends StatelessWidget {
  const DarkButton({super.key, required this.label, required this.onPressed, this.enabled = true});

  final String label;
  final VoidCallback onPressed;
  final bool enabled;

  @override
  Widget build(BuildContext context) => Opacity(
        opacity: enabled ? 1 : 0.5,
        child: Pressable(
          onPressed: onPressed,
          enabled: enabled,
          outline: brand,
          radius: 8,
          builder: (hover) => Container(
            height: 40,
            padding: const EdgeInsets.symmetric(horizontal: 16),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: hover ? const Color(0xff27272a) : zinc900,
              borderRadius: BorderRadius.circular(8),
              boxShadow: _shadowSm,
            ),
            child: Text(label, style: inter(14, w: FontWeight.w500, c: Colors.white, lh: 20)),
          ),
        ),
      );
}

/// `.btn-quiet`: 36 px, white with a zinc border; dark when pressed.
class QuietButton extends StatelessWidget {
  const QuietButton({super.key, required this.label, required this.onPressed, this.icon, this.pressed = false, this.enabled = true});

  final String label;
  final VoidCallback onPressed;
  final String? icon;
  final bool pressed;
  final bool enabled;

  @override
  Widget build(BuildContext context) => Opacity(
        opacity: enabled ? 1 : 0.5,
        child: Pressable(
          onPressed: onPressed,
          enabled: enabled,
          outline: brand,
          radius: 8,
          builder: (hover) => Container(
            height: 36,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: pressed ? zinc900 : Colors.white,
              border: Border.all(color: pressed ? zinc900 : (hover ? zinc300 : zinc200), width: hair),
              borderRadius: BorderRadius.circular(8),
              boxShadow: pressed ? _shadowSm : null,
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (icon != null) ...[
                  SvgPicture.asset(icon!,
                      width: 16, height: 16, colorFilter: pressed ? const ColorFilter.mode(Colors.white, BlendMode.srcIn) : null),
                  const SizedBox(width: 8),
                ],
                Text(label, style: inter(14, w: FontWeight.w500, c: pressed ? Colors.white : zinc700, lh: 20)),
              ],
            ),
          ),
        ),
      );
}

class _RemoveKeyButton extends StatelessWidget {
  const _RemoveKeyButton({required this.onPressed});

  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => Pressable(
        onPressed: onPressed,
        outline: brand,
        radius: 8,
        builder: (hover) => Container(
          height: 36,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          alignment: Alignment.center,
          decoration: BoxDecoration(color: hover ? const Color(0xfffef2f2) : null, borderRadius: BorderRadius.circular(8)),
          child: Text('Remove key', style: inter(12, w: FontWeight.w500, c: hover ? red600 : zinc500, lh: 16)),
        ),
      );
}

/// `.field`: 40 px input, zinc border, indigo ring on focus.
class Field extends StatelessWidget {
  const Field({
    super.key,
    required this.controller,
    this.focusNode,
    this.placeholder,
    this.monospace = false,
    this.obscure = false,
    this.readOnly = false,
    this.fontSize = 14,
    this.onChanged,
  });

  final TextEditingController controller;
  final FocusNode? focusNode;
  final String? placeholder;
  final bool monospace;
  final bool obscure;
  final bool readOnly;
  final double fontSize;
  final ValueChanged<String>? onChanged;

  @override
  Widget build(BuildContext context) {
    // A whole-pixel line box keeps the decorated field at exactly 40 px.
    final line = fontSize + 6;
    final style = monospace ? mono(fontSize, c: readOnly ? zinc500 : zinc900, lh: line) : inter(fontSize, c: readOnly ? zinc500 : zinc900, lh: line);
    OutlineInputBorder border(Color c, [double? w]) =>
        OutlineInputBorder(borderRadius: BorderRadius.circular(8), borderSide: BorderSide(color: c, width: w ?? hair));
    return Container(
      height: 40,
      decoration: BoxDecoration(borderRadius: BorderRadius.circular(8), boxShadow: _shadowSm),
      child: TextField(
        controller: controller,
        focusNode: focusNode,
        readOnly: readOnly,
        obscureText: obscure,
        obscuringCharacter: '•',
        onChanged: onChanged,
        style: style,
        textAlignVertical: TextAlignVertical.center,
        decoration: InputDecoration(
          isDense: true,
          filled: true,
          fillColor: readOnly ? zinc50 : Colors.white,
          hintText: placeholder,
          hintStyle: style.copyWith(color: zinc400),
          contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: (40 - line) / 2),
          enabledBorder: border(zinc200),
          focusedBorder: border(brand, 2),
        ),
      ),
    );
  }
}

/// A `.field` select: box with a chevron, menu on click.
class Select<T> extends StatelessWidget {
  const Select({super.key, required this.value, required this.options, required this.onChanged});

  final T? value;
  final List<(T, String)> options;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final label = options.where((o) => o.$1 == value).firstOrNull?.$2 ?? '';
    return Builder(builder: (context) {
      return Pressable(
        outline: brand,
        radius: 8,
        onPressed: () async {
          final box = context.findRenderObject()! as RenderBox;
          final origin = box.localToGlobal(Offset.zero);
          final picked = await showMenu<T>(
            context: context,
            position: RelativeRect.fromLTRB(origin.dx, origin.dy + box.size.height, origin.dx + box.size.width, 0),
            constraints: BoxConstraints(minWidth: box.size.width, maxWidth: box.size.width),
            color: Colors.white,
            elevation: 4,
            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: const BorderSide(color: zinc200)),
            items: [
              for (final o in options)
                PopupMenuItem<T>(
                  value: o.$1,
                  height: 32,
                  child: Text(o.$2, style: inter(14, lh: 20, w: o.$1 == value ? FontWeight.w600 : FontWeight.w400)),
                ),
            ],
          );
          if (picked != null && picked != value) onChanged(picked);
        },
        child: Container(
          height: 40,
          // The native select adds 4 px inside the 12 px padding.
          padding: const EdgeInsets.only(left: 16, right: 10),
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: zinc200, width: hair),
            boxShadow: _shadowSm,
          ),
          child: Row(children: [
            Expanded(child: Text(label, style: inter(14, lh: 20))),
            svgIcon(_iconChevron, 16, zinc900),
          ]),
        ),
      );
    });
  }
}

class _DashedBorder extends CustomPainter {
  _DashedBorder({required this.color, required this.radius});

  final Color color;
  final double radius;

  @override
  void paint(Canvas canvas, Size size) {
    final path = Path()..addRRect(RRect.fromRectAndRadius((Offset.zero & size).deflate(0.5), Radius.circular(radius)));
    final paint = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = hair;
    final dashed = Path();
    for (final metric in path.computeMetrics()) {
      for (var d = 0.0; d < metric.length; d += 6) {
        dashed.addPath(metric.extractPath(d, d + 3), Offset.zero);
      }
    }
    canvas.drawPath(dashed, paint);
  }

  @override
  bool shouldRepaint(_DashedBorder old) => old.color != color;
}
