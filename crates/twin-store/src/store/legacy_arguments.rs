use rusqlite::Connection;

pub(super) fn remove_legacy_arguments(conn: &Connection) -> Result<(), rusqlite::Error> {
    for (table, column) in [
        ("observations", "metadata_json"),
        ("nodes", "metadata_json"),
        ("edges", "metadata_json"),
        ("node_history", "metadata_json"),
        ("edge_history", "metadata_json"),
        ("snapshot_nodes", "metadata_json"),
        ("snapshot_edges", "metadata_json"),
        ("collector_runs", "metadata_json"),
        ("test_runs", "report_json"),
    ] {
        conn.execute_batch(&format!(r"UPDATE {table} SET {column} = (
    WITH RECURSIVE
    safe(doc) AS (SELECT CASE WHEN json_valid({column}) THEN {column} ELSE '{{}}' END),
    counted(doc) AS (SELECT CASE
        WHEN json_type(doc, '$.argc') IS NOT NULL THEN doc
        WHEN json_type(doc, '$.argv') = 'array' THEN json_set(doc, '$.argc', json_array_length(doc, '$.argv'))
        WHEN json_type(doc, '$.argv_json') = 'text' THEN
            CASE WHEN json_valid(json_extract(doc, '$.argv_json')) THEN
                CASE WHEN json_type(json_extract(doc, '$.argv_json')) = 'array'
                     THEN json_set(doc, '$.argc', json_array_length(json_extract(doc, '$.argv_json')))
                     ELSE doc END
            ELSE doc END
        ELSE doc END FROM safe),
    paths(n, path) AS (SELECT row_number() OVER (ORDER BY length(fullkey) DESC, fullkey DESC), fullkey
        FROM counted, json_tree(counted.doc) WHERE key IN ('argv', 'argv_json')),
    cleaned(n, doc) AS (
        SELECT 0, doc FROM counted
        UNION ALL SELECT paths.n, json_remove(cleaned.doc, paths.path)
        FROM cleaned JOIN paths ON paths.n = cleaned.n + 1)
    SELECT doc FROM cleaned ORDER BY n DESC LIMIT 1
);"))?;
    }
    Ok(())
}
