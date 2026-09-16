#!/usr/bin/env python3
"""Generate docs/database-schema.md from the baseline schema.

The document is derived from `src-tauri/src/db/schema.rs`, never edited by
hand: the previous version described a 94-migration chain for weeks after
that chain was deleted. Applying the baseline to an in-memory SQLite database
and reading `sqlite_master` back gives the same view the app has.

    python3 scripts/db/generate-schema-doc.py          # rewrite the doc
    python3 scripts/db/generate-schema-doc.py --check  # exit 1 if it is stale

Only the Python standard library is needed. SQLCipher is not: the baseline is
plain SQL and the encryption is applied per profile at open time.
"""

import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "src-tauri" / "src" / "db" / "schema.rs"
RUNNER = ROOT / "src-tauri" / "src" / "db" / "mod.rs"
TESTS = ROOT / "src-tauri" / "src" / "db" / "schema_tests.rs"
DOC = ROOT / "docs" / "database-schema.md"

# Domain assignment by name. Every table must match exactly one rule; the
# generator fails otherwise, so a new table has to be placed deliberately.
DOMAINS = [
    ("Identity", r"^local_identity$"),
    ("Guardianship", r"^guardian_"),
    ("Taxonomy", r"^(subject_fields|subjects|skills|skill_prerequisites|skill_relations|taxonomy_versions)$"),
    ("Courses and learning", r"^(courses|course_chapters|course_elements|element_skill_tags|enrollments|element_progress|course_notes|video_chapters|catalog)$"),
    ("Assessments", r"^(question_banks|assessment_items|assessment_item_skills|assessment_attempts|attempt_items)$"),
    ("Goals", r"^goal_template"),
    ("Community plugins", r"^(plugin_|element_submissions$)"),
    ("Verifiable credentials", r"^(key_registry|credentials|credential_status_lists|credential_anchors|credentials_pending_verification|credential_allowlist|presentations_seen|pinboard_observations)$"),
    ("Chain journal", r"^chain_submission"),
    ("Completion and endorsements", r"^(completion_|course_completion_endorsements$)"),
    ("Opinions", r"^opinions"),
    ("Reputation", r"^(reputation_|derived_skill_)"),
    ("Integrity (Sentinel)", r"^(integrity_|sentinel_)"),
    ("Interviews", r"^interview_"),
    ("Organizations and role assessments", r"^(organizations|role_assessments)$"),
    ("Classrooms and tutoring", r"^(classroom|tutoring_sessions$)"),
    ("Genesis trust", r"^governance_genesis_trust_anchors$"),
    ("P2P, content and sync", r"^(peers|pins|sync_log|devices|sync_state|sync_queue|pending_pairings|dht_records|peer_profiles|username_claims|content_mappings|stake_pubkey_registry)$"),
    ("Settings", r"^app_settings$"),
]


def baseline_sql():
    text = SCHEMA.read_text()
    return text.split('const MIGRATION_001_BASELINE: &str = r#"', 1)[1].rsplit('"#;', 1)[0]


def const(path, name):
    match = re.search(rf"pub const {name}: [^=]+= ([^;]+);", path.read_text())
    if not match:
        sys.exit(f"{name} not found in {path}")
    return match.group(1).strip().strip('"')


def forbidden_tables():
    text = TESTS.read_text()
    block = text.split("const FORBIDDEN_TABLES", 1)[1].split("];", 1)[0]
    return re.findall(r'"([a-z_]+)"', block)


def inventory():
    conn = sqlite3.connect(":memory:")
    conn.executescript(baseline_sql())
    tables = {}
    for name, sql in conn.execute(
        "SELECT name, sql FROM sqlite_master WHERE type = 'table' "
        "AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ):
        comments = {}
        for line in sql.split("\n"):
            match = re.match(r"\s*([a-z_]+)\s.*?--\s*(.+)$", line)
            if match:
                comments[match.group(1)] = match.group(2).strip()
        columns = [
            {"name": r[1], "type": r[2], "notnull": bool(r[3]), "default": r[4], "pk": bool(r[5])}
            for r in conn.execute(f"PRAGMA table_info({name})")
        ]
        fks = [
            {"col": r[3], "ref": r[2], "refcol": r[4] or "id"}
            for r in conn.execute(f"PRAGMA foreign_key_list({name})")
        ]
        tables[name] = {"columns": columns, "fks": fks, "comments": comments}
    indexes = [
        r[0]
        for r in conn.execute(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'"
        )
    ]
    views = dict(conn.execute("SELECT name, sql FROM sqlite_master WHERE type = 'view'"))
    triggers = [r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type = 'trigger' ORDER BY name")]
    return tables, indexes, views, triggers


def domain_of(name):
    matches = [d for d, pattern in DOMAINS if re.search(pattern, name)]
    if len(matches) != 1:
        sys.exit(f"table {name} matches {matches or 'no'} domain rules; it must match exactly one")
    return matches[0]


def render_column(col, fks, comments):
    parts = [f"`{col['name']}`", col["type"] or "ANY"]
    if col["pk"]:
        parts.append("PK")
    elif col["notnull"]:
        parts.append("NOT NULL")
    if col["default"] is not None:
        parts.append(f"default `{col['default']}`")
    for fk in fks:
        if fk["col"] == col["name"]:
            parts.append(f"→ `{fk['ref']}.{fk['refcol']}`")
    line = " ".join(parts)
    if col["name"] in comments:
        line += f" — {comments[col['name']]}"
    return line


def render(tables, indexes, views, triggers):
    app_id = const(RUNNER, "SCHEMA_APPLICATION_ID")
    epoch = const(RUNNER, "SCHEMA_EPOCH")
    family = const(RUNNER, "SCHEMA_FAMILY")
    forbidden = forbidden_tables()

    by_domain = {}
    for name in tables:
        by_domain.setdefault(domain_of(name), []).append(name)

    out = []
    w = out.append
    w("# Database Schema")
    w("")
    w("> Alexandria — SQLite (local-first)")
    w("")
    w("> **Generated** from `src-tauri/src/db/schema.rs` by")
    w("> `scripts/db/generate-schema-doc.py`. Do not edit by hand; regenerate, or run")
    w("> it with `--check` to see whether this file is stale.")
    w("")
    w("**Engine**: SQLCipher (rusqlite, `bundled-sqlcipher`) — each profile is its own encrypted database, opened with `PRAGMA key`.")
    w(f"**Schema**: one baseline migration, family `{family}`, epoch {epoch}.")
    w(f"**Objects**: {len(tables)} tables, {len(indexes)} indexes, {len(views)} view, {len(triggers)} triggers.")
    w("")
    w("---")
    w("")
    w("## How the schema is managed")
    w("")
    w("One migration, `MIGRATION_001_BASELINE`, creates the whole schema. It replaced")
    w("a chain of 94 migrations: that chain was replayed into a database, the result")
    w("was dumped, the retired tables and columns were removed, and parity was checked")
    w("object by object. The runner in `db/mod.rs` still applies migrations")
    w("atomically, one transaction each, and records them in `_migrations`; the")
    w("baseline is a starting point for future migrations, not a replacement for them.")
    w("")
    w("A database is stamped with its schema family before any normal query runs:")
    w("")
    w(f"- `PRAGMA application_id` = `{app_id}` — the first four bytes of `sha256(\"{family}\")`, masked positive. A file carrying any other value was written by something else.")
    w(f"- `PRAGMA user_version` = `{epoch}` — the family epoch. A new baseline bumps it; ordinary migrations never do.")
    w(f"- `_schema_identity` — one row naming the family and epoch, so a refusal can say what a file belongs to.")
    w("")
    w("`validate_or_stamp_identity` in `db/mod.rs` stamps an empty file and refuses")
    w("everything else that does not match: a file with tables but no stamp (the old")
    w("chain), a foreign `application_id`, an epoch ahead of this build, an epoch")
    w("behind it, or a header that disagrees with `_schema_identity`. A refusal")
    w("never converts or deletes the file. Pre-launch data is disposable, which is")
    w("why there is no upgrade path from the old chain.")
    w("")
    w("## Deliberately absent")
    w("")
    w("These tables existed in the old chain and are not created by the baseline.")
    w("Each lost the code that gave it authority; `forbidden_tables_are_absent` in")
    w("`db/schema_tests.rs` fails if one returns.")
    w("")
    for name in forbidden:
        w(f"- `{name}`")
    w("")
    w("Columns dropped with them: `local_identity.account_role` (superseded by the")
    w("`account_roles` set) and the CIP-68 columns on `reputation_snapshots`")
    w("(`policy_id`, `ref_asset_name`, `user_asset_name`, `snapshot_format`,")
    w("`snapshot_scope`).")
    w("")
    w("---")
    w("")
    w("## Design principles")
    w("")
    w("- **Deterministic IDs**: most entities use `hex(blake2b_256(parts.join(\"|\")))` rather than generated UUIDs.")
    w("- **Singleton identity per profile**: `local_identity` has `CHECK (id = 1)`. Each profile is its own database file, so the singleton is per profile, not per device.")
    w("- **No server tables**: the app is profile-based and local-first.")
    w("- **External content**: course content, published profiles and evidence bundles live in the iroh content store as BLAKE3-addressed blobs; SQLite holds metadata, references and caches.")
    w("- **Text timestamps**: ISO-8601 `TEXT`, for portability and inspection.")
    w("- **Canonical source**: exact DDL, defaults, `CHECK` constraints and indexes are in `src-tauri/src/db/schema.rs`. This document is a rendering of it.")
    w("")
    w("---")
    w("")
    w("## Tables by domain")
    w("")
    for domain, _ in DOMAINS:
        names = by_domain.get(domain)
        if not names:
            continue
        w(f"### {domain} ({len(names)})")
        w("")
        for name in names:
            t = tables[name]
            w(f"#### `{name}`")
            w("")
            for col in t["columns"]:
                w(f"- {render_column(col, t['fks'], t['comments'])}")
            w("")
    w("---")
    w("")
    w("## View and triggers")
    w("")
    for name, sql in views.items():
        w(f"**`{name}`**")
        w("")
        w("```sql")
        w(sql.strip() + ";")
        w("```")
        w("")
    w("Triggers: " + ", ".join(f"`{t}`" for t in triggers) + ".")
    w("")
    w("---")
    w("")
    w("## Entity relationships")
    w("")
    w("Every foreign key in the baseline, parent to child.")
    w("")
    w("```mermaid")
    w("erDiagram")
    seen = set()
    for name in tables:
        for fk in tables[name]["fks"]:
            edge = (fk["ref"], name, fk["col"])
            if edge in seen:
                continue
            seen.add(edge)
            w(f"    {fk['ref']} ||--o{{ {name} : {fk['col']}")
    w("```")
    w("")
    return "\n".join(out)


def main():
    text = render(*inventory())
    if "--check" in sys.argv:
        if DOC.read_text() != text:
            sys.exit(f"{DOC.relative_to(ROOT)} is stale; regenerate it")
        print("database-schema.md is current")
        return
    DOC.write_text(text)
    print(f"wrote {DOC.relative_to(ROOT)} ({text.count(chr(10))} lines)")


if __name__ == "__main__":
    main()
