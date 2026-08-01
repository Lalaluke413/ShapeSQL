SELECT FALSE AS value
FROM unit
INTERSECT
SELECT 1 / id = 0 AS value
FROM numbers;
