SELECT id
FROM outer_rows AS o
INNER JOIN inner_rows AS i ON o.id = i.id;
