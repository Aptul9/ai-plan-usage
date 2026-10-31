import 'dart:async';
import 'dart:io';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/widgets.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:intl/intl.dart';
import 'package:path/path.dart' as p;
import 'package:tray_manager/tray_manager.dart';

import 'app.dart';
import 'popover.dart';
import 'settings_page.dart';
import 'settings_store.dart';
import 'tray.dart';

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  final locale = Intl.canonicalizedLocale(Platform.localeName);
  await initializeDateFormatting();
  Intl.defaultLocale = DateFormat.localeExists(locale) ? locale : 'en_US';

  final window = await WindowController.fromCurrentEngine();
  if (window.arguments == settingsArgs) return runSettings(window);

  final store = SettingsStore(defaultSettingsPath());
  final popover = PopoverWindow();
  if (!await _firstInstance(p.dirname(store.path), popover.toggle)) exit(0);

  final model = AppModel(store);
  final settings = SettingsHost(model);
  await settings.init();
  await popover.init();
  await Tray(
    model,
    onLeftClick: popover.toggle,
    onSettings: settings.open,
    onQuit: () async {
      await trayManager.destroy();
      _instanceLock?.closeSync();
      exit(0);
    },
  ).init();
  runApp(PopoverApp(model: model, window: popover, onOpenSettings: settings.open));
  model.start();
}

// The single-instance guard, held until Quit. It must be read somewhere: a
// release build drops a global nobody reads, and a collected file closes
// its handle, which releases the lock.
RandomAccessFile? _instanceLock;
ServerSocket? _ping;

/// False when another instance runs; that one gets a ping and toggles its
/// popover, as the Tauri single-instance plugin does.
Future<bool> _firstInstance(String dir, Future<void> Function() toggle) async {
  Directory(dir).createSync(recursive: true);
  final portFile = File(p.join(dir, 'instance.port'));
  final lock = File(p.join(dir, 'instance.lock')).openSync(mode: FileMode.append);
  try {
    lock.lockSync(FileLock.exclusive, 0, 1);
  } on FileSystemException {
    lock.closeSync();
    try {
      final s = await Socket.connect(InternetAddress.loopbackIPv4, int.parse(portFile.readAsStringSync().trim()));
      await s.close();
    } catch (_) {}
    return false;
  }
  _instanceLock = lock;
  _ping = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
  portFile.writeAsStringSync('${_ping!.port}');
  _ping!.listen((s) {
    s.destroy();
    unawaited(toggle());
  });
  return true;
}
