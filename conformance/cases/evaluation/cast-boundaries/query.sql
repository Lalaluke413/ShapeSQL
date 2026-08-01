SELECT
    -9223372036854775808 % -1 AS remainder,
    CAST('+001' AS INT64) AS parsed_integer,
    CAST('fAlSe' AS BOOLEAN) AS parsed_boolean
FROM unit;
