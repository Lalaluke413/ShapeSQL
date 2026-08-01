WITH unused AS (
    SELECT 1 / 0 AS value
    FROM unit
)
SELECT 7 AS value
FROM unit;
