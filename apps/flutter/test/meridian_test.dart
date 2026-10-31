import 'package:ai_plan_usage/meridian.dart';
import 'package:ai_plan_usage/model.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

Map<String, dynamic> body() => {
      'profiles': [
        {
          'id': 'notarify',
          'type': 'claude-max',
          'windows': [
            {'type': 'five_hour', 'utilization': 0.22, 'resetsAt': 1793131200000},
            {'type': 'seven_day', 'utilization': 1.04, 'resetsAt': 1793563200000},
          ],
          'error': null,
        },
        {'id': 'everbright', 'type': 'claude-max', 'windows': [], 'error': 'no_token'},
        {
          'id': 'gpt',
          'type': 'chatgpt',
          'windows': [
            {'type': 'five_hour', 'utilization': 0.5}
          ]
        },
      ],
      'activeProfile': 'notarify',
      'asOf': 1793120000000,
    };

void main() {
  test('parses Claude profiles and skips ChatGPT', () {
    final p = parseQuotaAll(body())!;
    expect(p.length, 2);
    expect(p[0].session?.usedPct, 22.0);
    expect(p[0].session?.resetsAt, startsWith('2026-'));
    expect(p[0].weekly?.usedPct, 104.0);
    expect(p[1].error, 'no_token');
  });

  test('rejects a body that is not Meridian', () {
    expect(parseQuotaAll({'error': 'x'}), isNull);
    expect(parseQuotaAll('nope'), isNull);
  });

  test('maps profile states', () {
    final p = parseQuotaAll(body());
    expect(meridianResultFor(p, 'notarify').error, isNull);
    expect(meridianResultFor(p, 'everbright').error?.kind, 'signed-out');
    expect(meridianResultFor(p, 'gone').error?.kind, 'upstream');
    expect(meridianResultFor(null, 'notarify').error?.kind, 'offline');
  });

  test('fetch sends the key only when set and trims the trailing slash', () async {
    late http.Request sent;
    final client = MockClient((r) async {
      sent = r;
      return http.Response('{"profiles":[]}', 200);
    });
    expect(await fetchMeridian(client, MeridianConfig(url: 'http://127.0.0.1:3456/', apiKey: 'k')), isEmpty);
    expect(sent.url.toString(), 'http://127.0.0.1:3456/v1/usage/quota/all');
    expect(sent.headers['x-api-key'], 'k');
    await fetchMeridian(client, MeridianConfig(apiKey: ''));
    expect(sent.headers.containsKey('x-api-key'), isFalse);
    expect(await fetchMeridian(MockClient((_) async => http.Response('', 500)), MeridianConfig()), isNull);
  });
}
