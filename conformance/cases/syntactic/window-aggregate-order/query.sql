SELECT SUM(i.id) OVER (ORDER BY i.id)
FROM inner_rows AS i;
