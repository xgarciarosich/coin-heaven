-- Deleting a transaction must never leave a half-made link behind.
--
-- A refund points at the bill it reimburses through two columns that the table
-- insists are both set or both empty. The foreign key cleared only one of them
-- when the bill went, so deleting an account, undoing an import or removing the
-- sample failed with a constraint error whenever a refund was linked across.
CREATE TRIGGER txn_unlink_children
BEFORE DELETE ON txn
BEGIN
    UPDATE txn SET parent_txn_id = NULL, link_kind = NULL
     WHERE parent_txn_id = OLD.id;
END;

-- A transfer is two rows that name each other. When one goes, the other is no
-- longer half of anything: it is an ordinary transaction again, and flagged so
-- that the person decides what it is.
CREATE TRIGGER txn_transfer_pair_broken
AFTER DELETE ON txn
WHEN OLD.transfer_group_id IS NOT NULL
BEGIN
    UPDATE txn
       SET transfer_group_id = NULL, is_neutral_transfer = 0, needs_review = 1
     WHERE transfer_group_id = OLD.transfer_group_id
       AND (SELECT count(*) FROM txn WHERE transfer_group_id = OLD.transfer_group_id) < 2;
END;
