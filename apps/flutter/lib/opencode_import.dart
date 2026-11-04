// Copilot tokens from OpenCode's store (`opencode.db`, table `credential`),
// opened read only. GitHub device-flow tokens do not expire, so a copy is safe.
import 'dart:convert';
import 'dart:io';

import 'package:path/path.dart' as p;
import 'package:sqlite3/sqlite3.dart';

import 'model.dart';

class CopilotLogin {
  const CopilotLogin(this.id, this.label, this.token);

  final String id;
  final String label;
  final String token;
}

String opencodeDbPath() =>
    p.join(Platform.environment['USERPROFILE'] ?? '', '.local', 'share', 'opencode', 'opencode.db');

List<CopilotLogin> readCopilot(Database db) {
  final out = <CopilotLogin>[];
  final rows = db.select("SELECT id, label, value FROM credential WHERE integration_id = 'github-copilot' ORDER BY time_created");
  for (final r in rows) {
    // NULL, non-JSON or token-less rows are skipped.
    Object? value;
    try {
      value = jsonDecode(r['value'] as String? ?? '');
    } on FormatException {
      value = null;
    }
    final token = value is Map ? value['access'] : null;
    if (token is! String || token.isEmpty) continue;
    var label = r['label'] as String? ?? '';
    if (label.startsWith('Copilot - ')) label = label.substring('Copilot - '.length);
    label = label.trim();
    out.add(CopilotLogin(r['id'] as String, label.isEmpty ? 'OpenCode' : label, token));
  }
  return out;
}

List<CopilotLogin> listCopilotLogins([String? path]) {
  final dbPath = path ?? opencodeDbPath();
  if (!File(dbPath).existsSync()) throw AppError('OpenCode store not found at $dbPath');
  final Database db;
  try {
    db = sqlite3.open(dbPath, mode: OpenMode.readOnly);
  } on SqliteException catch (e) {
    throw AppError(e.toString());
  }
  try {
    return readCopilot(db);
  } on SqliteException catch (e) {
    throw AppError(e.toString());
  } finally {
    db.close();
  }
}
