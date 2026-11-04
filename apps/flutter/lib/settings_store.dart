// Settings file: the Tauri v2 schema, in a folder of its own so the two apps
// never refresh the same tokens. Sync IO on purpose: two saves cannot
// interleave on the temp file.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;

import 'model.dart';

const settingsVersion = 2;

String defaultSettingsPath() =>
    p.join(Platform.environment['APPDATA'] ?? '.', 'ai-plan-usage-flutter', 'settings.json');

class Persist {
  Persist({List<Account>? accounts, this.primary = '', MeridianConfig? meridian, this.devMode = false, this.iconStyle, this.pollIntervalMs})
      : accounts = accounts ?? [],
        meridian = meridian ?? MeridianConfig();

  final List<Account> accounts;
  final String primary;
  final MeridianConfig meridian;
  final bool devMode;
  final String? iconStyle;
  final int? pollIntervalMs;

  factory Persist.of(AppState s) => Persist(
        accounts: s.accounts,
        primary: s.primary,
        meridian: s.meridian,
        devMode: s.devMode,
        iconStyle: s.iconStyle,
        pollIntervalMs: s.pollIntervalMs,
      );

  factory Persist.fromJson(Map<String, dynamic> j) => Persist(
        accounts: [for (final a in (j['accounts'] as List? ?? [])) Account.fromJson(a as Map<String, dynamic>)],
        primary: j['primary'] as String? ?? '',
        meridian: j['meridian'] == null ? null : MeridianConfig.fromJson(j['meridian'] as Map<String, dynamic>),
        devMode: j['devMode'] as bool? ?? false,
        iconStyle: j['iconStyle'] as String?,
        pollIntervalMs: (j['pollIntervalMs'] as num?)?.toInt(),
      );

  Map<String, dynamic> toJson() => {
        'settingsVersion': settingsVersion,
        'accounts': [for (final a in accounts) a.toJson()],
        'primary': primary,
        'meridian': meridian.toJson(),
        'devMode': devMode,
        'iconStyle': iconStyle,
        'pollIntervalMs': pollIntervalMs,
      };

  /// Only this build's version; anything else is copied aside by the caller.
  static Persist? parse(String raw) {
    try {
      final j = jsonDecode(raw);
      if (j is! Map<String, dynamic> || j['settingsVersion'] != settingsVersion) return null;
      return Persist.fromJson(j);
    } catch (_) {
      return null;
    }
  }
}

class SettingsStore {
  SettingsStore(this.path);

  final String path;

  /// Set when settings.json exists, could not be used, and copying it aside
  /// failed: saving would overwrite the only copy of its tokens.
  bool blocked = false;

  AppState load() {
    final state = AppState();
    final String raw;
    try {
      raw = File(path).readAsStringSync();
    } on PathNotFoundException {
      return state;
    } on FileSystemException catch (e) {
      _keepUnreadable('cannot read $path: ${e.message}');
      return state;
    }
    final persisted = Persist.parse(raw);
    if (persisted == null) {
      _keepUnreadable('$path is not usable by this build');
      return state;
    }
    state.accounts = persisted.accounts;
    state.meridian = persisted.meridian;
    state.devMode = persisted.devMode;
    if (iconStyles.contains(persisted.iconStyle)) state.iconStyle = persisted.iconStyle!;
    if (pollIntervals.any((x) => x.ms == persisted.pollIntervalMs)) state.pollIntervalMs = persisted.pollIntervalMs!;
    state.primary = normalizePrimary(state.accounts, persisted.primary);
    state.ensureSnapshots();
    return state;
  }

  void persist(AppState state) {
    if (blocked) {
      stderr.writeln('settings: not saving, the unusable settings file could not be copied aside');
      return;
    }
    final tmp = File('$path.tmp');
    try {
      Directory(p.dirname(path)).createSync(recursive: true);
      // Flushed to disk before the rename, so a crash cannot leave an empty settings.json.
      final f = tmp.openSync(mode: FileMode.write);
      try {
        f.writeStringSync(const JsonEncoder.withIndent('  ').convert(Persist.of(state).toJson()));
        f.flushSync();
      } finally {
        f.closeSync();
      }
      tmp.renameSync(path);
    } on FileSystemException catch (e) {
      stderr.writeln('settings: saving $path failed: ${e.message}');
    }
  }

  /// Copies a settings file this build cannot use next to it, so the first
  /// save of defaults does not destroy the only copy of its tokens. When the
  /// copy fails, saving stays off for this run.
  void _keepUnreadable(String why) {
    final backup = p.join(p.dirname(path), 'settings.unreadable-${DateTime.now().millisecondsSinceEpoch}.json');
    try {
      File(path).copySync(backup);
      stderr.writeln('settings: $why; kept a copy at $backup');
    } on FileSystemException catch (e) {
      blocked = true;
      stderr.writeln('settings: $why; copying it aside failed: ${e.message}; changes will not be saved');
    }
  }
}
