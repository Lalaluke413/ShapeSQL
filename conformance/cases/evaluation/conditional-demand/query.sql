SELECT
    CASE WHEN TRUE THEN 7 ELSE 1 / 0 END AS case_value,
    FALSE AND (1 / 0 = 0) AS and_value,
    TRUE OR (1 / 0 = 0) AS or_value
FROM unit;
