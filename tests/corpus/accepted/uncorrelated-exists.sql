SELECT o.id
FROM outer_rows AS o
WHERE EXISTS (
    SELECT i.id
    FROM inner_rows AS i
    WHERE i.group_id = 1
);
