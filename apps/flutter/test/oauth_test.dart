import 'dart:convert';

import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/oauth.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

String jwt(Map<String, dynamic> claims) => 'h.${base64Url.encode(utf8.encode(jsonEncode(claims))).replaceAll('=', '')}.s';

void main() {
  const p = Pkce(verifier: 'v', challenge: 'c', state: 's');

  test('challenge matches the RFC 7636 vector', () {
    expect(challengeFor('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk'), 'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM');
  });

  test('verifier and state are 43 url-safe chars', () {
    final k = newPkce();
    expect(k.verifier.length, 43);
    expect(RegExp(r'^[A-Za-z0-9_-]+$').hasMatch(k.verifier), isTrue);
    expect(k.challenge, challengeFor(k.verifier));
    expect(k.state.length, 43);
    expect(k.state, isNot(k.verifier));
  });

  test('Claude authorize URL carries PKCE and state, never the verifier', () {
    final k = newPkce();
    final url = Uri.parse(claudeAuthorizeUrl(k));
    final q = url.queryParameters;
    expect(url.host, 'claude.com');
    expect(url.path, '/cai/oauth/authorize');
    expect(q['code_challenge'], k.challenge);
    expect(q['state'], k.state);
    expect(q['code_challenge_method'], 'S256');
    expect(q['scope'], 'user:profile');
    expect(q['client_id'], '9d1c250a-e61b-44d9-88ed-5944d1962f5e');
    expect(q['redirect_uri'], 'https://platform.claude.com/oauth/code/callback');
    expect(url.toString(), isNot(contains(k.verifier)));
  });

  test('pasted code splits; empty state counts as absent', () {
    expect(splitCode(' abc#xyz \n'), ('abc', 'xyz'));
    expect(splitCode('abc'), ('abc', null));
    expect(splitCode('abc#'), ('abc', null));
  });

  test('pasted state must belong to this sign-in', () {
    expect(codeFor('abc#s', p), 'abc');
    expect(codeFor('abc', p), 'abc');
    expect(codeFor('abc#', p), 'abc');
    expect(() => codeFor('abc#other', p),
        throwsA(isA<AppError>().having((e) => e.message, 'm', 'This code belongs to another sign-in. Open the sign-in page again.')));
    expect(() => codeFor('  ', p), throwsA(isA<AppError>().having((e) => e.message, 'm', 'Paste the code shown after signing in.')));
    expect(() => codeFor('#s', p), throwsA(isA<AppError>()));
  });

  test('Codex callback checks path, state and error', () {
    const k = Pkce(verifier: 'v', challenge: 'c', state: 's1');
    expect(callbackCode(Uri.parse('/favicon.ico'), k), isNull);
    expect(callbackCode(Uri.parse('/auth/callback?code=abc&state=s1'), k), (code: 'abc', error: null));
    expect(callbackCode(Uri.parse('/auth/callback?code=abc&state=other'), k)?.error, 'This answer belongs to another sign-in. Try again.');
    expect(callbackCode(Uri.parse('/auth/callback?error=access_denied&error_description=User%20cancelled&state=s1'), k)?.error,
        'Sign-in failed: User cancelled');
    expect(callbackCode(Uri.parse('/auth/callback?state=s1'), k)?.error, 'Sign-in returned no code.');
  });

  test('Codex authorize URL carries PKCE, redirect and the CLI flags', () {
    const k = Pkce(verifier: 'v', challenge: 'ch', state: 'st');
    final url = Uri.parse(codexAuthorizeUrl(k, codexRedirect(1455)));
    final q = url.queryParameters;
    expect(url.toString(), startsWith('https://auth.openai.com/oauth/authorize?'));
    expect(q['redirect_uri'], 'http://127.0.0.1:1455/auth/callback');
    expect(q['code_challenge'], 'ch');
    expect(q['state'], 'st');
    expect(q['client_id'], 'app_EMoamEEZ73f0CkXaXp7hrann');
    expect(q['scope'], 'openid profile email offline_access');
    expect(q['id_token_add_organizations'], 'true');
    expect(q['codex_cli_simplified_flow'], 'true');
    expect(q['originator'], 'codex_cli_rs');
  });

  test('account id comes from the JWT', () {
    expect(jwtAccountId(jwt({'https://api.openai.com/auth': {'chatgpt_account_id': 'acc-1'}})), 'acc-1');
    expect(jwtAccountId(jwt({'chatgpt_account_id': 'acc-2'})), 'acc-2');
    expect(jwtAccountId(jwt({'organizations': [{'id': 'org-1'}]})), 'org-1');
    expect(jwtAccountId('not-a-jwt'), isNull);
  });

  test('token response keeps the old refresh token unless rotated', () {
    final kept = credsFrom({'access_token': 'a', 'expires_in': 60}, 'old', null);
    expect(kept.access, 'a');
    expect(kept.refresh, 'old');
    expect(kept.expiresAt, greaterThan(DateTime.now().millisecondsSinceEpoch));
    expect(credsFrom({'access_token': 'a', 'refresh_token': 'new', 'expires_in': 60}, 'old', null).refresh, 'new');
  });

  test('initial exchange without a refresh token is rejected', () {
    expect(() => requireRefresh(credsFrom({'access_token': 'a'}, '', null)),
        throwsA(isA<AppError>().having((e) => e.message, 'm', 'Sign-in returned no refresh token.')));
    expect(requireRefresh(credsFrom({'access_token': 'a', 'refresh_token': 'r'}, '', null)).refresh, 'r');
  });

  test('Claude exchange posts JSON with the Claude Code User-Agent', () async {
    late http.Request sent;
    final client = MockClient((r) async {
      sent = r;
      return http.Response(jsonEncode({'access_token': 'A', 'refresh_token': 'R', 'expires_in': 3600}), 200);
    });
    final creds = await claudeExchange(client, 'thecode#s', p);
    expect(sent.url.toString(), 'https://platform.claude.com/v1/oauth/token');
    expect(sent.headers['User-Agent'], 'claude-cli/2.1.302 (external, cli)');
    final body = jsonDecode(sent.body) as Map<String, dynamic>;
    expect(body, containsPair('code', 'thecode'));
    expect(body, containsPair('state', 's'));
    expect(body, containsPair('code_verifier', 'v'));
    expect(body, containsPair('grant_type', 'authorization_code'));
    expect((creds.access, creds.refresh), ('A', 'R'));
  });

  test('token endpoint errors keep the status and the body head', () async {
    final client = MockClient((_) async => http.Response('{"error":"invalid_grant"}', 400));
    expect(() => claudeExchange(client, 'c', p),
        throwsA(isA<AppError>().having((e) => e.message, 'm', 'HTTP 400: {"error":"invalid_grant"}')));
  });

  test('Codex exchange is form encoded and reads the account id', () async {
    late http.Request sent;
    final client = MockClient((r) async {
      sent = r;
      return http.Response(
          jsonEncode({
            'access_token': 'A',
            'refresh_token': 'R',
            'id_token': jwt({'https://api.openai.com/auth': {'chatgpt_account_id': 'acc-9'}}),
          }),
          200);
    });
    final creds = await codexExchange(client, 'code1', codexRedirect(1457), p);
    expect(sent.headers['content-type'], startsWith('application/x-www-form-urlencoded'));
    expect(sent.bodyFields, {
      'grant_type': 'authorization_code',
      'code': 'code1',
      'redirect_uri': 'http://127.0.0.1:1457/auth/callback',
      'client_id': 'app_EMoamEEZ73f0CkXaXp7hrann',
      'code_verifier': 'v',
    });
    expect(creds.accountId, 'acc-9');
  });
}
