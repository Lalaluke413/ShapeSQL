SELECT
    i.id,
    BOOL_AND(i.flag) OVER (PARTITION BY i.group_id) AS group_all_flags,
    BOOL_OR(i.flag) OVER (PARTITION BY i.group_id) AS group_any_flag
FROM inner_rows AS i;
