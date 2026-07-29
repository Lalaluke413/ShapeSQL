SELECT o.id
FROM outer_rows AS o
WHERE o.id IN (
    SELECT i.id
    FROM inner_rows AS i
);
