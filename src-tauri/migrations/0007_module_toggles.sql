-- Module visibility and the primary display currency.
--
-- Toggling a module off only hides it: no data is touched, so turning it back
-- on restores everything. That matters because a user who hides the simulator
-- for a year should not lose their scenarios.
INSERT INTO app_setting (key, value_json) VALUES
    ('modules.import.enabled',    'true'),
    ('modules.ledger.enabled',    'true'),
    ('modules.wealth.enabled',    'true'),
    ('modules.simulator.enabled', 'true'),
    ('modules.assistant.enabled', 'true'),
    ('ui.currency.primary',       '"EUR"'),
    ('ui.locale',                 '"es-ES"')
ON CONFLICT(key) DO NOTHING;
