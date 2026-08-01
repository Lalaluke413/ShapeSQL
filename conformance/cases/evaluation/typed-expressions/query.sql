SELECT
    CASE WHEN flag THEN -9223372036854775808 ELSE id END AS selected_id,
    CAST(id AS TEXT) || ':' || CAST(flag AS TEXT) AS label,
    CAST('42' AS INT64) AS parsed_integer,
    CAST('true' AS BOOLEAN) AS parsed_boolean,
    CAST(NULL AS INT64) AS missing_id,
    id IN (1, NULL, 3) AS membership,
    COUNT(*) OVER (PARTITION BY group_id) AS group_size
FROM inner_rows
WHERE flag IS NULL OR flag;
