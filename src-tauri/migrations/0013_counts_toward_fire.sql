-- Whether a class counts toward the FIRE portfolio is the user's call.
--
-- Until now it followed from the kind: "liquid" counted, everything else did
-- not. But the kind is chosen for how a class behaves in net worth, and a new
-- class made as "Other asset" — a brokerage account abroad, gold, a fund the
-- starter list had no name for — silently stayed out of the target with no
-- way to say otherwise. The flag starts where the old rule left it, so no
-- figure changes until someone ticks or unticks a box.
ALTER TABLE asset_class ADD COLUMN counts_fire INTEGER NOT NULL DEFAULT 0
    CHECK (counts_fire IN (0,1));
UPDATE asset_class SET counts_fire = 1 WHERE kind = 'liquid';

-- One definition of "counts", read by the view and by the FIRE command alike.
DROP VIEW v_net_worth;
CREATE VIEW v_net_worth AS
SELECT s.id         AS snapshot_id,
       s.taken_on,
       s.label,
       COALESCE(SUM(CASE WHEN k.kind = 'liability' THEN 0
                         ELSE l.gross_value_minor END), 0)  AS gross_assets_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'liability'
                         THEN l.gross_value_minor + l.debt_minor
                         ELSE l.debt_minor END), 0)         AS total_debt_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'liability'
                         THEN -(l.gross_value_minor + l.debt_minor)
                         ELSE l.gross_value_minor - l.debt_minor END), 0)
                                                            AS net_worth_minor,
       COALESCE(SUM(CASE WHEN k.kind <> 'liability' AND k.counts_fire = 1
                         THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0)
                                                            AS liquid_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'property'
                         THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0)
                                                            AS re_equity_minor
FROM wealth_snapshot s
LEFT JOIN wealth_line  l ON l.snapshot_id = s.id
LEFT JOIN asset_class  k ON k.key = l.bucket
GROUP BY s.id;
