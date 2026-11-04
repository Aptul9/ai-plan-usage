// Tray icon, tooltip and menu (tray.rs). tray_manager 0.5.3 loads Windows
// icons from an .ico file, so each render is written next to the settings.
import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:path/path.dart' as p;
import 'package:tray_manager/tray_manager.dart';

import 'app.dart';
import 'icon.dart';
import 'model.dart';

class Tray with TrayListener {
  Tray(this.model, {required this.onLeftClick, required this.onSettings, required this.onQuit});

  final AppModel model;
  final Future<void> Function() onLeftClick;
  final void Function() onSettings;
  final void Function() onQuit;

  late final _icoPath = p.join(p.dirname(model.store.path), 'tray.ico');
  var _menuKey = '';
  Future<void> _queue = Future.value();

  Future<void> init() async {
    trayManager.addListener(this);
    // tray_manager adds the icon with whatever its tooltip buffer holds,
    // uninitialized memory on the first add: clear it first.
    await trayManager.setToolTip('');
    final green = (await rootBundle.load('assets/icon-green.png')).buffer.asUint8List();
    await _setIcon(pngToIco(green, 32));
    await _setMenu();
    model.addListener(refresh);
    // The pace color depends on the clock, so the icon is redrawn even
    // without new data. Local render only.
    Timer.periodic(const Duration(seconds: 60), (_) => refresh());
  }

  Future<void> _setIcon(Uint8List ico) async {
    final f = File(_icoPath);
    f.parent.createSync(recursive: true);
    f.writeAsBytesSync(ico);
    await trayManager.setIcon(_icoPath);
  }

  String _pct(UsageBlock b) => b.usedPct == null ? '-' : '${b.usedPct!.round()}%';

  /// Icon, tooltip and, when dev mode or the style changed, the menu.
  /// Serialized: the .ico file is rewritten for every render.
  Future<void> refresh() => _queue = _queue.then((_) async {
        final s = model.state;
        await _setIcon(pngToIco(await renderIconPng(s.iconStyle, payloadFromState(s)), 64));
        await trayManager.setToolTip('Session ${_pct(s.session)} · Weekly ${_pct(s.weekly)}');
        await _setMenu();
      });

  Future<void> _setMenu() async {
    final s = model.state;
    final key = '${s.devMode}|${s.iconStyle}';
    if (key == _menuKey) return;
    _menuKey = key;
    await trayManager.setContextMenu(Menu(items: [
      MenuItem(key: 'refresh', label: 'Refresh now'),
      MenuItem(key: 'settings', label: 'Settings…'),
      if (s.devMode) ...[
        MenuItem.separator(),
        MenuItem.submenu(
          label: 'Dev: simulate state',
          submenu: Menu(items: [
            MenuItem(key: 'dev_low', label: 'Low (45 / 12)'),
            MenuItem(key: 'dev_mid', label: 'Mid (85 / 50)'),
            MenuItem(key: 'dev_high', label: 'High (97 / 78)'),
            MenuItem(key: 'dev_reset', label: 'Reset (2 / 3)'),
            MenuItem.separator(),
            MenuItem(key: 'dev_stale', label: 'Force stale (22m old)'),
            MenuItem(key: 'dev_error', label: 'Force error'),
            MenuItem(key: 'dev_clear_error', label: 'Clear error'),
          ]),
        ),
        MenuItem.submenu(
          label: 'Dev: icon style (now: ${s.iconStyle})',
          submenu: Menu(items: [
            for (final style in iconStyles) MenuItem.checkbox(key: 'icon_style:$style', label: style, checked: style == s.iconStyle),
          ]),
        ),
      ],
      MenuItem.separator(),
      MenuItem(key: 'quit', label: 'Quit'),
    ]));
  }

  @override
  void onTrayIconMouseDown() => unawaited(onLeftClick());

  @override
  void onTrayIconRightMouseDown() => unawaited(trayManager.popUpContextMenu());

  @override
  void onTrayMenuItemClick(MenuItem menuItem) {
    final key = menuItem.key ?? '';
    switch (key) {
      case 'refresh':
        unawaited(model.runOnce());
      case 'settings':
        onSettings();
      case 'quit':
        onQuit();
      case _ when key.startsWith('icon_style:'):
        unawaited(model.invoke('settings_set_icon_style', {'style': key.substring('icon_style:'.length)}));
      case _ when key.startsWith('dev_'):
        model.simulate(key);
    }
  }
}
