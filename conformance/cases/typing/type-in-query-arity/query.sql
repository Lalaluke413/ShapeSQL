SELECT id
FROM outer_rows
WHERE id IN (
    SELECT id, group_id
    FROM inner_rows
);
