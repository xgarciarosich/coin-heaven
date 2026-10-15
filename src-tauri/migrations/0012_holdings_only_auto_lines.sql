-- Only the portfolio writes net-worth lines on its own.
--
-- 0011 let bank accounts and loans compute snapshot lines too. In use that was
-- a second place to maintain numbers the snapshot already asks for, so the
-- screen that managed them is gone. Lines they had already written are kept —
-- they become ordinary lines the user can edit — rather than vanishing from
-- past net worth. The `loan` table and the account anchor columns are left in
-- place, unused: dropping them would only make this migration irreversible.
UPDATE wealth_line
   SET source = 'manual', source_id = NULL
 WHERE source IN ('account', 'loan');
