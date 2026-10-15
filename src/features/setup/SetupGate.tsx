import { useEffect, useState } from "react";
import { ArrowRight, Check, KeyRound, Lock, Moon, Sun, Unlock } from "lucide-react";
import { api } from "@/lib/ipc";
import type { Theme } from "@/lib/theme";
import type { PassphraseStrength, UnlockStatus } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/**
 * Everything that happens before the database is open.
 *
 * The first screen is a question, not an action. An earlier version created a
 * key and claimed a keychain entry the instant it launched, so the first thing
 * anyone saw was macOS demanding a password for a decision they had never been
 * offered. Nothing is created here until the user has chosen.
 */
export function SetupGate({
  status,
  onDone,
  theme,
  onToggleTheme,
}: {
  status: UnlockStatus;
  onDone: () => void;
  theme: Theme;
  onToggleTheme: () => void;
}) {
  const t = useT();
  return (
    <div className="relative flex h-full flex-col">
      <div
        data-tauri-drag-region
        className="flex h-9 shrink-0 items-center justify-end px-4"
      >
        <button
          type="button"
          onClick={onToggleTheme}
          aria-label={t.setup.switchTheme}
          className="rounded p-1 text-ink-3 transition-colors hover:text-ink"
        >
          {theme === "light" ? (
            <Moon className="size-3.5" strokeWidth={1.75} />
          ) : (
            <Sun className="size-3.5" strokeWidth={1.75} />
          )}
        </button>
      </div>

      <div className="flex flex-1 items-center justify-center overflow-y-auto px-8 pb-16">
        <div className="w-full max-w-[460px]">
          <Router status={status} onDone={onDone} />
        </div>
      </div>
    </div>
  );
}

function Router({
  status,
  onDone,
}: {
  status: UnlockStatus;
  onDone: () => void;
}) {
  const [chosen, setChosen] = useState<"none" | "passphrase" | null>(null);

  switch (status.state) {
    case "needs_setup":
      return chosen === "passphrase" ? (
        <ChoosePassphrase onBack={() => setChosen(null)} onDone={onDone} />
      ) : (
        <ChooseProtection
          onPickNone={onDone}
          onPickPassphrase={() => setChosen("passphrase")}
        />
      );
    case "needs_passphrase":
      return <UnlockPassphrase onDone={onDone} />;
    case "needs_recovery_phrase":
      return <UnlockPhrase onDone={onDone} />;
    case "needs_legacy_upgrade":
      return <LegacyUpgrade onDone={onDone} />;
    default:
      return <ShowPhrase onDone={onDone} pending={status.phrasePending} />;
  }
}

/* --------------------------------------------------------- 1. the choice -- */

function ChooseProtection({
  onPickNone,
  onPickPassphrase,
}: {
  onPickNone: () => void;
  onPickPassphrase: () => void;
}) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const createWithout = () => {
    setBusy(true);
    setError(null);
    api
      .setupDatabase(null)
      .then(onPickNone)
      .catch((e: unknown) =>
        setError(e instanceof Error ? e.message : String(e)),
      )
      .finally(() => setBusy(false));
  };

  return (
    <div>
      <p className="eyebrow">{t.setup.settingUp}</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.protectTitle[0]}
        <br />
        {t.setup.protectTitle[1]}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {t.setup.protectLede}
      </p>

      <div className="mt-7 space-y-3">
        <Option
          icon={<Lock className="size-4" strokeWidth={1.75} />}
          title={t.setup.askTitle}
          body={t.setup.askBody}
          note={t.setup.recommended}
          onClick={onPickPassphrase}
          disabled={busy}
        />
        <Option
          icon={<Unlock className="size-4" strokeWidth={1.75} />}
          title={t.setup.autoTitle}
          body={t.setup.autoBody}
          onClick={createWithout}
          disabled={busy}
        />
      </div>

      {error && <Problem message={error} />}

      <p className="mt-6 border-t border-rule pt-4 text-[11.5px] leading-[1.6] text-ink-3">
        {t.setup.eitherWay}
      </p>
    </div>
  );
}

function Option({
  icon,
  title,
  body,
  note,
  onClick,
  disabled,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
  note?: string;
  onClick: () => void;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="group w-full rounded-lg border border-rule bg-surface px-4 py-3.5 text-left transition-colors hover:border-brass disabled:opacity-40"
    >
      <div className="flex items-center gap-2.5">
        <span className="text-brass">{icon}</span>
        <span className="text-[13.5px] font-medium text-ink">{title}</span>
        {note && (
          <span className="ml-auto rounded bg-brass-wash px-1.5 py-0.5 text-[10px] tracking-wide text-brass uppercase">
            {note}
          </span>
        )}
      </div>
      <p className="mt-1.5 text-[12px] leading-[1.6] text-ink-2">{body}</p>
    </button>
  );
}

/* ----------------------------------------------------- 2. the passphrase -- */

function ChoosePassphrase({
  onBack,
  onDone,
}: {
  onBack: () => void;
  onDone: () => void;
}) {
  const t = useT();
  const [pass, setPass] = useState("");
  const [again, setAgain] = useState("");
  const [strength, setStrength] = useState<PassphraseStrength | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!pass) {
      setStrength(null);
      return;
    }
    let live = true;
    api
      .passphraseStrength(pass)
      .then((s) => live && setStrength(s))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [pass]);

  const mismatch = again.length > 0 && again !== pass;
  const ready = !!strength?.acceptable && pass === again && pass.length > 0;

  return (
    <div>
      <p className="eyebrow">{t.setup.settingUp}</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.chooseTitle}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {t.setup.chooseLede}
      </p>

      <div className="mt-7 space-y-4">
        <Field
          label={t.setup.passphrase}
          value={pass}
          onChange={setPass}
          autoFocus
        />
        {strength && (
          <div>
            <div className="flex gap-1">
              {[0, 1, 2, 3].map((i) => (
                <span
                  key={i}
                  className={cn(
                    "h-[3px] flex-1 rounded-full transition-colors",
                    i < strength.score
                      ? strength.score >= 3
                        ? "bg-positive"
                        : "bg-brass"
                      : "bg-rule-strong",
                  )}
                />
              ))}
            </div>
            <p className="mt-1.5 text-[11.5px] text-ink-3">{strength.message}</p>
          </div>
        )}
        <Field label={t.setup.repeat} value={again} onChange={setAgain} />
        {mismatch && (
          <p className="text-[12px] text-negative">
            {t.setup.mismatch}
          </p>
        )}
      </div>

      {error && <Problem message={error} />}

      <Action
        disabled={!ready}
        busy={busy}
        label={t.setup.setPassphrase}
        busyLabel={t.setup.encrypting}
        onClick={() => {
          setBusy(true);
          setError(null);
          api
            .setupDatabase(pass)
            .then(onDone)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            )
            .finally(() => setBusy(false));
        }}
      />
      <button
        type="button"
        onClick={onBack}
        className="mt-3 w-full text-[12px] text-ink-3 transition-colors hover:text-ink"
      >
        {t.setup.back}
      </button>
    </div>
  );
}

/* --------------------------------------------------- 3. the phrase, once -- */

function ShowPhrase({
  onDone,
  pending,
}: {
  onDone: () => void;
  pending: boolean;
}) {
  const t = useT();
  const [phrase, setPhrase] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [gone, setGone] = useState(!pending);

  useEffect(() => {
    if (!pending) return;
    api
      .recoveryTakePhrase()
      .then((p) => {
        setPhrase(p);
        setGone(p === null);
      })
      .catch((e: unknown) =>
        setError(e instanceof Error ? e.message : String(e)),
      );
  }, [pending]);

  const words = phrase?.split(/\s+/) ?? [];
  const ready = typed.trim().split(/\s+/).filter(Boolean).length === 12;

  return (
    <div>
      <p className="eyebrow">{t.setup.lastThing}</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.writeTitle[0]}
        <br />
        {t.setup.writeTitle[1]}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {t.setup.writeLede}
      </p>

      {gone ? (
        <p className="mt-6 rounded-lg border border-rule bg-surface px-4 py-3.5 text-[12px] leading-[1.6] text-ink-2">
          {t.setup.gone}
        </p>
      ) : phrase === null ? (
        <p className="mt-6 text-[12px] text-ink-3">{t.setup.generating}</p>
      ) : (
        <ol
          data-selectable
          className="mt-7 grid grid-cols-3 gap-x-6 border-y border-rule py-1"
        >
          {words.map((w, i) => (
            <li
              key={`${i}-${w}`}
              className="flex items-baseline gap-2.5 border-b border-rule/50 py-2 [&:nth-last-child(-n+3)]:border-b-0"
            >
              <span className="tnum w-3 text-right text-[10px] text-ink-3">
                {i + 1}
              </span>
              <span className="font-mono text-[13px] text-ink">{w}</span>
            </li>
          ))}
        </ol>
      )}

      <div className="mt-7">
        <label htmlFor="confirm" className="eyebrow">
          {t.setup.typeBack}
        </label>
        <textarea
          id="confirm"
          rows={3}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          value={typed}
          onChange={(e) => setTyped(e.currentTarget.value)}
          placeholder={t.setup.typePlaceholder}
          className="mt-2 w-full rounded-lg border border-rule bg-surface px-3.5 py-3 text-[13px] leading-[1.6] text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-brass"
        />
      </div>

      {error && <Problem message={error} />}

      <Action
        disabled={!ready}
        busy={busy}
        label={t.setup.writtenDown}
        busyLabel={t.setup.checking}
        icon={<Check className="size-3.5" strokeWidth={2} />}
        onClick={() => {
          setBusy(true);
          setError(null);
          api
            .recoveryConfirmPhrase(typed)
            .then(onDone)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            )
            .finally(() => setBusy(false));
        }}
      />

      <button
        type="button"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          api
            .recoveryDeferConfirmation()
            .then(onDone)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            )
            .finally(() => setBusy(false));
        }}
        className="mt-3 w-full text-[12px] text-ink-3 transition-colors hover:text-ink"
      >
        {t.setup.skip}
      </button>
      <p className="mt-2 text-center text-[11px] text-ink-3/80">
        {t.setup.reminding}
      </p>
    </div>
  );
}

/* --------------------------------------------------------- 4. unlocking -- */

function UnlockPassphrase({ onDone }: { onDone: () => void }) {
  const t = useT();
  const [pass, setPass] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);

  if (showRecovery) return <UnlockPhrase onDone={onDone} forgot />;

  return (
    <div>
      <p className="eyebrow">Coin Heaven</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.welcome}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {t.setup.enterPassphrase}
      </p>

      <form
        className="mt-6"
        onSubmit={(e) => {
          e.preventDefault();
          if (!pass) return;
          setBusy(true);
          setError(null);
          api
            .unlockWithPassphrase(pass)
            .then(onDone)
            .catch((err: unknown) =>
              setError(err instanceof Error ? err.message : String(err)),
            )
            .finally(() => setBusy(false));
        }}
      >
        <Field label={t.setup.passphrase} value={pass} onChange={setPass} autoFocus />
        {error && <Problem message={error} />}
        <Action
          disabled={!pass}
          busy={busy}
          label={t.setup.unlock}
          busyLabel={t.setup.unlocking}
          icon={<KeyRound className="size-3.5" strokeWidth={2} />}
          submit
        />
      </form>

      <button
        type="button"
        onClick={() => setShowRecovery(true)}
        className="mt-4 w-full text-[12px] text-ink-3 transition-colors hover:text-ink"
      >
        {t.setup.forgot}
      </button>
    </div>
  );
}

function UnlockPhrase({
  onDone,
  forgot,
}: {
  onDone: () => void;
  forgot?: boolean;
}) {
  const t = useT();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ready = typed.trim().split(/\s+/).filter(Boolean).length === 12;

  return (
    <div>
      <p className="eyebrow">{t.setup.locked}</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.phraseTitle}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {forgot ? t.setup.phraseForgot : t.setup.phraseMissing}
      </p>

      <textarea
        rows={3}
        spellCheck={false}
        autoComplete="off"
        autoCapitalize="off"
        value={typed}
        onChange={(e) => setTyped(e.currentTarget.value)}
        placeholder={t.setup.typePlaceholder}
        className="mt-6 w-full rounded-lg border border-rule bg-surface px-3.5 py-3 text-[13px] leading-[1.6] text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-brass"
      />

      {error && <Problem message={error} />}

      <Action
        disabled={!ready}
        busy={busy}
        label={t.setup.unlock}
        busyLabel={t.setup.unlocking}
        onClick={() => {
          setBusy(true);
          setError(null);
          api
            .unlockWithRecoveryPhrase(typed)
            .then(onDone)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            )
            .finally(() => setBusy(false));
        }}
      />

      <p className="mt-5 border-t border-rule pt-4 text-[11.5px] leading-[1.6] text-ink-3">
        <Rich text={t.setup.restoring} />
      </p>
    </div>
  );
}

function LegacyUpgrade({ onDone }: { onDone: () => void }) {
  const t = useT();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ready = typed.trim().split(/\s+/).filter(Boolean).length === 12;

  return (
    <div>
      <p className="eyebrow">{t.setup.upgradeEyebrow}</p>
      <h1 className="display mt-3 text-[30px] leading-[1.15] text-ink">
        {t.setup.upgradeTitle[0]}
        <br />
        {t.setup.upgradeTitle[1]}
      </h1>
      <p className="mt-4 text-[13px] leading-[1.65] text-ink-2">
        {t.setup.upgradeLede}
      </p>

      <textarea
        rows={3}
        spellCheck={false}
        autoComplete="off"
        autoCapitalize="off"
        value={typed}
        onChange={(e) => setTyped(e.currentTarget.value)}
        placeholder={t.setup.typePlaceholder}
        className="mt-6 w-full rounded-lg border border-rule bg-surface px-3.5 py-3 text-[13px] leading-[1.6] text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-brass"
      />

      {error && <Problem message={error} />}

      <Action
        disabled={!ready}
        busy={busy}
        label={t.setup.upgrade}
        busyLabel={t.setup.reencrypting}
        onClick={() => {
          setBusy(true);
          setError(null);
          api
            .upgradeLegacyDatabase(typed)
            .then(onDone)
            .catch((e: unknown) =>
              setError(e instanceof Error ? e.message : String(e)),
            )
            .finally(() => setBusy(false));
        }}
      />

      <p className="mt-5 border-t border-rule pt-4 text-[11.5px] leading-[1.6] text-ink-3">
        <Rich text={t.setup.noPhrase} />
      </p>
    </div>
  );
}

/* ------------------------------------------------------------- controls -- */

function Field({
  label,
  value,
  onChange,
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  autoFocus?: boolean;
}) {
  return (
    <label className="block">
      <span className="eyebrow">{label}</span>
      <input
        type="password"
        value={value}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => onChange(e.currentTarget.value)}
        className="mt-2 w-full rounded-lg border border-rule bg-surface px-3.5 py-2.5 font-mono text-[13px] text-ink outline-none transition-colors focus:border-brass"
      />
    </label>
  );
}

function Problem({ message }: { message: string }) {
  return (
    <p className="mt-3 text-[12px] leading-[1.55] text-negative">{message}</p>
  );
}

function Action({
  disabled,
  busy,
  label,
  busyLabel,
  onClick,
  icon,
  submit,
}: {
  disabled: boolean;
  busy: boolean;
  label: string;
  busyLabel: string;
  onClick?: () => void;
  icon?: React.ReactNode;
  submit?: boolean;
}) {
  return (
    <button
      type={submit ? "submit" : "button"}
      disabled={disabled || busy}
      onClick={onClick}
      className="group mt-5 flex w-full items-center justify-center gap-2 rounded-lg bg-ink px-4 py-2.5 text-[13px] font-medium text-paper transition-opacity disabled:opacity-25"
    >
      {busy ? busyLabel : label}
      {!busy &&
        (icon ?? (
          <ArrowRight
            className="size-3.5 transition-transform group-hover:translate-x-0.5"
            strokeWidth={2}
          />
        ))}
    </button>
  );
}

/** Text with `backticked` file names set in the mono face. */
function Rich({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, i) =>
        i % 2 === 1 ? (
          <span key={i} className="font-mono text-[11px]">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
  );
}
