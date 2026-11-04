import 'package:ai_plan_usage/model.dart';
import 'package:ai_plan_usage/opencode_import.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sqlite3/sqlite3.dart';

void main() {
  test('reads Copilot rows only', () {
    final db = sqlite3.openInMemory();
    addTearDown(db.close);
    db.execute('''
      CREATE TABLE credential (id TEXT, integration_id TEXT, label TEXT, value TEXT, time_created INTEGER);
      INSERT INTO credential VALUES ('c2','github-copilot','Copilot - Tecnomat','{"type":"oauth","access":"gho_b","refresh":"gho_b","expires":0}',2);
      INSERT INTO credential VALUES ('c1','github-copilot','Copilot - BIP','{"type":"oauth","access":"gho_a","refresh":"gho_a","expires":0}',1);
      INSERT INTO credential VALUES ('c3','openai','OAuth','{"type":"oauth","access":"x"}',3);
      INSERT INTO credential VALUES ('c4','github-copilot',NULL,'{"type":"oauth"}',4);
      INSERT INTO credential VALUES ('c5','github-copilot',NULL,'{"type":"oauth","access":"gho_c"}',5);
      INSERT INTO credential VALUES ('c6','github-copilot','Copilot - Null',NULL,6);
    ''');
    expect([for (final l in readCopilot(db)) (l.id, l.label, l.token)], [
      ('c1', 'BIP', 'gho_a'),
      ('c2', 'Tecnomat', 'gho_b'),
      ('c5', 'OpenCode', 'gho_c'),
    ]);
  });

  test('a missing store is a readable error', () {
    expect(() => listCopilotLogins(r'C:\nowhere\opencode.db'),
        throwsA(isA<AppError>().having((e) => e.message, 'm', r'OpenCode store not found at C:\nowhere\opencode.db')));
  });
}
