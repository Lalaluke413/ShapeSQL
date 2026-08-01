SELECT
    i.group_id,
    BOOL_AND(i.flag) AS all_flags,
    BOOL_OR(i.flag) AS any_flag
FROM inner_rows AS i
GROUP BY i.group_id;
