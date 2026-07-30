SELECT
    i.group_id,
    COUNT(*) AS group_size,
    ROW_NUMBER() OVER (ORDER BY i.group_id ASC) AS position
FROM inner_rows AS i
GROUP BY i.group_id;
