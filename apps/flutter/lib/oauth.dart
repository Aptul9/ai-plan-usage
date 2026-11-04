// The app's own sign-ins: Claude (PKCE, code pasted back from the callback
// page) and Codex (PKCE, browser redirect to a localhost callback).
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:http/http.dart' as http;

import 'model.dart';

// Claude Code 2.1.302 values, read from claude.exe on 2026-10-30.
const claudeClientId = '9d1c250a-e61b-44d9-88ed-5944d1962f5e';
const _claudeAuthorize = 'https://claude.com/cai/oauth/authorize';
const _claudeRedirect = 'https://platform.claude.com/oauth/code/callback';
const claudeTokenUrl = 'https://platform.claude.com/v1/oauth/token';
// The usage endpoint needs user:profile. If the authorize page rejects it on
// its own, use "user:profile user:inference".
const _claudeScope = 'user:profile';
// Claude Code's own User-Agent. Anthropic answers any other one on the token
// endpoint with HTTP 429 "Rate limited", even for a request it would reject
// anyway (checked 2026-10-31: "claude-code/2.1.0" got 429, this got 400).
const claudeCodeUa = 'claude-cli/2.1.302 (external, cli)';

// OpenCode packages/core/src/plugin/provider/openai.ts, v2.0.32.
const codexClientId = 'app_EMoamEEZ73f0CkXaXp7hrann';
const _codexIssuer = 'https://auth.openai.com';
const appUa = 'ai-plan-usage';
const httpTimeout = Duration(seconds: 20);

/// `state` is its own random value: the verifier must never leave the app, and
/// the callback page echoes `state` back in the pasted code.
class Pkce {
  const Pkce({required this.verifier, required this.challenge, required this.state});

  final String verifier;
  final String challenge;
  final String state;
}

String _b64url(List<int> bytes) => base64Url.encode(bytes).replaceAll('=', '');

String challengeFor(String verifier) => _b64url(sha256.convert(ascii.encode(verifier)).bytes);

Pkce newPkce() {
  final rnd = Random.secure();
  List<int> random32() => List.generate(32, (_) => rnd.nextInt(256));
  final verifier = _b64url(random32());
  return Pkce(verifier: verifier, challenge: challengeFor(verifier), state: _b64url(random32()));
}

String claudeAuthorizeUrl(Pkce p) => Uri.parse(_claudeAuthorize).replace(queryParameters: {
      'code': 'true',
      'client_id': claudeClientId,
      'response_type': 'code',
      'redirect_uri': _claudeRedirect,
      'scope': _claudeScope,
      'code_challenge': p.challenge,
      'code_challenge_method': 'S256',
      'state': p.state,
    }).toString();

/// The callback page shows `code#state`; a bare code is accepted too, and an
/// empty state counts as absent.
(String, String?) splitCode(String input) {
  final t = input.trim();
  final i = t.indexOf('#');
  if (i < 0) return (t, null);
  final state = t.substring(i + 1);
  return (t.substring(0, i), state.isEmpty ? null : state);
}

/// The code to exchange, after checking the pasted state belongs to this sign-in.
String codeFor(String pasted, Pkce p) {
  final (code, state) = splitCode(pasted);
  if (code.isEmpty) throw const AppError('Paste the code shown after signing in.');
  if (state != null && state != p.state) {
    throw const AppError('This code belongs to another sign-in. Open the sign-in page again.');
  }
  return code;
}

/// `prevRefresh` is kept when the server does not rotate it.
OauthCreds credsFrom(Map<String, dynamic> body, String prevRefresh, String? accountId) => OauthCreds(
      access: body['access_token'] as String,
      refresh: body['refresh_token'] as String? ?? prevRefresh,
      expiresAt: DateTime.now().millisecondsSinceEpoch + ((body['expires_in'] as num?)?.toInt() ?? 3600) * 1000,
      accountId: accountId,
    );

/// An initial exchange has no earlier refresh token to fall back on.
OauthCreds requireRefresh(OauthCreds c) {
  if (c.refresh.isEmpty) throw const AppError('Sign-in returned no refresh token.');
  return c;
}

bool isOk(int status) => status >= 200 && status < 300;

String bodyText(http.Response r) => utf8.decode(r.bodyBytes, allowMalformed: true);

Future<http.Response> _send(Future<http.Response> request) async {
  try {
    return await request.timeout(httpTimeout);
  } catch (e) {
    throw AppError('network: $e');
  }
}

Map<String, dynamic> _tokenBody(http.Response r) {
  final text = bodyText(r);
  if (!isOk(r.statusCode)) {
    throw AppError('HTTP ${r.statusCode}: ${text.length > 300 ? text.substring(0, 300) : text}');
  }
  try {
    final j = jsonDecode(text) as Map<String, dynamic>;
    j['access_token'] as String;
    return j;
  } catch (e) {
    throw AppError('token parse: $e');
  }
}

Future<OauthCreds> claudeExchange(http.Client client, String pasted, Pkce p) async {
  final code = codeFor(pasted, p);
  final r = await _send(client.post(
    Uri.parse(claudeTokenUrl),
    headers: {'User-Agent': claudeCodeUa, 'Accept': 'application/json', 'Content-Type': 'application/json'},
    body: jsonEncode({
      'code': code,
      'state': p.state,
      'grant_type': 'authorization_code',
      'client_id': claudeClientId,
      'redirect_uri': _claudeRedirect,
      'code_verifier': p.verifier,
    }),
  ));
  return requireRefresh(credsFrom(_tokenBody(r), '', null));
}

/// Redirect for a callback server on `port`. Only these ports are registered
/// for the Codex client (Codex CLI `codex-rs/login/src/server.rs`).
const codexPorts = [1455, 1457];

String codexRedirect(int port) => 'http://127.0.0.1:$port/auth/callback';

String codexAuthorizeUrl(Pkce p, String redirect) => Uri.parse('$_codexIssuer/oauth/authorize').replace(queryParameters: {
      'response_type': 'code',
      'client_id': codexClientId,
      'redirect_uri': redirect,
      'scope': 'openid profile email offline_access',
      'code_challenge': p.challenge,
      'code_challenge_method': 'S256',
      'state': p.state,
      'id_token_add_organizations': 'true',
      'codex_cli_simplified_flow': 'true',
      'originator': 'codex_cli_rs',
    }).toString();

/// The code from a callback request. Null for any other path, such as the
/// browser asking for a favicon.
({String? code, String? error})? callbackCode(Uri uri, Pkce p) {
  if (uri.path != '/auth/callback') return null;
  final q = uri.queryParameters;
  final error = q['error'];
  if (error != null) return (code: null, error: 'Sign-in failed: ${q['error_description'] ?? error}');
  if (q['state'] != p.state) return (code: null, error: 'This answer belongs to another sign-in. Try again.');
  final code = q['code'];
  if (code == null || code.isEmpty) return (code: null, error: 'Sign-in returned no code.');
  return (code: code, error: null);
}

Future<OauthCreds> codexExchange(http.Client client, String code, String redirect, Pkce p) async {
  final r = await _send(client.post(
    Uri.parse('$_codexIssuer/oauth/token'),
    headers: {'User-Agent': appUa},
    body: {
      'grant_type': 'authorization_code',
      'code': code,
      'redirect_uri': redirect,
      'client_id': codexClientId,
      'code_verifier': p.verifier,
    },
  ));
  final tokens = _tokenBody(r);
  final idToken = tokens['id_token'] as String?;
  final account = (idToken == null ? null : jwtAccountId(idToken)) ?? jwtAccountId(tokens['access_token'] as String);
  return requireRefresh(credsFrom(tokens, '', account));
}

Future<OauthCreds> codexRefresh(http.Client client, OauthCreds creds) async {
  final r = await _send(client.post(
    Uri.parse('$_codexIssuer/oauth/token'),
    headers: {'User-Agent': appUa},
    body: {'grant_type': 'refresh_token', 'refresh_token': creds.refresh, 'client_id': codexClientId},
  ));
  final tokens = _tokenBody(r);
  final idToken = tokens['id_token'] as String?;
  final account = (idToken == null ? null : jwtAccountId(idToken)) ?? creds.accountId;
  return credsFrom(tokens, creds.refresh, account);
}

String? jwtAccountId(String token) {
  final parts = token.split('.');
  if (parts.length < 2) return null;
  try {
    final claims = jsonDecode(utf8.decode(base64Url.decode(base64Url.normalize(parts[1])))) as Map<String, dynamic>;
    final orgs = claims['organizations'];
    final id = claims['chatgpt_account_id'] ??
        (claims['https://api.openai.com/auth'] as Map<String, dynamic>?)?['chatgpt_account_id'] ??
        (orgs is List && orgs.isNotEmpty && orgs.first is Map ? orgs.first['id'] : null);
    return id is String ? id : null;
  } catch (_) {
    return null;
  }
}

/// Default browser, no shell in between (URLs carry `&`).
void openBrowser(String url) {
  Process.start('rundll32.exe', ['url.dll,FileProtocolHandler', url]).ignore();
}
