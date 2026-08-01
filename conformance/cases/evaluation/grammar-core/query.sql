WITH totals AS (
    SELECT
        i.group_id AS group_id,
        SUM(i.id) AS total
    FROM inner_rows AS i
    GROUP BY i.group_id
)
SELECT
    o.group_id,
    t.total
FROM outer_rows AS o
LEFT JOIN totals AS t ON t.group_id = o.group_id
WHERE o.id > 0
ORDER BY group_id ASC, total ASC NULLS LAST
LIMIT 10 OFFSET 0;
