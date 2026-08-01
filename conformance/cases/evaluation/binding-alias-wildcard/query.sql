WITH chosen AS (
    SELECT i.id AS item_id, i.flag
    FROM inner_rows AS i
)
SELECT c.*, o.group_id AS outer_group
FROM chosen AS c
INNER JOIN outer_rows AS o ON c.item_id = o.id
ORDER BY outer_group ASC;
