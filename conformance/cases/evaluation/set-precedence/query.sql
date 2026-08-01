SELECT o.id
FROM outer_rows AS o
UNION
SELECT i.id
FROM inner_rows AS i
INTERSECT
SELECT i.group_id
FROM inner_rows AS i
ORDER BY id ASC;
