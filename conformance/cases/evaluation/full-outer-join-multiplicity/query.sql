SELECT l.id AS left_id, r.id AS right_id
FROM left_rows AS l
FULL JOIN right_rows AS r ON l.id = r.id;
