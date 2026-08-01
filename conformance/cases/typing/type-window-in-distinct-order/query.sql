SELECT DISTINCT group_id
FROM outer_rows
ORDER BY RANK() OVER (ORDER BY group_id);
