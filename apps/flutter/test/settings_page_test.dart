import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/settings_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, dynamic> bundle() => jsonDecode(jsonEncode({
      'fetchedAt': '2026-10-31T10:00:00Z',
      'iconStyle': 'ring+number',
      'pollIntervalMs': 300000,
      'devMode': false,
      'primary': 'claude-1',
      'accounts': [
        {'id': 'claude-1', 'provider': 'claude', 'source': 'oauth', 'label': 'Work', 'enabled': true},
        {'id': 'copilot-2', 'provider': 'copilot', 'source': 'token', 'label': 'BIP', 'enabled': true},
      ],
      'snapshots': {},
      'meridian': {'url': 'http://127.0.0.1:3456', 'hasApiKey': false, 'enabled': false},
      'meridianStatus': {'reachable': false, 'profiles': 0, 'checkedAt': null},
      'intervals': [
        {'label': '1 minute', 'ms': 60000},
        {'label': '5 minutes', 'ms': 300000},
      ],
      'iconStyles': ['solid', 'ring+number'],
    })) as Map<String, dynamic>;

class FakeApi extends Api {
  final calls = <String>[];
  final updatesCtl = StreamController<void>.broadcast();
  final failures = <String, String>{};

  @override
  Stream<void> get updates => updatesCtl.stream;

  @override
  Future<Object?> call(String method, [Map<String, Object?> args = const {}]) async {
    if (method != 'settings_get_all' && method != 'settings_get_autostart') calls.add('$method ${jsonEncode(args)}');
    final fail = failures[method];
    if (fail != null) throw AppError(fail);
    return switch (method) {
      'settings_get_all' => bundle(),
      'settings_get_autostart' => false,
      'settings_set_autostart' => args['on'],
      'settings_set_interval' => args['ms'],
      'settings_set_icon_style' => args['style'],
      'claude_login_start' => 'https://claude.com/cai/oauth/authorize?x=1',
      'copilot_import_list' => [
          {'id': 'cred_a', 'label': 'Tecnomat'}
        ],
      _ => null,
    };
  }
}

Future<FakeApi> pumpPage(WidgetTester tester) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(520, 1800);
  addTearDown(tester.view.reset);
  final api = FakeApi();
  await tester.pumpWidget(MaterialApp(home: SettingsPage(api: api)));
  await tester.pumpAndSettle();
  return api;
}

void main() {
  // The real font: the test font's square glyphs do not fit the buttons.
  setUpAll(() => (FontLoader('Inter')..addFont(Future.value(ByteData.sublistView(File('assets/fonts/Inter.ttf').readAsBytesSync())))).load());

  testWidgets('account row actions call the Tauri commands', (tester) async {
    final api = await pumpPage(tester);
    expect(find.text('Work'), findsOneWidget);
    expect(find.text('Copilot · Token'), findsOneWidget);

    await tester.tap(find.byTooltip('Use for the tray icon'));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'settings_set_primary {"id":"copilot-2"}');

    await tester.tap(find.byType(Toggle).first);
    await tester.pumpAndSettle();
    expect(api.calls.last, 'account_set_enabled {"id":"claude-1","on":false}');

    final removes = find.byWidgetPredicate((w) => w is IconBox && w.tooltip == null);
    await tester.tap(removes.last);
    await tester.pump();
    expect(find.text('Remove?'), findsOneWidget, reason: 'first click arms');
    expect(api.calls.where((c) => c.startsWith('account_remove')), isEmpty);
    await tester.tap(find.text('Remove?'));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'account_remove {"id":"copilot-2"}');
  });

  testWidgets('rename saves on Enter, keeps the old name on Escape', (tester) async {
    final api = await pumpPage(tester);
    await tester.tap(find.byTooltip('Rename').first);
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).first, ' Office ');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(api.calls.last, 'account_rename {"id":"claude-1","label":"Office"}');

    await tester.tap(find.byTooltip('Rename').last);
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).first, 'Nope');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(api.calls.where((c) => c.contains('Nope')), isEmpty);
    expect(find.text('BIP'), findsOneWidget);
  });

  testWidgets('Copilot import lists OpenCode logins and adds one', (tester) async {
    final api = await pumpPage(tester);
    await tester.tap(find.text('Copilot').last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Show OpenCode logins'));
    await tester.pumpAndSettle();
    expect(find.text('Tecnomat'), findsOneWidget);
    await tester.tap(find.text('Add').last);
    await tester.pumpAndSettle();
    expect(api.calls.last, 'copilot_import {"id":"cred_a","label":"Tecnomat"}');
    expect(find.text('Copilot · Tecnomat added.'), findsOneWidget);
  });

  testWidgets('Claude sign-in is two steps and errors show in the status line', (tester) async {
    final api = await pumpPage(tester);
    await tester.tap(find.text('Claude').first);
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).first, 'Work 2');
    await tester.tap(find.text('Open sign-in page'));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'claude_login_start {}');
    expect(find.text('Code from the last page'), findsOneWidget);
    api.failures['claude_login_finish'] = 'This code belongs to another sign-in. Open the sign-in page again.';
    await tester.enterText(find.byType(TextField).at(1), 'abc#other');
    await tester.tap(find.text('Add'));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'claude_login_finish {"code":"abc#other","label":"Work 2"}');
    expect(find.text('This code belongs to another sign-in. Open the sign-in page again.'), findsOneWidget);
  });

  testWidgets('switches and selects', (tester) async {
    final api = await pumpPage(tester);
    final toggles = find.byType(Toggle);
    // Account toggles come first, then Meridian, Start with Windows, Developer mode.
    await tester.tap(toggles.at(2));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'meridian_set_enabled {"on":true}');
    await tester.tap(toggles.at(3));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'settings_set_autostart {"on":true}');
    await tester.tap(toggles.at(4));
    await tester.pumpAndSettle();
    expect(api.calls.last, 'settings_set_dev_mode {"on":true}');

    await tester.tap(find.text('ring+number'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('solid').last);
    await tester.pumpAndSettle();
    expect(api.calls.last, 'settings_set_icon_style {"style":"solid"}');
    expect(find.text('solid'), findsOneWidget);
  });

  testWidgets('a pushed update reloads the page', (tester) async {
    final api = await pumpPage(tester);
    api.updatesCtl.add(null);
    await tester.pumpAndSettle();
    expect(find.text('Work'), findsOneWidget);
  });
}
