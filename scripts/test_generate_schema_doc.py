import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parent / 'db' / 'generate-schema-doc.py'
spec = importlib.util.spec_from_file_location('generate_schema_doc', SCRIPT)
schema_doc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(schema_doc)


class GenerateSchemaDocTests(unittest.TestCase):
    def test_every_current_migration_is_documented(self):
        migrations = schema_doc.migrations()
        self.assertEqual([version for version, _, _ in migrations], list(range(1, 10)))
        tables, indexes, views, triggers = schema_doc.inventory()
        self.assertEqual(len(tables), 106)
        self.assertIn('developer_discussion_drafts', tables)
        self.assertIn('discussion_events', tables)
        self.assertIn('tutoring_presence', tables)
        self.assertIn('decision_shadow_samples', tables)
        self.assertEqual(schema_doc.render(tables, indexes, views, triggers),
                         schema_doc.DOC.read_text())

    def test_unsupported_expression_is_not_silently_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / 'schema.rs'
            source.write_text('pub const MIGRATIONS: &[(i64, &str, &str)] = &[\n'
                              '(1, "unknown", concat!("CREATE ", "TABLE sample(id);")),\n];')
            with patch.object(schema_doc, 'SCHEMA', source):
                with self.assertRaisesRegex(SystemExit, 'unsupported migration entry'):
                    schema_doc.migrations()


if __name__ == '__main__':
    unittest.main()
