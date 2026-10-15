-- Minimal, opinion-light starter taxonomy. Users are expected to edit it;
-- nothing downstream hardcodes these ids.
INSERT INTO pl_category (name, kind, sort_order, color) VALUES
    ('Salary',            'income',  10, '#22c55e'),
    ('Rental income',     'income',  20, '#16a34a'),
    ('Other income',      'income',  30, '#4ade80'),
    ('Housing',           'expense', 40, '#f97316'),
    ('Food & groceries',  'expense', 50, '#eab308'),
    ('Transport',         'expense', 60, '#06b6d4'),
    ('Leisure & travel',  'expense', 70, '#a855f7'),
    ('Health',            'expense', 80, '#ef4444'),
    ('Subscriptions',     'expense', 90, '#8b5cf6'),
    ('Taxes',             'expense', 100, '#64748b'),
    ('Uncategorised',     'expense', 999, '#94a3b8'),
    ('Internal transfer', 'neutral', 1000, '#334155'),
    ('Investment flow',   'neutral', 1010, '#475569');

INSERT INTO app_setting (key, value_json) VALUES
    ('schema.currency.default',        '"EUR"'),
    ('privacy.ai.enabled',             'false'),
    ('privacy.ai.require_preview',     'true'),
    ('privacy.ai.allow_raw_descriptions', 'false'),
    ('import.transfer_window_days',    '3'),
    ('import.transfer_tolerance_minor','0'),
    ('ui.theme',                       '"dark"'),
    ('onboarding.completed',           'false');
