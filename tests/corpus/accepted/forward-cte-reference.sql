WITH
    first_rows AS (
        SELECT s.id
        FROM second_rows AS s
    ),
    second_rows AS (
        SELECT i.id
        FROM inner_rows AS i
    )
SELECT f.id
FROM first_rows AS f;
