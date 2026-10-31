// Owner of the app state: polling, the settings window's commands, saves.
// Command names are the Tauri ones (commands.rs).
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import 'meridian.dart';
import 'model.dart';
import 'oauth.dart';
import 'opencode_import.dart';
import 'providers.dart';
import 'settings_store.dart';

// How long a browser sign-in waits for the redirect.
const loginTimeout = Duration(minutes: 10);

const _runKey = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Run';
const _runValue = 'ai-plan-usage-flutter';

class _CodexLogin {
  _CodexLogin(this.server);

  final HttpServer server;
  final done = Completer<String>();
}

class AppModel extends ChangeNotifier {
  AppModel(this.store, {http.Client? client})
      : client = client ?? http.Client(),
        state = store.load();

  final SettingsStore store;
  final http.Client client;
  final AppState state;

  // Sign-ins started in settings and not finished yet.
  Pkce? _claudePkce;
  _CodexLogin? _codex;

  Future<void>? _inFlight;
  bool _rerun = false;

  void start() {
    // First fetch after a small grace, then one per poll interval.
    Future.delayed(const Duration(seconds: 2), () async {
      while (true) {
        await runOnce();
        await Future<void>.delayed(Duration(milliseconds: state.pollIntervalMs));
      }
    });
  }

  /// One round at a time: a call during a round makes it run once more, and
  /// waits for that.
  Future<void> runOnce() {
    if (_inFlight != null) {
      _rerun = true;
      return _inFlight!;
    }
    return _inFlight = () async {
      try {
        do {
          _rerun = false;
          await _round();
        } while (_rerun);
      } finally {
        _inFlight = null;
      }
    }();
  }

  void _saveRotated(String id, OauthCreds creds) {
    final acc = state.accounts.where((a) => a.id == id).firstOrNull;
    if (acc == null) return;
    acc.oauth = creds;
    store.persist(state);
  }

  Future<void> _round() async {
    final out = await fetchAll(client, [...state.accounts], state.meridian, _saveRotated);
    final s = state;
    var dirty = false;
    final profiles = out.meridian;
    s.meridianStatus = MeridianStatus(reachable: profiles != null, profiles: profiles?.length ?? 0, checkedAt: nowIso());
    if (!s.meridian.enabled) {
      final before = s.accounts.length;
      s.accounts.removeWhere((a) => a.source == Source.meridian);
      dirty |= s.accounts.length != before;
    }
    if (profiles != null) {
      dirty |= mergeMeridian(s.accounts, profiles.map((p) => p.id).where((id) => !s.meridian.removed.contains(id)));
    }
    s.ensureSnapshots();
    for (final a in s.accounts) {
      final pid = a.profileId;
      if (a.enabled && pid != null) meridianResultFor(profiles, pid).applyTo(s.snapshots[a.id]!);
    }
    for (final (id, res) in out.results) {
      final rotated = res.rotated;
      final acc = s.accounts.where((a) => a.id == id).firstOrNull;
      if (rotated != null && acc != null) {
        acc.oauth = rotated;
        dirty = true;
      }
      final snap = s.snapshots[id];
      if (snap != null) res.applyTo(snap);
    }
    s.primary = normalizePrimary(s.accounts, s.primary);
    s.mirrorPrimary();
    s.fetchedAt = nowIso();
    if (dirty) store.persist(s);
    notifyListeners();
  }

  /// Save and tell the tray, the popover and the settings window.
  void publish() {
    state.ensureSnapshots();
    state.primary = normalizePrimary(state.accounts, state.primary);
    state.mirrorPrimary();
    store.persist(state);
    notifyListeners();
  }

  void _refreshSoon() => unawaited(runOnce());

  Account? _find(String id) => state.accounts.where((a) => a.id == id).firstOrNull;

  String _addAccount(Account account) {
    if (state.primary.isEmpty) state.primary = account.id;
    state.accounts.add(account);
    publish();
    _refreshSoon();
    return account.id;
  }

  void _ensureNewToken(String token) {
    if (state.accounts.any((a) => a.token == token)) throw const AppError('That token is already added');
  }

  static String _labelOr(String? label, String fallback) {
    final l = (label ?? '').trim();
    return l.isEmpty ? fallback : l;
  }

  Map<String, dynamic> settingsBundle() => {
        ...state.publicJson(),
        'intervals': [for (final i in pollIntervals) {'label': i.label, 'ms': i.ms}],
        'iconStyles': iconStyles,
      };

  /// The settings window's commands. Errors are AppError with a message for the user.
  Future<Object?> invoke(String method, Map<String, Object?> a) async {
    switch (method) {
      case 'snapshot_get':
        return state.publicJson();
      case 'settings_get_all':
        return settingsBundle();
      case 'settings_get_autostart':
        return autostartEnabled();
      case 'settings_set_autostart':
        return setAutostart(a['on'] == true);
      case 'settings_set_primary':
        final id = a['id'] as String;
        if (state.accounts.any((x) => x.id == id && x.enabled)) {
          state.primary = id;
          publish();
        }
        return state.primary;
      case 'settings_set_interval':
        final ms = (a['ms'] as num).toInt();
        if (pollIntervals.any((x) => x.ms == ms)) {
          state.pollIntervalMs = ms;
          store.persist(state);
        }
        return state.pollIntervalMs;
      case 'settings_set_icon_style':
        final style = a['style'] as String;
        if (iconStyles.contains(style)) {
          state.iconStyle = style;
          publish();
        }
        return state.iconStyle;
      case 'settings_set_dev_mode':
        state.devMode = a['on'] == true;
        store.persist(state);
        notifyListeners();
        return state.devMode;
      case 'account_set_enabled':
        final on = a['on'] == true;
        _find(a['id'] as String)?.enabled = on;
        publish();
        if (on) _refreshSoon();
        return null;
      case 'account_rename':
        final label = (a['label'] as String? ?? '').trim();
        if (label.isEmpty) throw const AppError('Name cannot be empty');
        _find(a['id'] as String)?.label = label;
        publish();
        return null;
      case 'account_move':
        final from = state.accounts.indexWhere((x) => x.id == a['id']);
        if (from >= 0) {
          final acc = state.accounts.removeAt(from);
          state.accounts.insert((a['to'] as num).toInt().clamp(0, state.accounts.length), acc);
          publish();
        }
        return null;
      case 'account_remove':
        final id = a['id'] as String;
        // Remembered so the next poll does not add the Meridian profile back.
        if (id.startsWith(meridianPrefix)) {
          final pid = id.substring(meridianPrefix.length);
          if (!state.meridian.removed.contains(pid)) state.meridian.removed.add(pid);
        }
        state.accounts.removeWhere((x) => x.id == id);
        publish();
        return null;
      case 'meridian_set':
        // apiKey: null keeps the stored key, an empty string removes it.
        state.meridian.url = (a['url'] as String? ?? '').trim().replaceAll(RegExp(r'/+$'), '');
        final key = a['apiKey'] as String?;
        if (key != null) state.meridian.apiKey = key.trim().isEmpty ? null : key.trim();
        publish();
        _refreshSoon();
        return null;
      case 'meridian_set_enabled':
        // Off drops its accounts; on adds every profile back on the next poll, removed ones included.
        final on = a['on'] == true;
        state.meridian.enabled = on;
        if (on) {
          state.meridian.removed.clear();
        } else {
          state.accounts.removeWhere((x) => x.source == Source.meridian);
        }
        publish();
        _refreshSoon();
        return null;
      case 'claude_login_start':
        final p = newPkce();
        final url = claudeAuthorizeUrl(p);
        _claudePkce = p;
        openBrowser(url);
        return url;
      case 'claude_login_finish':
        final p = _claudePkce;
        if (p == null) throw const AppError('Open the sign-in page first');
        final creds = await claudeExchange(client, a['code'] as String? ?? '', p);
        _claudePkce = null;
        return _addAccount(Account.create(Provider.claude, Source.oauth, _labelOr(a['label'] as String?, 'Claude'))..oauth = creds);
      case 'codex_login':
        return codexLogin(a['label'] as String?);
      case 'copilot_add_token':
        final token = (a['token'] as String? ?? '').trim();
        if (token.isEmpty) throw const AppError('Paste a token');
        _ensureNewToken(token);
        return _addAccount(Account.create(Provider.copilot, Source.token, _labelOr(a['label'] as String?, 'Copilot'))..token = token);
      case 'copilot_import_list':
        return [for (final l in await _opencodeLogins()) {'id': l.id, 'label': l.label}];
      case 'copilot_import':
        final login = (await _opencodeLogins()).where((l) => l.id == a['id']).firstOrNull;
        if (login == null) throw const AppError('That OpenCode login is gone');
        _ensureNewToken(login.token);
        return _addAccount(
            Account.create(Provider.copilot, Source.token, _labelOr(a['label'] as String?, login.label))..token = login.token);
    }
    throw AppError('Unknown command $method');
  }

  // OpenCode's SQLite store can be busy, so it is read off the UI isolate.
  Future<List<CopilotLogin>> _opencodeLogins() => Isolate.run(listCopilotLogins);

  /// Browser sign-in: a one-shot server on a registered localhost port catches
  /// OpenAI's redirect. A newer click replaces this one and frees its port.
  Future<String> codexLogin(String? label) async {
    final old = _codex;
    if (old != null) {
      _codex = null;
      await old.server.close(force: true);
      if (!old.done.isCompleted) old.done.completeError(const AppError('Replaced by a newer sign-in'));
    }
    final p = newPkce();
    HttpServer? server;
    var port = 0;
    for (final candidate in codexPorts) {
      try {
        server = await HttpServer.bind(InternetAddress.loopbackIPv4, candidate);
        port = candidate;
        break;
      } on SocketException {
        continue;
      }
    }
    if (server == null) {
      throw const AppError('Ports 1455 and 1457 are busy: another Codex or OpenCode sign-in is open. Close it and try again.');
    }
    final login = _CodexLogin(server);
    _codex = login;
    final redirect = codexRedirect(port);

    void finish(Object result) {
      if (identical(_codex, login)) _codex = null;
      unawaited(login.server.close(force: true));
      if (login.done.isCompleted) return;
      result is String ? login.done.complete(result) : login.done.completeError(result);
    }

    final timeout = Timer(loginTimeout, () => finish(const AppError('Sign-in timed out. Try again.')));
    var handled = false;
    server.listen((req) async {
      final result = callbackCode(req.uri, p);
      if (result == null || handled) {
        req.response.statusCode = HttpStatus.notFound;
        await req.response.close();
        return;
      }
      handled = true;
      var error = result.error;
      OauthCreds? creds;
      if (error == null) {
        try {
          creds = await codexExchange(client, result.code!, redirect, p);
        } on AppError catch (e) {
          error = e.message;
        }
      }
      req.response
        ..headers.contentType = ContentType.html
        ..write(callbackPage(error));
      await req.response.close();
      timeout.cancel();
      if (login.done.isCompleted) return;
      if (creds == null) {
        finish(AppError(error!));
        return;
      }
      finish(_addAccount(Account.create(Provider.codex, Source.oauth, _labelOr(label, 'Codex'))..oauth = creds));
    });
    openBrowser(codexAuthorizeUrl(p, redirect));
    return login.done.future;
  }
}

/// The page the browser lands on after the redirect.
String callbackPage(String? error) {
  final body = error == null
      ? '<h2>Signed in</h2><p>Codex is added to ai-plan-usage. You can close this tab.</p>'
      : '<h2>Sign-in failed</h2><p>${const HtmlEscape().convert(error)}</p><p>Go back to ai-plan-usage and try again.</p>';
  return '<!doctype html><meta charset="utf-8"><title>ai-plan-usage</title>'
      '<body style="font-family:Segoe UI,system-ui,sans-serif;padding:40px;color:#18181b">$body</body>';
}

Future<bool> autostartEnabled() async => (await Process.run('reg', ['query', _runKey, '/v', _runValue])).exitCode == 0;

/// Writes or deletes the Run value, then reports what is there.
Future<bool> setAutostart(bool on) async {
  await Process.run(
    'reg',
    on
        ? ['add', _runKey, '/v', _runValue, '/t', 'REG_SZ', '/d', '"${Platform.resolvedExecutable}"', '/f']
        : ['delete', _runKey, '/v', _runValue, '/f'],
  );
  return autostartEnabled();
}
