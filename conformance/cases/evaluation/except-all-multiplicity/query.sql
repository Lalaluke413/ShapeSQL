SELECT value FROM left_rows
EXCEPT ALL
SELECT value FROM right_rows;
