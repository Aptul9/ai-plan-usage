import 'dart:convert';

import 'package:ai_plan_usage/model.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('merge adds only new profiles', () {
    final list = [Account.meridian('notarify')..enabled = false];
    expect(mergeMeridian(list, ['notarify', 'everbright']), isTrue);
    expect(list.length, 2);
    expect(list[0].enabled, isFalse, reason: 'existing toggle kept');
    expect(list[1].id, 'meridian:everbright');
    expect(list[1].profileId, 'everbright');
    expect(mergeMeridian(list, ['everbright']), isFalse);
  });

  test('primary falls back to first enabled', () {
    final list = [Account.meridian('a')..enabled = false, Account.meridian('b')];
    expect(normalizePrimary(list, 'meridian:a'), 'meridian:b');
    expect(normalizePrimary(list, 'meridian:b'), 'meridian:b');
    expect(normalizePrimary([], 'x'), '');
  });

  test('public view has no credentials', () {
    final acc = Account.create(Provider.codex, Source.oauth, ' Work ')
      ..oauth = const OauthCreds(access: 'SECRET_A', refresh: 'SECRET_R', expiresAt: 1)
      ..token = 'SECRET_T';
    final text = jsonEncode(acc.publicJson());
    expect(text, isNot(contains('SECRET')));
    expect(text, contains('"provider":"codex"'));
    expect(text, contains('"label":"Work"'));
  });

  test('serializes with camelCase keys and round trips', () {
    final acc = Account.create(Provider.claude, Source.oauth, 'x')
      ..oauth = const OauthCreds(access: 'a', refresh: 'r', expiresAt: 5);
    final text = jsonEncode(acc.toJson());
    expect(text, contains('"expiresAt":5'));
    expect(text, contains('"source":"oauth"'));
    expect(text, isNot(contains('accountId')));
    final back = Account.fromJson(jsonDecode(text) as Map<String, dynamic>);
    expect(jsonEncode(back.toJson()), text);
  });

  test('public state hides secrets', () {
    final s = AppState();
    s.accounts.add(Account.create(Provider.claude, Source.oauth, 'Notarify')
      ..oauth = const OauthCreds(access: 'SECRET1', refresh: 'SECRET2', expiresAt: 0));
    s.meridian.apiKey = 'SECRET3';
    final text = jsonEncode(s.publicJson());
    expect(text, isNot(contains('SECRET')));
    expect(text, contains('"hasApiKey":true'));
  });

  test('snapshots follow accounts', () {
    final s = AppState();
    s.snapshots['gone'] = ProviderSnapshot();
    s.accounts.add(Account.meridian('a'));
    s.ensureSnapshots();
    expect(s.snapshots.keys.toList(), ['meridian:a']);
  });

  test('old meridian config without enabled stays on', () {
    expect(MeridianConfig.fromJson({'url': 'http://127.0.0.1:3456'}).enabled, isTrue);
  });
}
