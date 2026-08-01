SELECT
    COUNT(*) AS row_count,
    COUNT(value) AS value_count,
    SUM(value) AS total,
    MIN(value) AS minimum,
    MAX(value) AS maximum,
    BOOL_AND(flag) AS all_flags,
    BOOL_OR(flag) AS any_flag
FROM rows;
