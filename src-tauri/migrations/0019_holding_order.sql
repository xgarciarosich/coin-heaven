-- Holdings keep the order the person drags them into, as categories, asset
-- classes and the boxes of a monthly plan already do. Existing holdings start
-- in the alphabetical order they were shown in.
ALTER TABLE holding ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

UPDATE holding
   SET sort_order = (SELECT count(*) FROM holding h2 WHERE h2.name < holding.name
                        OR (h2.name = holding.name AND h2.id < holding.id)) * 10 + 10;
