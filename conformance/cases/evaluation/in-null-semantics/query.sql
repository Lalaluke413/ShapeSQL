SELECT
    value,
    value IN (1, NULL) AS is_member,
    value NOT IN (1, NULL) AS is_not_member
FROM values_rows;
