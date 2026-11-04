// Usage fetchers for Claude, Codex and Copilot accounts. Rotated tokens are
// handed to `save` before the new access token is used: the old refresh
// token is spent, so a quit later in the round must not lose the new pair.
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:http/http.dart' as http;

import 'meridian.dart';
import 'model.dart';
import 'oauth.dart';

typedef SaveRotated = void Function(String id, OauthCreds creds);

class FetchResult {
  FetchResult({this.session, this.weekly, this.planType, this.error, this.rotated});

  FetchResult.err(String message, String kind, {String? detail, String? retryAt})
      : this(error: SnapshotError(message: message, kind: kind, detail: detail, occurredAt: nowIso(), retryAt: retryAt));

  final UsageBlock? session;
  final UsageBlock? weekly;
  final String? planType;
  final SnapshotError? error;

  /// Tokens that replaced the account's own after a refresh. Saved even when
  /// the usage call afterwards failed: refresh tokens are single use.
  OauthCreds? rotated;

  void applyTo(ProviderSnapshot snap) {
    if (error != null) {
      snap.error = error;
      return;
    }
    if (session != null) snap.session = session!;
    if (weekly != null) snap.weekly = weekly!;
    if (planType != null) snap.planType = planType;
    snap.error = null;
  }
}

/// A failed token refresh: offline when no answer came back, rate-limited on
/// 429, upstream on 5xx, otherwise the sign-in is gone. `detail` is
/// `network: ...`, `HTTP <code>: ...` (oauth.dart) or `refresh HTTP <code>: ...`
/// (Claude). Only `signed-out` uses the caller's sign-in-again text.
FetchResult refreshFailed(String provider, String signInAgain, String detail) {
  final code = RegExp(r'^(?:refresh )*HTTP (\d{3})').firstMatch(detail)?.group(1);
  final status = code == null ? null : int.parse(code);
  final (kind, message) = switch (status) {
    _ when detail.startsWith('network:') => ('offline', '$provider unreachable. Retrying next poll.'),
    429 => ('rate-limited', '$provider token refresh is rate limited. Retrying later.'),
    final s? when s >= 500 && s <= 599 => ('upstream', '$provider sign-in service error. Retrying next poll.'),
    _ => ('signed-out', signInAgain),
  };
  return FetchResult.err(message, kind, detail: detail);
}

String _iso(DateTime d) => d.toUtc().toIso8601String();

String truncate(String input, int maxChars) => input.length > maxChars ? '${input.substring(0, maxChars)}...' : input;

// ---- Claude ----

const _claudeUsageUrl = 'https://api.anthropic.com/api/oauth/usage';
const _oauthBeta = 'oauth-2025-04-20';
// Treat the token as expired slightly early so it is refreshed before the
// server would start rejecting it.
const expirySkewMs = 60000;
const _claudeSignInAgain = 'Claude sign-in expired. Sign in again in Settings.';

/// Anthropic's Retry-After after a 429, per account id.
final claudeRetryAt = <String, DateTime>{};

DateTime? currentRetryAt(String id) {
  final at = claudeRetryAt[id];
  if (at == null) return null;
  if (at.isAfter(DateTime.now())) return at;
  claudeRetryAt.remove(id);
  return null;
}

void setRetryAt(String id, DateTime? at) => at == null ? claudeRetryAt.remove(id) : claudeRetryAt[id] = at;

String formatDuration(Duration d) {
  final total = max(0, d.inSeconds);
  final h = total ~/ 3600, m = (total % 3600) ~/ 60, s = total % 60;
  if (h > 0) return '${h}h ${m}m';
  if (m > 0) return '${m}m ${s}s';
  return '${s}s';
}

DateTime? retryAtFromHeaders(Map<String, String> headers, DateTime now) {
  final value = headers['retry-after']?.trim();
  if (value == null) return null;
  final seconds = int.tryParse(value);
  if (seconds != null) return now.add(Duration(seconds: max(0, seconds)));
  try {
    return HttpDate.parse(value);
  } on FormatException {
    return null;
  }
}

String? _anthropicErrorSummary(String body) {
  try {
    final error = (jsonDecode(body) as Map<String, dynamic>)['error'];
    if (error is! Map) return null;
    final type = error['type'], message = error['message'];
    if (type is String && message is String) return '$type: $message';
    if (type is String) return type;
    if (message is String) return message;
  } catch (_) {}
  return null;
}

String _httpErrorDetail(int status, String body, DateTime? retryAt) {
  final parts = ['HTTP $status'];
  final summary = _anthropicErrorSummary(body);
  if (summary != null) {
    parts.add(summary);
  } else if (body.trim().isNotEmpty) {
    parts.add('Body: ${truncate(body.trim(), 600)}');
  }
  if (retryAt != null) parts.add('Retry-After: ${formatDuration(retryAt.difference(DateTime.now()))} (${_iso(retryAt)})');
  return parts.join('\n');
}

/// A missing expiry is treated as "try it" rather than dead.
bool tokenExpired(int? expiresAt, int now) => expiresAt != null && expiresAt <= now + expirySkewMs;

sealed class _Refresh {}

class _Refreshed extends _Refresh {
  _Refreshed(this.creds);
  final OauthCreds creds;
}

class _RateLimited extends _Refresh {
  _RateLimited(this.retryAt);
  final DateTime? retryAt;
}

class _Terminal extends _Refresh {
  _Terminal(this.detail);
  final String detail;
}

Future<_Refresh> _claudeRefresh(http.Client client, OauthCreds creds) async {
  if (creds.refresh.isEmpty) return _Terminal('no refresh token stored');
  final http.Response r;
  try {
    r = await client.post(
      Uri.parse(claudeTokenUrl),
      headers: {'Accept': 'application/json', 'User-Agent': claudeCodeUa},
      body: {'grant_type': 'refresh_token', 'refresh_token': creds.refresh, 'client_id': claudeClientId},
    ).timeout(httpTimeout);
  } catch (e) {
    return _Terminal('network: $e');
  }
  if (r.statusCode == 429) {
    return _RateLimited(retryAtFromHeaders(r.headers, DateTime.now()) ?? DateTime.now().add(const Duration(minutes: 5)));
  }
  final body = bodyText(r);
  if (!isOk(r.statusCode)) return _Terminal('refresh HTTP ${r.statusCode}: ${truncate(body.trim(), 200)}');
  try {
    return _Refreshed(credsFrom(jsonDecode(body) as Map<String, dynamic>, creds.refresh, null));
  } catch (e) {
    return _Terminal('refresh parse: $e');
  }
}

Future<http.Response> _claudeUsage(http.Client client, String token) => client.get(Uri.parse(_claudeUsageUrl), headers: {
      'Authorization': 'Bearer $token',
      'anthropic-beta': _oauthBeta,
      'User-Agent': claudeCodeUa,
      'Accept': 'application/json',
    }).timeout(httpTimeout);

// The retry gate is set by the caller before this runs.
FetchResult _rateLimitedResult(DateTime? retryAt, String source) {
  final wait = retryAt == null ? 'a few minutes' : formatDuration(retryAt.difference(DateTime.now()));
  return FetchResult.err(
    'Claude $source is rate limited. Retrying in $wait.',
    'rate-limited',
    detail: 'Anthropic returned HTTP 429 on the Claude $source; requests are paused until the backoff expires.',
    retryAt: retryAt == null ? null : _iso(retryAt),
  );
}

UsageBlock? _claudeWindow(Object? w) {
  if (w == null) return null;
  final m = w as Map<String, dynamic>;
  return UsageBlock(usedPct: (m['utilization'] as num?)?.toDouble(), resetsAt: m['resets_at'] as String?);
}

Future<FetchResult> fetchClaude(http.Client client, String id, OauthCreds creds, SaveRotated save) async {
  OauthCreds? rotated;
  FetchResult done(FetchResult r) => r..rotated = rotated;

  final gate = currentRetryAt(id);
  if (gate != null) {
    return FetchResult.err(
      'Claude usage API is rate limited. Retrying in ${formatDuration(gate.difference(DateTime.now()))}.',
      'rate-limited',
      detail: 'Anthropic previously returned HTTP 429; usage requests are paused until the Retry-After window expires.',
      retryAt: _iso(gate),
    );
  }

  // Refresh outcome handling shared by the pre-flight and the 401 retry; null
  // means the new token is in place.
  Future<FetchResult?> refresh() async {
    switch (await _claudeRefresh(client, creds)) {
      case _Refreshed(:final creds):
        save(id, creds);
        rotated = creds;
        setRetryAt(id, null);
        return null;
      case _RateLimited(:final retryAt):
        setRetryAt(id, retryAt);
        return done(_rateLimitedResult(retryAt, 'token refresh endpoint'));
      case _Terminal(:final detail):
        return done(refreshFailed('Claude', _claudeSignInAgain, detail));
    }
  }

  // Pre-flight expiry guard. Sending a known-dead token only trips Anthropic's
  // edge rate limiter (HTTP 429), which masks the underlying 401.
  if (tokenExpired(creds.expiresAt, DateTime.now().millisecondsSinceEpoch)) {
    final failed = await refresh();
    if (failed != null) return failed;
  }

  http.Response resp;
  try {
    resp = await _claudeUsage(client, (rotated ?? creds).access);
  } catch (e) {
    return done(FetchResult.err('Claude network: $e', 'offline'));
  }

  // One refresh on 401, unless this round already refreshed: the old refresh
  // token is spent by then.
  if (resp.statusCode == 401 && rotated == null) {
    final failed = await refresh();
    if (failed != null) return failed;
    try {
      resp = await _claudeUsage(client, rotated!.access);
    } catch (e) {
      return done(FetchResult.err('Claude network: $e', 'offline'));
    }
  }

  if (resp.statusCode == 401) {
    setRetryAt(id, null);
    return done(FetchResult.err(_claudeSignInAgain, 'signed-out'));
  }
  final body = bodyText(resp);
  if (!isOk(resp.statusCode)) {
    final now = DateTime.now();
    final retryAt = resp.statusCode == 429 ? retryAtFromHeaders(resp.headers, now) ?? now.add(const Duration(minutes: 5)) : null;
    final detail = _httpErrorDetail(resp.statusCode, body, retryAt);
    if (retryAt != null) {
      setRetryAt(id, retryAt);
      return done(FetchResult.err(
        'Claude usage API is rate limited. Try again in ${formatDuration(retryAt.difference(now))}.',
        'rate-limited',
        detail: detail,
        retryAt: _iso(retryAt),
      ));
    }
    return done(FetchResult.err('Claude HTTP ${resp.statusCode}', 'upstream', detail: detail));
  }
  setRetryAt(id, null);
  try {
    final data = jsonDecode(body) as Map<String, dynamic>;
    return done(FetchResult(session: _claudeWindow(data['five_hour']), weekly: _claudeWindow(data['seven_day'])));
  } catch (e) {
    return done(FetchResult.err('Claude parse: $e', 'upstream', detail: 'Body: ${truncate(body.trim(), 600)}'));
  }
}

// ---- Codex ----

// OpenCode refreshes when less than 5 minutes remain.
const _codexRefreshSkewMs = 5 * 60000;
const _codexSignInAgain = 'Codex sign-in expired. Sign in again in Settings.';

Future<http.Response> _codexUsage(http.Client client, OauthCreds creds) =>
    client.get(Uri.parse('https://chatgpt.com/backend-api/wham/usage'), headers: {
      'Authorization': 'Bearer ${creds.access}',
      'User-Agent': appUa,
      'Accept': 'application/json',
      if (creds.accountId != null) 'ChatGPT-Account-Id': creds.accountId!,
    }).timeout(httpTimeout);

UsageBlock? _codexWindow(Object? w) {
  if (w == null) return null;
  final m = w as Map<String, dynamic>;
  final reset = (m['reset_at'] as num?)?.toInt();
  return UsageBlock(
    usedPct: (m['used_percent'] as num?)?.toDouble(),
    resetsAt: reset == null ? null : _iso(DateTime.fromMillisecondsSinceEpoch(reset * 1000, isUtc: true)),
  );
}

Future<FetchResult> fetchCodex(http.Client client, String id, OauthCreds creds, SaveRotated save) async {
  OauthCreds? rotated;
  FetchResult done(FetchResult r) => r..rotated = rotated;

  Future<FetchResult?> refresh(OauthCreds from) async {
    try {
      rotated = await codexRefresh(client, from);
    } on AppError catch (e) {
      return done(refreshFailed('Codex', _codexSignInAgain, e.message));
    }
    save(id, rotated!);
    return null;
  }

  if (creds.expiresAt - DateTime.now().millisecondsSinceEpoch < _codexRefreshSkewMs) {
    final failed = await refresh(creds);
    if (failed != null) return failed;
  }
  http.Response resp;
  try {
    resp = await _codexUsage(client, rotated ?? creds);
  } catch (e) {
    return done(FetchResult.err('Codex network: $e', 'offline'));
  }
  // One refresh on 401, unless this round already refreshed.
  if (resp.statusCode == 401 && rotated == null) {
    final failed = await refresh(creds);
    if (failed != null) return failed;
    try {
      resp = await _codexUsage(client, rotated!);
    } catch (e) {
      return done(FetchResult.err('Codex network: $e', 'offline'));
    }
  }
  if (resp.statusCode == 401) return done(FetchResult.err(_codexSignInAgain, 'signed-out'));
  if (!isOk(resp.statusCode)) return done(FetchResult.err('Codex HTTP ${resp.statusCode}', 'upstream'));
  try {
    final data = jsonDecode(bodyText(resp)) as Map<String, dynamic>;
    final limits = data['rate_limit'] as Map<String, dynamic>?;
    return done(FetchResult(
      session: _codexWindow(limits?['primary_window']),
      weekly: _codexWindow(limits?['secondary_window']),
      planType: data['plan_type'] as String?,
    ));
  } catch (e) {
    return done(FetchResult.err('Codex parse: $e', 'upstream'));
  }
}

// ---- Copilot ----

double? _readPct(Object? q) {
  if (q is! Map) return null;
  final pr = q['percent_remaining'] ?? q['percentRemaining'];
  if (pr is num) return (100 - pr).clamp(0, 100).toDouble();
  final used = q['used_percent'] ?? q['usedPercent'];
  return used is num ? used.toDouble() : null;
}

/// The usage of a `copilot_internal/user` body.
FetchResult copilotResult(Map<String, dynamic> data) {
  final snaps = data['quota_snapshots'] ?? data['quotaSnapshots'];
  final premium = snaps is Map ? (snaps['premium_interactions'] ?? snaps['premiumInteractions']) : null;
  final q = premium is Map ? premium : const {};
  final resetDate = data['quota_reset_date'];
  final monthlyReset = data['quota_reset_date_utc'] as String? ?? (resetDate is String ? '${resetDate}T00:00:00Z' : null);
  final entitlement = (q['entitlement'] as num?)?.toInt();
  final overage = (q['overage_count'] as num?)?.toInt() ?? 0;
  final remainingAbs = q['quota_remaining'] ?? q['quotaRemaining'];
  final remaining = (q['remaining'] as num?)?.toInt() ?? (remainingAbs is num ? remainingAbs.round() : null);
  // Consumed = (entitlement - remaining) + overage. Falls back to the old
  // entitlement + overage when `remaining` is absent (pre-2026-06 payloads).
  final usedAbs = entitlement == null ? null : (remaining == null ? entitlement + overage : max(0, entitlement - remaining) + overage);
  return FetchResult(
    session: const UsageBlock(),
    weekly: UsageBlock(usedPct: _readPct(premium), resetsAt: monthlyReset, usedAbs: usedAbs, entitlement: entitlement, overage: overage),
    planType: data['copilot_plan'] as String? ?? data['copilotPlan'] as String?,
  );
}

Future<FetchResult> fetchCopilot(http.Client client, String token) async {
  final http.Response resp;
  try {
    resp = await client.get(Uri.parse('https://api.github.com/copilot_internal/user'), headers: {
      'Authorization': 'token $token',
      'Editor-Version': 'vscode/1.96.2',
      'Editor-Plugin-Version': 'copilot-chat/0.26.7',
      'User-Agent': 'GitHubCopilotChat/0.26.7',
      'X-Github-Api-Version': '2025-04-01',
      'Accept': 'application/json',
    }).timeout(httpTimeout);
  } catch (e) {
    return FetchResult.err('Copilot network: $e', 'offline');
  }
  if (resp.statusCode == 401 || resp.statusCode == 403) {
    return FetchResult.err('GitHub token rejected. Needs Copilot access.', 'signed-out');
  }
  if (!isOk(resp.statusCode)) return FetchResult.err('Copilot HTTP ${resp.statusCode}', 'upstream');
  try {
    return copilotResult(jsonDecode(bodyText(resp)) as Map<String, dynamic>);
  } catch (e) {
    return FetchResult.err('Copilot parse: $e', 'upstream');
  }
}

// ---- All accounts ----

class FetchOutput {
  FetchOutput(this.results, this.meridian);

  /// One entry per enabled account that is not a Meridian one.
  final List<(String, FetchResult)> results;

  /// Meridian's Claude profiles; null when it is off or unreachable.
  final List<MeridianProfile>? meridian;
}

Future<FetchResult> _fetchOne(http.Client client, Account a, SaveRotated save) {
  final o = a.oauth, t = a.token;
  return switch (a.provider) {
    Provider.claude when o != null => fetchClaude(client, a.id, o, save),
    Provider.codex when o != null => fetchCodex(client, a.id, o, save),
    Provider.copilot when t != null => fetchCopilot(client, t),
    _ => Future.value(FetchResult.err('Not signed in. Open Settings.', 'signed-out')),
  };
}

Future<FetchOutput> fetchAll(http.Client client, List<Account> accounts, MeridianConfig cfg, SaveRotated save) async {
  final own = Future.wait([
    for (final a in accounts)
      if (a.enabled && a.source != Source.meridian) _fetchOne(client, a, save).then((r) => (a.id, r)),
  ]);
  final meridian = !cfg.enabled || cfg.url.trim().isEmpty ? Future.value(null) : fetchMeridian(client, cfg);
  return FetchOutput(await own, await meridian);
}
