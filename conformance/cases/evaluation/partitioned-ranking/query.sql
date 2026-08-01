SELECT
    id,
    COUNT(*) OVER (PARTITION BY group_id) AS partition_count,
    SUM(value) OVER (PARTITION BY group_id) AS partition_total,
    ROW_NUMBER() OVER (
        PARTITION BY group_id
        ORDER BY group_id ASC, id ASC, value ASC
    ) AS row_number,
    RANK() OVER (
        PARTITION BY group_id
        ORDER BY value DESC
    ) AS rank,
    DENSE_RANK() OVER (
        PARTITION BY group_id
        ORDER BY value DESC
    ) AS dense_rank
FROM rows;
