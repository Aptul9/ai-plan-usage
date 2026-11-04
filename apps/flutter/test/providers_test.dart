import 'dart:convert';

import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/providers.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

int now() => DateTime.now().millisecondsSinceEpoch;

const usageBody = '{"five_hour":{"utilization":16.0,"resets_at":"2026-06-21T14:50:00Z"},'
    '"seven_day":{"utilization":17.0,"resets_at":"2026-06-26T12:00:00Z"}}';

/// Answers token posts and usage gets in order, logging each request and each save.
class Script {
  Script(this.answers);

  final List<http.Response> answers;
  final log = <String>[];
  final requests = <http.Request>[];
  late final client = MockClient((r) async {
    log.add('${r.method} ${r.url.host}${r.url.path}');
    requests.add(r);
    return answers.removeAt(0);
  });

  void save(String id, OauthCreds c) => log.add('save $id ${c.access}');
}

void main() {
  setUp(claudeRetryAt.clear);

  test('refresh failure kinds', () {
    const cases = [
      ('network: timed out', 'offline', 'Codex unreachable. Retrying next poll.'),
      ('HTTP 400: invalid_grant', 'signed-out', 'Sign in again.'),
      ('HTTP 429: slow down', 'rate-limited', 'Codex token refresh is rate limited. Retrying later.'),
      ('HTTP 503: unavailable', 'upstream', 'Codex sign-in service error. Retrying next poll.'),
      ('refresh HTTP 502: bad gateway', 'upstream', 'Codex sign-in service error. Retrying next poll.'),
      ('refresh HTTP 401: x', 'signed-out', 'Sign in again.'),
    ];
    for (final (detail, kind, message) in cases) {
      final e = refreshFailed('Codex', 'Sign in again.', detail).error!;
      expect((e.kind, e.message), (kind, message), reason: detail);
      expect(e.detail, detail);
    }
  });

  test('token expiry with skew', () {
    expect(tokenExpired(1000, 1000), isTrue);
    expect(tokenExpired(1000 + expirySkewMs, 1000), isTrue);
    expect(tokenExpired(10000000000, 0), isFalse);
    expect(tokenExpired(null, 12345), isFalse);
  });

  test('Claude: expired token is refreshed and saved before the usage call', () async {
    final s = Script([
      http.Response(jsonEncode({'access_token': 'NEW', 'refresh_token': 'R2', 'expires_in': 3600}), 200),
      http.Response(usageBody, 200),
    ]);
    final r = await fetchClaude(s.client, 'c1', OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: now() - 1), s.save);
    expect(s.log, ['POST platform.claude.com/v1/oauth/token', 'save c1 NEW', 'GET api.anthropic.com/api/oauth/usage']);
    expect(r.error, isNull);
    expect(r.rotated?.refresh, 'R2');
    expect(r.session?.usedPct, 16.0);
    expect(r.weekly?.resetsAt, '2026-06-26T12:00:00Z');
  });

  test('Claude: 401 after a refresh this round does not refresh twice', () async {
    final s = Script([
      http.Response(jsonEncode({'access_token': 'NEW', 'refresh_token': 'R2'}), 200),
      http.Response('', 401),
    ]);
    final r = await fetchClaude(s.client, 'c1', OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: now() - 1), s.save);
    expect(s.log.where((l) => l.startsWith('POST')).length, 1);
    expect(r.error?.kind, 'signed-out');
    expect(r.error?.message, 'Claude sign-in expired. Sign in again in Settings.');
    expect(r.rotated?.access, 'NEW', reason: 'spent refresh token must still be replaced');
  });

  test('Claude: 401 with a fresh token refreshes once and retries', () async {
    final s = Script([
      http.Response('', 401),
      http.Response(jsonEncode({'access_token': 'NEW', 'refresh_token': 'R2'}), 200),
      http.Response(usageBody, 200),
    ]);
    final r = await fetchClaude(s.client, 'c1', OauthCreds(access: 'OK', refresh: 'R1', expiresAt: now() + 3600000), s.save);
    expect(s.log, [
      'GET api.anthropic.com/api/oauth/usage',
      'POST platform.claude.com/v1/oauth/token',
      'save c1 NEW',
      'GET api.anthropic.com/api/oauth/usage',
    ]);
    expect(r.session?.usedPct, 16.0);
  });

  test('Claude: refresh rejected means signed out, with the refresh detail', () async {
    final s = Script([http.Response('{"error":"invalid_grant"}', 400)]);
    final r = await fetchClaude(s.client, 'c1', OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: now() - 1), s.save);
    expect(r.error?.kind, 'signed-out');
    expect(r.error?.detail, 'refresh HTTP 400: {"error":"invalid_grant"}');
    expect(r.rotated, isNull);
  });

  test('Claude: 429 sets a per-account retry gate', () async {
    final s = Script([http.Response('{"error":{"type":"rate_limit_error","message":"Rate limited"}}', 429, headers: {'retry-after': '120'})]);
    final creds = OauthCreds(access: 'OK', refresh: 'R1', expiresAt: now() + 3600000);
    final r = await fetchClaude(s.client, 'c1', creds, s.save);
    expect(r.error?.kind, 'rate-limited');
    expect(r.error?.message, startsWith('Claude usage API is rate limited. Try again in '));
    expect(r.error?.detail, startsWith('HTTP 429\nrate_limit_error: Rate limited\nRetry-After: '));
    expect(currentRetryAt('c1'), isNotNull);
    expect(currentRetryAt('c2'), isNull);
    final again = await fetchClaude(s.client, 'c1', creds, s.save);
    expect(again.error?.message, startsWith('Claude usage API is rate limited. Retrying in '));
    expect(s.log.length, 1, reason: 'gated call makes no request');
  });

  test('Codex: expired token is refreshed and saved before the usage call', () async {
    final s = Script([
      http.Response(jsonEncode({'access_token': 'NEW', 'refresh_token': 'R2'}), 200),
      http.Response(
          jsonEncode({
            'plan_type': 'plus',
            'rate_limit': {
              'primary_window': {'used_percent': 12, 'reset_at': 1793131200},
              'secondary_window': {'used_percent': 40.5, 'reset_at': 1793563200},
            },
          }),
          200),
    ]);
    final r = await fetchCodex(s.client, 'x1', OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: now(), accountId: 'acc'), s.save);
    expect(s.log, ['POST auth.openai.com/oauth/token', 'save x1 NEW', 'GET chatgpt.com/backend-api/wham/usage']);
    final usage = s.requests.last;
    expect(usage.headers['ChatGPT-Account-Id'], 'acc');
    expect(usage.headers['Authorization'], 'Bearer NEW');
    expect(r.session?.usedPct, 12);
    expect(r.session?.resetsAt, '2026-10-27T20:00:00.000Z');
    expect(r.weekly?.usedPct, 40.5);
    expect(r.planType, 'plus');
  });

  test('Codex: refresh with no answer is offline', () async {
    final client = MockClient((_) async => throw http.ClientException('connection refused'));
    final r = await fetchCodex(client, 'x1', OauthCreds(access: 'OLD', refresh: 'R1', expiresAt: 0), (_, _) {});
    expect(r.error?.kind, 'offline');
    expect(r.error?.message, 'Codex unreachable. Retrying next poll.');
  });

  test('Copilot used count = entitlement - remaining + overage', () {
    Map<String, dynamic> body(Map<String, dynamic> premium) => {
          'quota_snapshots': {'premium_interactions': premium},
          'quota_reset_date_utc': '2026-11-01T00:00:00.000Z',
          'copilot_plan': 'business',
        };
    var w = copilotResult(body({'entitlement': 300, 'remaining': 177, 'percent_remaining': 59.0, 'overage_count': 0})).weekly!;
    expect((w.usedAbs, w.entitlement, w.overage, w.usedPct), (123, 300, 0, 41.0));
    w = copilotResult(body({'entitlement': 300, 'quota_remaining': 0.4, 'overage_count': 12})).weekly!;
    expect(w.usedAbs, 312);
    w = copilotResult(body({'entitlement': 300, 'overage_count': 5})).weekly!;
    expect(w.usedAbs, 305, reason: 'old payloads without remaining');
    final r = copilotResult(body({'entitlement': 300, 'remaining': 300}));
    expect(r.planType, 'business');
    expect(r.weekly?.resetsAt, '2026-11-01T00:00:00.000Z');
    expect(r.session?.usedPct, isNull);
    expect(copilotResult({'quota_reset_date': '2026-11-01'}).weekly?.resetsAt, '2026-11-01T00:00:00Z');
  });

  test('Copilot: rejected token is signed out', () async {
    final r = await fetchCopilot(MockClient((_) async => http.Response('', 403)), 't');
    expect((r.error?.kind, r.error?.message), ('signed-out', 'GitHub token rejected. Needs Copilot access.'));
  });

  test('fetchAll skips off and Meridian accounts and asks no off Meridian', () async {
    final asked = <String>[];
    final client = MockClient((r) async {
      asked.add(r.url.host);
      return http.Response('{}', 200);
    });
    final accounts = [
      Account.create(Provider.copilot, Source.token, 'on')..token = 't',
      Account.create(Provider.copilot, Source.token, 'off')
        ..token = 't2'
        ..enabled = false,
      Account.meridian('m'),
      Account.create(Provider.claude, Source.oauth, 'no creds'),
    ];
    final out = await fetchAll(client, accounts, MeridianConfig(enabled: false), (_, _) {});
    expect(asked, ['api.github.com']);
    expect(out.meridian, isNull);
    expect(out.results.map((e) => e.$1), [accounts[0].id, accounts[3].id]);
    expect(out.results[1].$2.error?.message, 'Not signed in. Open Settings.');
  });

  test('a successful result clears the error and keeps missing blocks', () {
    final snap = ProviderSnapshot(weekly: const UsageBlock(usedPct: 5), error: const SnapshotError(message: 'x', kind: 'offline'));
    FetchResult(session: const UsageBlock(usedPct: 1)).applyTo(snap);
    expect((snap.session.usedPct, snap.weekly.usedPct, snap.error), (1, 5, null));
    FetchResult.err('down', 'offline').applyTo(snap);
    expect((snap.session.usedPct, snap.error?.message), (1, 'down'));
  });
}
