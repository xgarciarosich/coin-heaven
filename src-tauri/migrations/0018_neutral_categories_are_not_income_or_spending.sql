-- Money moved between your own pockets is neither earned nor spent.
--
-- A transfer marked as such (`is_neutral_transfer`) was already left out of
-- every income, spending, budget and analytics figure. A transaction filed
-- under a *neutral* category — "Internal transfer", "Investment flow", or any
-- category the person made neutral — was not: it counted as spending when it
-- left and as income when it came back. Every figure reads this view, so the
-- rule is stated once, here.
DROP VIEW IF EXISTS v_txn_net;

CREATE VIEW v_txn_net AS
SELECT e.id,
       e.account_id,
       e.booked_on,
       e.currency,
       e.project_id,
       e.effective_category_id,
       e.amount_minor AS gross_minor,
       e.amount_minor + COALESCE(
           (SELECT SUM(c.amount_minor) FROM txn c
             WHERE c.parent_txn_id = e.id AND c.link_kind = 'reimbursement'), 0
       ) AS net_minor
FROM v_txn_effective e
JOIN txn t ON t.id = e.id
LEFT JOIN pl_category k ON k.id = e.effective_category_id
WHERE t.is_neutral_transfer = 0
  AND COALESCE(k.kind, 'expense') <> 'neutral'
  AND NOT (t.parent_txn_id IS NOT NULL AND t.link_kind = 'reimbursement');
