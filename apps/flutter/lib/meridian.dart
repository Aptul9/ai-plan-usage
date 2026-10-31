// Claude usage from a local Meridian (`GET /v1/usage/quota/all`). The app
// never reads or refreshes Meridian's own credential files.
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'model.dart';
import 'providers.dart';

class MeridianProfile {
  const MeridianProfile({required this.id, this.session, this.weekly, this.error});

  final String id;
  final UsageBlock? session;
  final UsageBlock? weekly;

  /// Meridian's `error`, kept only when the profile has no windows.
  final String? error;
}

UsageBlock? _block(List windows, String kind) {
  final w = windows.whereType<Map>().where((w) => w['type'] == kind).firstOrNull;
  if (w == null) return null;
  final u = w['utilization'];
  final r = w['resetsAt'];
  return UsageBlock(
    // utilization is 0..1 and can pass 1; one decimal avoids 22.000000000000004.
    usedPct: u is num ? (u * 1000).round() / 10 : null,
    resetsAt: r is int ? DateTime.fromMillisecondsSinceEpoch(r, isUtc: true).toIso8601String() : null,
  );
}

/// Claude profiles from a quota body; null when the body is not Meridian's.
List<MeridianProfile>? parseQuotaAll(Object? body) {
  final profiles = body is Map ? body['profiles'] : null;
  if (profiles is! List) return null;
  return [
    for (final p in profiles.whereType<Map>())
      if (p['id'] is String && p['type'] != 'chatgpt' && p['provider'] != 'chatgpt')
        () {
          final windows = p['windows'] is List ? p['windows'] as List : const [];
          final error = p['error'];
          return MeridianProfile(
            id: p['id'] as String,
            session: _block(windows, 'five_hour'),
            weekly: _block(windows, 'seven_day'),
            error: windows.isEmpty && error is String ? error : null,
          );
        }(),
  ];
}

/// Null when Meridian is unreachable or answered something else.
Future<List<MeridianProfile>?> fetchMeridian(http.Client client, MeridianConfig cfg) async {
  final url = '${cfg.url.replaceAll(RegExp(r'/+$'), '')}/v1/usage/quota/all';
  try {
    final key = cfg.apiKey ?? '';
    final r = await client.get(Uri.parse(url), headers: {
      'Accept': 'application/json',
      if (key.isNotEmpty) 'x-api-key': key,
    }).timeout(const Duration(seconds: 6));
    if (r.statusCode < 200 || r.statusCode >= 300) return null;
    return parseQuotaAll(jsonDecode(utf8.decode(r.bodyBytes)));
  } catch (_) {
    return null;
  }
}

/// The usage of one Meridian account. `profiles` is null when Meridian was
/// unreachable.
FetchResult meridianResultFor(List<MeridianProfile>? profiles, String profileId) {
  if (profiles == null) return FetchResult.err('Meridian offline', 'offline');
  final p = profiles.where((p) => p.id == profileId).firstOrNull;
  if (p == null) return FetchResult.err('Not in Meridian', 'upstream');
  return switch (p.error) {
    'rate_limited' => FetchResult.err('Meridian rate limited', 'rate-limited'),
    'no_token' || 'not_oauth' => FetchResult.err('Meridian profile not signed in', 'signed-out'),
    final String other => FetchResult.err('Meridian: $other', 'upstream'),
    null => FetchResult(session: p.session, weekly: p.weekly),
  };
}
