// Accounts, usage snapshots and the app state. Credentials live here and
// never reach the settings window: it gets publicJson() only.

enum Provider { claude, codex, copilot }

enum Source { meridian, oauth, token }

const meridianPrefix = 'meridian:';
const defaultMeridianUrl = 'http://127.0.0.1:3456';

const pollIntervals = [
  (label: '30 seconds (dev)', ms: 30000),
  (label: '1 minute', ms: 60000),
  (label: '5 minutes', ms: 5 * 60000),
  (label: '15 minutes', ms: 15 * 60000),
  (label: '30 minutes', ms: 30 * 60000),
];

const iconStyles = ['solid', 'number', 'ring', 'ring+number', 'bar'];

String nowIso() => DateTime.now().toUtc().toIso8601String();

class OauthCreds {
  const OauthCreds({required this.access, required this.refresh, required this.expiresAt, this.accountId});

  final String access;
  final String refresh;

  /// Epoch milliseconds.
  final int expiresAt;

  /// ChatGPT account id, Codex only.
  final String? accountId;

  factory OauthCreds.fromJson(Map<String, dynamic> j) => OauthCreds(
        access: j['access'] as String,
        refresh: j['refresh'] as String,
        expiresAt: (j['expiresAt'] as num).toInt(),
        accountId: j['accountId'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'access': access,
        'refresh': refresh,
        'expiresAt': expiresAt,
        if (accountId != null) 'accountId': accountId,
      };
}

class Account {
  Account({
    required this.id,
    required this.provider,
    required this.source,
    required this.label,
    this.enabled = true,
    this.oauth,
    this.token,
  });

  factory Account.create(Provider provider, Source source, String label) => Account(
        id: '${provider.name}-${DateTime.now().millisecondsSinceEpoch}',
        provider: provider,
        source: source,
        label: label.trim(),
      );

  factory Account.meridian(String profileId) => Account(
        id: '$meridianPrefix$profileId',
        provider: Provider.claude,
        source: Source.meridian,
        label: profileId,
      );

  /// Also reads the public view, which has no credentials.
  factory Account.fromJson(Map<String, dynamic> j) => Account(
        id: j['id'] as String,
        provider: Provider.values.byName(j['provider'] as String),
        source: Source.values.byName(j['source'] as String),
        label: j['label'] as String,
        enabled: j['enabled'] as bool,
        oauth: j['oauth'] == null ? null : OauthCreds.fromJson(j['oauth'] as Map<String, dynamic>),
        token: j['token'] as String?,
      );

  final String id;
  final Provider provider;
  final Source source;
  String label;
  bool enabled;
  OauthCreds? oauth;
  String? token;

  String? get profileId => id.startsWith(meridianPrefix) ? id.substring(meridianPrefix.length) : null;

  /// What the settings window may see.
  Map<String, dynamic> publicJson() => {
        'id': id,
        'provider': provider.name,
        'source': source.name,
        'label': label,
        'enabled': enabled,
      };

  Map<String, dynamic> toJson() => {
        ...publicJson(),
        if (oauth != null) 'oauth': oauth!.toJson(),
        if (token != null) 'token': token,
      };
}

/// Appends an enabled account for each Meridian profile not seen before.
/// Returns true when something was added.
bool mergeMeridian(List<Account> accounts, Iterable<String> profileIds) {
  var added = false;
  for (final pid in profileIds) {
    if (!accounts.any((a) => a.profileId == pid)) {
      accounts.add(Account.meridian(pid));
      added = true;
    }
  }
  return added;
}

/// The primary when it still exists and is enabled, else the first enabled
/// account, else an empty id.
String normalizePrimary(List<Account> accounts, String primary) {
  if (accounts.any((a) => a.id == primary && a.enabled)) return primary;
  for (final a in accounts) {
    if (a.enabled) return a.id;
  }
  return '';
}

class MeridianConfig {
  MeridianConfig({this.url = defaultMeridianUrl, this.apiKey, this.enabled = true, List<String>? removed})
      : removed = removed ?? [];

  /// Empty turns Meridian off.
  String url;
  String? apiKey;

  /// Off: Meridian is not asked, and its accounts are dropped.
  bool enabled;

  /// Profiles the user removed; not added back while Meridian stays on.
  final List<String> removed;

  factory MeridianConfig.fromJson(Map<String, dynamic> j) => MeridianConfig(
        url: j['url'] as String,
        apiKey: j['apiKey'] as String?,
        enabled: j['enabled'] as bool? ?? true,
        removed: (j['removed'] as List?)?.cast<String>().toList(),
      );

  Map<String, dynamic> toJson() => {
        'url': url,
        if (apiKey != null) 'apiKey': apiKey,
        'enabled': enabled,
        if (removed.isNotEmpty) 'removed': removed,
      };
}

class UsageBlock {
  const UsageBlock({this.usedPct, this.resetsAt, this.usedAbs, this.entitlement, this.overage});

  final double? usedPct;
  final String? resetsAt;
  final int? usedAbs;
  final int? entitlement;
  final int? overage;

  UsageBlock withPct(double? pct) =>
      UsageBlock(usedPct: pct, resetsAt: resetsAt, usedAbs: usedAbs, entitlement: entitlement, overage: overage);

  factory UsageBlock.fromJson(Map<String, dynamic>? j) => j == null
      ? const UsageBlock()
      : UsageBlock(
          usedPct: (j['usedPct'] as num?)?.toDouble(),
          resetsAt: j['resetsAt'] as String?,
          usedAbs: (j['usedAbs'] as num?)?.toInt(),
          entitlement: (j['entitlement'] as num?)?.toInt(),
          overage: (j['overage'] as num?)?.toInt(),
        );

  Map<String, dynamic> toJson() => {
        'usedPct': usedPct,
        'resetsAt': resetsAt,
        if (usedAbs != null) 'usedAbs': usedAbs,
        if (entitlement != null) 'entitlement': entitlement,
        if (overage != null) 'overage': overage,
      };
}

class SnapshotError {
  const SnapshotError({required this.message, required this.kind, this.detail, this.occurredAt, this.retryAt});

  final String message;
  final String kind;
  final String? detail;
  final String? occurredAt;
  final String? retryAt;

  factory SnapshotError.fromJson(Map<String, dynamic> j) => SnapshotError(
        message: j['message'] as String,
        kind: j['kind'] as String,
        detail: j['detail'] as String?,
        occurredAt: j['occurredAt'] as String?,
        retryAt: j['retryAt'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'message': message,
        'kind': kind,
        if (detail != null) 'detail': detail,
        if (occurredAt != null) 'occurredAt': occurredAt,
        if (retryAt != null) 'retryAt': retryAt,
      };
}

class ProviderSnapshot {
  ProviderSnapshot({this.session = const UsageBlock(), this.weekly = const UsageBlock(), this.planType, this.error});

  UsageBlock session;
  UsageBlock weekly;
  String? planType;
  SnapshotError? error;

  factory ProviderSnapshot.fromJson(Map<String, dynamic> j) => ProviderSnapshot(
        session: UsageBlock.fromJson(j['session'] as Map<String, dynamic>?),
        weekly: UsageBlock.fromJson(j['weekly'] as Map<String, dynamic>?),
        planType: j['planType'] as String?,
        error: j['error'] == null ? null : SnapshotError.fromJson(j['error'] as Map<String, dynamic>),
      );

  Map<String, dynamic> toJson() => {
        'session': session.toJson(),
        'weekly': weekly.toJson(),
        if (planType != null) 'planType': planType,
        'error': error?.toJson(),
      };
}

class MeridianStatus {
  const MeridianStatus({this.reachable = false, this.profiles = 0, this.checkedAt});

  final bool reachable;
  final int profiles;
  final String? checkedAt;

  Map<String, dynamic> toJson() => {'reachable': reachable, 'profiles': profiles, 'checkedAt': checkedAt};
}

class AppState {
  String fetchedAt = nowIso();
  String iconStyle = 'ring+number';

  /// Mirror of the primary account, read by the tray icon and tooltip.
  UsageBlock session = const UsageBlock();
  UsageBlock weekly = const UsageBlock();
  SnapshotError? error;
  List<Account> accounts = [];
  String primary = '';
  MeridianConfig meridian = MeridianConfig();
  MeridianStatus meridianStatus = const MeridianStatus();
  int pollIntervalMs = 5 * 60000;
  bool devMode = false;
  final Map<String, ProviderSnapshot> snapshots = {};

  /// Everything the settings window gets. Credentials and the Meridian key stay here.
  Map<String, dynamic> publicJson() => {
        'fetchedAt': fetchedAt,
        'iconStyle': iconStyle,
        'session': session.toJson(),
        'weekly': weekly.toJson(),
        'error': error?.toJson(),
        'accounts': [for (final a in accounts) a.publicJson()],
        'primary': primary,
        'pollIntervalMs': pollIntervalMs,
        'devMode': devMode,
        'snapshots': {for (final e in snapshots.entries) e.key: e.value.toJson()},
        'meridian': {
          'url': meridian.url,
          'hasApiKey': (meridian.apiKey ?? '').isNotEmpty,
          'enabled': meridian.enabled,
        },
        'meridianStatus': meridianStatus.toJson(),
      };

  void mirrorPrimary() {
    final p = snapshots[primary];
    session = p?.session ?? const UsageBlock();
    weekly = p?.weekly ?? const UsageBlock();
    error = p?.error;
  }

  Provider? get primaryProvider {
    for (final a in accounts) {
      if (a.id == primary) return a.provider;
    }
    return null;
  }

  /// One snapshot per account, none for removed ones.
  void ensureSnapshots() {
    final ids = {for (final a in accounts) a.id};
    snapshots.removeWhere((id, _) => !ids.contains(id));
    for (final id in ids) {
      snapshots.putIfAbsent(id, ProviderSnapshot.new);
    }
  }
}
