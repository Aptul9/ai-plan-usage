// Settings window: a second engine opened on demand. It talks to the main
// engine over one channel with the Tauri command names (tauri-shim.js);
// values travel as JSON text.
import 'dart:async';
import 'dart:convert';

import 'package:desktop_multi_window/desktop_multi_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'model.dart';

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
  final _updates = StreamController<Map<String, dynamic>>.broadcast();

  Stream<Map<String, dynamic>> get updates => _updates.stream;

  Future<void> init(WindowController window) async {
    await _channel.setMethodCallHandler((call) async {
      if (call.method == 'snapshot-updated') _updates.add(jsonDecode(call.arguments as String) as Map<String, dynamic>);
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
  await windowManager.setSize(const Size(520, 640));
  await windowManager.setResizable(false);
  await windowManager.setMinimizable(false);
  await windowManager.setMaximizable(false);
  await windowManager.center();
  runApp(MaterialApp(debugShowCheckedModeBanner: false, home: SettingsPage(api: api)));
  await windowManager.show();
  await windowManager.focus();
}

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key, required this.api});

  final Api api;

  @override
  Widget build(BuildContext context) => const Scaffold(body: Center(child: Text('Settings')));
}
