import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:ai_plan_usage/app.dart';
import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/settings_store.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:path/path.dart' as p;

void main() {
  late Directory dir;
  late SettingsStore store;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('aipu-app-');
    store = SettingsStore(p.join(dir.path, 'settings.json'));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  Map<String, dynamic> saved() => jsonDecode(File(store.path).readAsStringSync()) as Map<String, dynamic>;

  AppModel model(MockClientHandler handler, List<Account> accounts) {
    final s = AppState()
      ..accounts = accounts
      ..meridian.enabled = false;
    s.primary = normalizePrimary(s.accounts, '');
    store.persist(s);
    return AppModel(store, client: MockClient(handler));
  }

  test('a round saves rotated tokens even when the usage call fails', () async {
    final m = model((r) async {
      if (r.method == 'POST') return http.Response(jsonEncode({'access_token': 'NEW', 'refresh_token': 'R2'}), 200);
      return http.Response('boom', 500);
    }, [Account(id: 'c1', provider: Provider.claude, source: Source.oauth, label: 'W', oauth: const OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: 0))]);
    await m.runOnce();
    expect(m.state.snapshots['c1']!.error?.kind, 'upstream');
    expect(m.state.accounts.single.oauth?.refresh, 'R2');
    expect(saved()['accounts'][0]['oauth']['refresh'], 'R2');
    expect(m.state.error?.message, 'Claude HTTP 500', reason: 'primary mirrored');
  });

  test('a call during a round makes it run once more', () async {
    var requests = 0;
    final gate = Completer<void>();
    final m = model((r) async {
      requests++;
      if (requests == 1) await gate.future;
      return http.Response('{}', 200);
    }, [Account(id: 't1', provider: Provider.copilot, source: Source.token, label: 'T', token: 't')]);
    final first = m.runOnce();
    await Future<void>.delayed(Duration.zero);
    final second = m.runOnce();
    unawaited(m.runOnce());
    expect(identical(first, second), isTrue);
    gate.complete();
    await first;
    expect(requests, 2);
  });

  test('Meridian off drops its rows, on brings back all but removed ones', () async {
    final m = model((r) async {
      return http.Response(jsonEncode({
        'profiles': [
          {'id': 'a', 'windows': []},
          {'id': 'b', 'windows': []},
        ]
      }), 200);
    }, []);
    await m.invoke('meridian_set_enabled', {'on': true});
    await m.runOnce();
    expect(m.state.accounts.map((a) => a.id), ['meridian:a', 'meridian:b']);
    await m.invoke('account_remove', {'id': 'meridian:a'});
    await m.runOnce();
    expect(m.state.accounts.map((a) => a.id), ['meridian:b']);
    expect(m.state.meridian.removed, ['a']);
    await m.invoke('meridian_set_enabled', {'on': false});
    expect(m.state.accounts, isEmpty);
    await m.invoke('meridian_set_enabled', {'on': true});
    await m.runOnce();
    expect(m.state.accounts.map((a) => a.id), ['meridian:a', 'meridian:b']);
  });

  test('commands keep the Tauri rules', () async {
    final m = model((r) async => http.Response('{}', 200), [
      Account(id: 't1', provider: Provider.copilot, source: Source.token, label: 'One', token: 'tok'),
      Account(id: 't2', provider: Provider.copilot, source: Source.token, label: 'Two', token: 'tok2', enabled: false),
    ]);
    expect(() => m.invoke('account_rename', {'id': 't1', 'label': '  '}), throwsA(isA<AppError>()));
    await m.invoke('account_rename', {'id': 't1', 'label': ' Uno '});
    expect(m.state.accounts.first.label, 'Uno');
    expect(() => m.invoke('copilot_add_token', {'token': ' tok ', 'label': ''}),
        throwsA(isA<AppError>().having((e) => e.message, 'm', 'That token is already added')));
    expect(await m.invoke('settings_set_primary', {'id': 't2'}), 't1', reason: 'off account cannot be primary');
    await m.invoke('account_move', {'id': 't1', 'to': 5});
    expect(m.state.accounts.map((a) => a.id), ['t2', 't1']);
    expect(await m.invoke('settings_set_interval', {'ms': 7}), 300000);
    expect(await m.invoke('settings_set_interval', {'ms': 60000}), 60000);
    expect(await m.invoke('settings_set_icon_style', {'style': 'bar'}), 'bar');
    await m.invoke('account_set_enabled', {'id': 't1', 'on': false});
    expect(m.state.primary, '', reason: 'nothing enabled');
    final id = await m.invoke('copilot_add_token', {'token': 'new', 'label': ''}) as String;
    expect(m.state.accounts.last.label, 'Copilot');
    expect(m.state.primary, id);
    await m.invoke('meridian_set', {'url': ' http://h:1/ ', 'apiKey': 'k'});
    expect((m.state.meridian.url, m.state.meridian.apiKey), ('http://h:1', 'k'));
    await m.invoke('meridian_set', {'url': 'http://h:1', 'apiKey': null});
    expect(m.state.meridian.apiKey, 'k', reason: 'null keeps the key');
    await m.invoke('meridian_set', {'url': 'http://h:1', 'apiKey': ''});
    expect(m.state.meridian.apiKey, isNull);
    final pub = jsonEncode(await m.invoke('settings_get_all', {}));
    expect(pub, isNot(contains('"tok"')));
    expect(pub, contains('"iconStyles"'));
  });

  test('finishing a Claude sign-in needs a started one', () async {
    final m = model((r) async => http.Response('{}', 200), []);
    expect(() => m.invoke('claude_login_finish', {'code': 'x#y', 'label': ''}),
        throwsA(isA<AppError>().having((e) => e.message, 'm', 'Open the sign-in page first')));
  });

  test('the callback page escapes the error', () {
    expect(callbackPage('<b>&'), contains('&lt;b&gt;&amp;'));
    expect(callbackPage(null), contains('Codex is added to ai-plan-usage.'));
  });
}
