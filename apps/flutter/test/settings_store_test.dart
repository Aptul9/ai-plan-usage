import 'dart:convert';
import 'dart:io';

import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/settings_store.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path/path.dart' as p;

void main() {
  late Directory dir;
  late String path;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('aipu-settings-');
    path = p.join(dir.path, 'settings.json');
  });
  tearDown(() => dir.deleteSync(recursive: true));

  List<FileSystemEntity> copies() =>
      dir.listSync().where((e) => p.basename(e.path).startsWith('settings.unreadable-')).toList();

  test('v2 round trips', () {
    final acc = Account.meridian('notarify')..label = 'Notarify';
    final raw = jsonEncode(Persist(accounts: [acc], primary: 'meridian:notarify').toJson());
    expect(jsonEncode(Persist.parse(raw)!.toJson()), raw);
  });

  test('a Tauri v2 file loads and saves with the same keys', () {
    const tauri = {
      'settingsVersion': 2,
      'accounts': [
        {
          'id': 'claude-1',
          'provider': 'claude',
          'source': 'oauth',
          'label': 'Work',
          'enabled': true,
          'oauth': {'access': 'a', 'refresh': 'r', 'expiresAt': 9}
        },
        {'id': 'codex-2', 'provider': 'codex', 'source': 'oauth', 'label': 'P', 'enabled': false, 'oauth': {'access': 'a', 'refresh': 'r', 'expiresAt': 9, 'accountId': 'acc'}},
        {'id': 'copilot-3', 'provider': 'copilot', 'source': 'token', 'label': 'T', 'enabled': true, 'token': 't'},
      ],
      'primary': 'claude-1',
      'meridian': {'url': 'http://127.0.0.1:3456', 'enabled': false, 'removed': ['x']},
      'devMode': true,
      'iconStyle': 'bar',
      'pollIntervalMs': 900000,
    };
    File(path).writeAsStringSync(const JsonEncoder.withIndent('  ').convert(tauri));
    final store = SettingsStore(path);
    final s = store.load();
    expect(s.accounts.length, 3);
    expect(s.primary, 'claude-1');
    expect(s.iconStyle, 'bar');
    expect(s.pollIntervalMs, 900000);
    expect(s.snapshots.length, 3);
    store.persist(s);
    expect(jsonDecode(File(path).readAsStringSync()), tauri);
    expect(File('$path.tmp').existsSync(), isFalse);
  });

  test('unknown icon style and interval keep defaults; primary normalized', () {
    File(path).writeAsStringSync(jsonEncode({
      'settingsVersion': 2,
      'accounts': [Account.meridian('a').toJson()],
      'primary': 'gone',
      'iconStyle': 'huge',
      'pollIntervalMs': 7,
    }));
    final s = SettingsStore(path).load();
    expect(s.iconStyle, 'ring+number');
    expect(s.pollIntervalMs, 300000);
    expect(s.primary, 'meridian:a');
  });

  test('missing file loads defaults and saves nothing yet', () {
    final s = SettingsStore(path).load();
    expect(s.accounts, isEmpty);
    expect(File(path).existsSync(), isFalse);
  });

  test('unusable file is copied aside and kept', () {
    for (final raw in ['{not json', '{"settingsVersion":3,"accounts":[]}', '{"settingsVersion":1,"copilotToken":"x"}']) {
      for (final c in copies()) {
        c.deleteSync();
      }
      File(path).writeAsStringSync(raw);
      final store = SettingsStore(path);
      expect(store.load().accounts, isEmpty);
      expect(store.blocked, isFalse);
      expect(copies().length, 1, reason: raw);
      expect(File(copies().single.path).readAsStringSync(), raw);
      expect(File(path).readAsStringSync(), raw);
    }
  });

  test('unreadable file that cannot be copied blocks saving', () {
    // A directory named settings.json can be neither read nor copied.
    Directory(path).createSync();
    final store = SettingsStore(path);
    store.load();
    expect(store.blocked, isTrue);
    store.persist(AppState());
    expect(Directory(path).existsSync(), isTrue);
  });

  test('persist replaces the previous file', () {
    final store = SettingsStore(path);
    final s = AppState()..devMode = true;
    store.persist(s);
    s.devMode = false;
    store.persist(s);
    expect(jsonDecode(File(path).readAsStringSync())['devMode'], isFalse);
  });
}
