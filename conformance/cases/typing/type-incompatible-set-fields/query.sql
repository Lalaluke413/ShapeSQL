SELECT id
FROM outer_rows
UNION
SELECT flag
FROM inner_rows;
