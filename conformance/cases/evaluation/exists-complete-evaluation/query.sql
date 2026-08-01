SELECT EXISTS (
    SELECT 1 / id
    FROM numbers
) AS any_row
FROM unit;
