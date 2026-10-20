-- A monthly plan can be made for one person or for two.
--
-- For two, every box says whose it is: one person's, the other's, or shared.
-- Shared spending is split half each or in proportion to what each brings in;
-- the split is worked out on screen from these rows, never stored, so it can
-- never disagree with them. The setup lives on the month, so an old plan reads
-- the way it was made even after the couple changes how they split.
ALTER TABLE plan_month ADD COLUMN mode TEXT NOT NULL DEFAULT 'individual'
    CHECK (mode IN ('individual', 'couple'));
ALTER TABLE plan_month ADD COLUMN person_a TEXT NOT NULL DEFAULT '';
ALTER TABLE plan_month ADD COLUMN person_b TEXT NOT NULL DEFAULT '';
ALTER TABLE plan_month ADD COLUMN split TEXT NOT NULL DEFAULT 'equal'
    CHECK (split IN ('equal', 'income'));

ALTER TABLE plan_item ADD COLUMN owner TEXT NOT NULL DEFAULT 'shared'
    CHECK (owner IN ('shared', 'a', 'b'));
